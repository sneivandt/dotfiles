use super::*;
use crate::infra::env::MapEnv;
use crate::infra::exec::MockExecutor;
use crate::infra::platform::{Os, Platform};

#[test]
fn profile_publication_preserves_intervening_edits_and_new_files() {
    for existing in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join(".profile");
        if existing {
            std::fs::write(&path, "initial\n").unwrap();
        }
        let before = PathEntryResource::shell_profile_contents(&path).unwrap();
        std::fs::write(&path, "external edit\n").unwrap();
        let error =
            PathEntryResource::publish_shell_profile(&path, &before, "stale replacement".into())
                .unwrap_err();
        assert!(
            error.to_string().contains("changed during update"),
            "{error:#}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "external edit\n");
        assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 1);
    }
}

#[cfg(windows)]
#[test]
fn registry_path_encoding_is_utf16_little_endian_with_one_terminator() {
    assert_eq!(encode_reg_string(""), [0, 0]);
    assert_eq!(
        encode_reg_string("Aé🦀"),
        [65, 0, 233, 0, 62, 216, 128, 221, 0, 0]
    );
}

#[cfg(windows)]
#[test]
fn registry_path_decoding_preserves_supported_string_values() {
    use winreg::enums::{REG_EXPAND_SZ, REG_SZ};

    for vtype in [REG_SZ, REG_EXPAND_SZ] {
        for content in ["", r"%USERPROFILE%\bin;C:\café"] {
            for terminated in [false, true] {
                let mut bytes = encode_reg_string(content);
                if !terminated {
                    bytes.truncate(bytes.len() - 2);
                }
                let value = winreg::RegValue {
                    bytes: bytes.into(),
                    vtype: vtype.clone(),
                };
                assert_eq!(decode_reg_string(&value).unwrap(), content);
            }
        }
    }
}

#[cfg(windows)]
#[test]
fn registry_path_decoding_rejects_values_that_would_be_lossily_rewritten() {
    use winreg::enums::{REG_BINARY, REG_DWORD, REG_EXPAND_SZ, REG_MULTI_SZ, REG_SZ};

    for (vtype, bytes, detail) in [
        (
            REG_BINARY,
            encode_reg_string("path"),
            "unsupported registry type",
        ),
        (REG_DWORD, vec![1, 0, 0, 0], "unsupported registry type"),
        (
            REG_MULTI_SZ,
            encode_reg_string("path"),
            "unsupported registry type",
        ),
        (REG_SZ, vec![b'C'], "incomplete UTF-16 code unit"),
        (REG_EXPAND_SZ, vec![0, 0xd8, 0, 0], "decoding HKCU"),
        (REG_SZ, encode_reg_string("first\0second"), "embedded NUL"),
    ] {
        let value = winreg::RegValue {
            bytes: bytes.into(),
            vtype,
        };
        let error = decode_reg_string(&value).unwrap_err();
        assert!(format!("{error:#}").contains(detail), "{error:#}");
    }
}

#[cfg(not(windows))]
#[test]
fn native_registry_backend_rejects_non_windows_access() {
    let store = RegistryPath {
        env: MapEnv::new().into_handle(),
    };
    for result in [store.contains("fixture"), store.append("fixture")] {
        assert_eq!(
            result.unwrap_err().to_string(),
            "user PATH registry access is only supported on Windows"
        );
    }
}

fn windows_resource(home: &Path, store: MockUserPath) -> PathEntryResource {
    let executor = MockExecutor::new();
    #[cfg(windows)]
    let executor = {
        let mut executor = executor;
        executor
            .expect_which()
            .with(mockall::predicate::eq("pwsh"))
            .returning(|_| true);
        executor
            .expect_execute()
            .once()
            .withf(|spec| spec.program() == "pwsh")
            .returning(|_| Ok(crate::infra::exec::ExecResult::success("")));
        executor
    };
    let env = MapEnv::new()
        .with("PATH", home.join(".local/bin").into_os_string())
        .into_handle();
    let mut resource = PathEntryResource::new(
        home,
        Platform::new(Os::Windows, false),
        Arc::new(executor),
        env,
    );
    if let PathStrategy::WindowsRegistry { store: backend, .. } = &mut resource.strategy {
        *backend = Arc::new(store);
    }
    resource
}

#[test]
fn transient_runtime_path_does_not_skip_persistence_and_repeated_apply_is_current() {
    let home = tempfile::tempdir().unwrap();
    let mut store = MockUserPath::new();
    let mut sequence = mockall::Sequence::new();
    store
        .expect_contains()
        .once()
        .in_sequence(&mut sequence)
        .returning(|_| Ok(false));
    store
        .expect_append()
        .once()
        .in_sequence(&mut sequence)
        .returning(|_| Ok(true));
    store
        .expect_contains()
        .once()
        .in_sequence(&mut sequence)
        .returning(|_| Ok(true));
    store
        .expect_append()
        .once()
        .in_sequence(&mut sequence)
        .returning(|_| Ok(false));
    let resource = windows_resource(home.path(), store);
    assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    assert_eq!(resource.apply().unwrap(), ResourceChange::AlreadyCorrect);
}

#[test]
fn persisted_path_read_and_write_errors_cannot_report_success() {
    let home = tempfile::tempdir().unwrap();
    let mut store = MockUserPath::new();
    store
        .expect_contains()
        .once()
        .returning(|_| anyhow::bail!("fixture registry read failure"));
    store
        .expect_append()
        .once()
        .returning(|_| anyhow::bail!("fixture registry write failure"));
    let resource = PathEntryResource {
        dir: home.path().join(".local/bin"),
        display_home: home.path().to_path_buf(),
        strategy: PathStrategy::WindowsRegistry {
            dir: "fixture".into(),
            store: Arc::new(store),
            executor: Arc::new(MockExecutor::new()),
        },
    };
    assert_eq!(
        resource.current_state().unwrap(),
        ResourceState::Unknown {
            reason: "fixture registry read failure".into()
        }
    );
    assert!(resource.apply().is_err());
}

#[test]
fn registry_preview_only_reads_persisted_state() {
    let home = tempfile::tempdir().unwrap();
    let mut store = MockUserPath::new();
    store.expect_contains().once().returning(|_| Ok(false));
    let resource = PathEntryResource {
        dir: home.path().join(".local/bin"),
        display_home: home.path().to_path_buf(),
        strategy: PathStrategy::WindowsRegistry {
            dir: "fixture".into(),
            store: Arc::new(store),
            executor: Arc::new(MockExecutor::new()),
        },
    };
    let ctx = crate::test_helpers::make_linux_context(home.path().into(), None).with_dry_run(true);
    let result = crate::engine::process_resources(
        &ctx,
        [resource],
        &crate::engine::ProcessOpts::strict("configure"),
    )
    .unwrap();
    assert_eq!(crate::test_helpers::task_batch(&result).changed_count(), 1);
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
}
#[test]
fn persisted_entries_match_expanded_windows_paths_without_runtime_path() {
    let env = MapEnv::new().with("USERPROFILE", r"C:\Users\Fixture User");
    for path in [
        r"%USERPROFILE%/.local/bin",
        r"c:\users\fixture user\.local\bin\",
    ] {
        assert!(
            path_contains_entry(path, r"C:\Users\Fixture User\.local\bin", true, &env),
            "{path}"
        );
    }
    assert!(!path_contains_entry(
        r"C:\Other\bin",
        r"C:\Users\Fixture User\.local\bin",
        true,
        &env
    ));
    assert_eq!(expand_env_vars(r"%UNKNOWN%\bin", &env), r"%UNKNOWN%\bin");
}

#[test]
fn persisted_entries_do_not_expand_the_literal_directory_being_added() {
    let env = MapEnv::new().with("USERNAME", "Expanded");
    assert!(!path_contains_entry(
        r"C:\Users\Expanded\.local\bin",
        r"C:\Users\%USERNAME%\.local\bin",
        true,
        &env
    ));
}

#[test]
fn path_append_rejects_literal_directories_that_cannot_converge() {
    let env = MapEnv::new().with("USERNAME", "Expanded");
    for existing in ["", r"%USERPROFILE%\bin;C:\Windows"] {
        for (dir, expandable) in [
            (r"C:\Users\%USERNAME%\.local\bin", true),
            (r"C:\Users\semi;colon\.local\bin", true),
            (r"C:\Users\semi;colon\.local\bin", false),
        ] {
            let error = appended_path(existing, dir, expandable, &env)
                .expect_err("an unrepresentable directory must fail before registry writes");
            assert!(error.to_string().contains("literal directory"), "{error:#}");
        }
    }
}

#[test]
fn path_append_preserves_existing_entries_and_converges_on_repeat() {
    let env = MapEnv::new().with("USERNAME", "Expanded");
    for (dir, expandable) in [
        (r"C:\Users\%USERNAME%\.local\bin", false),
        (r"C:\Users\Fixture\.local\bin", true),
        (r"C:\Users\Fixture\.local\bin", false),
    ] {
        for existing in ["", r"%USERPROFILE%\bin;C:\Windows"] {
            let updated = appended_path(existing, dir, expandable, &env)
                .unwrap()
                .unwrap();
            assert_eq!(
                updated,
                if existing.is_empty() {
                    dir.to_string()
                } else {
                    format!("{existing};{dir}")
                },
                "unrelated entries must remain byte-for-byte unchanged"
            );
            assert!(
                appended_path(&updated, dir, expandable, &env)
                    .unwrap()
                    .is_none(),
                "the persisted addition must make the next apply a no-op"
            );
        }
    }
}

#[test]
fn persisted_entries_expand_only_expandable_values_before_splitting() {
    let env = MapEnv::new()
        .with("USERPROFILE", r"C:\Users\Fixture")
        .with("TOOLS", r"C:\Other;C:\Users\Fixture\.local\bin");
    for (path, dir, expandable, expected) in [
        (
            r"%USERPROFILE%\.local\bin",
            r"C:\Users\Fixture\.local\bin",
            false,
            false,
        ),
        (
            r"%USERPROFILE%\.local\bin",
            r"C:\Users\Fixture\.local\bin",
            true,
            true,
        ),
        (
            r"%USERPROFILE%\.local\bin",
            r"%USERPROFILE%\.local\bin",
            false,
            true,
        ),
        (r"%TOOLS%", r"C:\Users\Fixture\.local\bin", true, true),
    ] {
        assert_eq!(
            path_contains_entry(path, dir, expandable, &env),
            expected,
            "{path:?}, {dir:?}, expandable={expandable}"
        );
    }
}

#[test]
fn profile_discovery_propagates_non_missing_read_errors() {
    let home = tempfile::tempdir_in(".").unwrap();
    let path = home.path().join("invalid\0profile");
    assert_ne!(
        std::fs::read_to_string(&path).unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    let resource = PathEntryResource {
        dir: home.path().join(".local").join("bin"),
        display_home: home.path().to_path_buf(),
        strategy: PathStrategy::ShellProfile { path },
    };

    assert!(
        resource.current_state().is_err(),
        "a profile inspection failure is not an absent managed PATH entry"
    );
    assert!(resource.apply().is_err());
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
}
