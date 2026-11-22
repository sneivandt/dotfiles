use super::*;

#[test]
fn assessment_defers_only_permission_failures_without_reading_unresolved_targets() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let root = fixture.path().canonicalize().unwrap();
    let target = root.join("unresolved");
    let resource = fixture_resource(
        entry(&root, &target, "fragment", MergeStrategy::Toml),
        Arc::new(MockExecutor::new()),
    );
    for kind in [
        ErrorKind::PermissionDenied,
        ErrorKind::NotFound,
        ErrorKind::Other,
    ] {
        let failure = anyhow!(std::io::Error::new(kind, "fixture resolution failed"))
            .context("resolve target parent");
        let result = resource.assessment_with_target("managed = true\n", Err(failure));
        if kind == ErrorKind::PermissionDenied {
            let (state, pending_target) = result.unwrap();
            assert!(matches!(state, ResourceState::Unknown { .. }), "{state:?}");
            assert_eq!(pending_target, target);
        } else {
            let error = result.unwrap_err();
            assert!(
                format!("{error:#}").contains("fixture resolution failed"),
                "{error}"
            );
        }
        assert!(!target.exists());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    }
}

#[test]
fn unsafe_configured_targets_cannot_be_assessed_or_delegated_after_permission_failures() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let root = dunce::canonicalize(fixture.path()).unwrap();
    fs::create_dir(root.join("system")).unwrap();
    fs::write(root.join("system/fragment"), "managed = true\n").unwrap();
    for target in [
        PathBuf::from("relative/target"),
        root.clone(),
        root.parent().unwrap().join("outside"),
        root.join("missing/../target"),
    ] {
        let resource = fixture_resource(
            entry(&root, &target, "fragment", MergeStrategy::Toml),
            Arc::new(MockExecutor::new()),
        );
        let denied = anyhow!(std::io::Error::from(ErrorKind::PermissionDenied));
        assert!(
            resource
                .assessment_with_target("managed = true\n", Err(denied))
                .is_err(),
            "{target:?}"
        );
        assert!(resource.apply().is_err(), "{target:?}");
        assert!(!root.join("target").exists());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    }
}

#[cfg(unix)]
#[test]
fn protected_targets_and_parents_remain_unknown_without_executor_calls_or_preview_writes() {
    use crate::engine::{ProcessOpts, process_resources_with_cache};
    use crate::test_helpers::{ContextBuilder, task_batch};
    use std::os::unix::fs::PermissionsExt as _;

    if nix::unistd::Uid::effective().is_root() {
        return; // Root is not constrained by Unix file permission bits.
    }
    for protect_parent in [false, true] {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = fixture.path().canonicalize().unwrap();
        fs::create_dir(root.join("system")).unwrap();
        fs::write(root.join("system/fragment"), "managed = true\n").unwrap();
        let parent = root.join("protected");
        fs::create_dir(&parent).unwrap();
        let nested = parent.join("nested");
        fs::create_dir(&nested).unwrap();
        let target = nested.join("target");
        let original = "private = 'fixture secret'\nmanaged = true\n";
        fs::write(&target, original).unwrap();
        let restricted = if protect_parent { &parent } else { &target };
        let permissions = fs::metadata(restricted).unwrap().permissions();
        fs::set_permissions(restricted, fs::Permissions::from_mode(0o0)).unwrap();
        let resource = fixture_resource(
            entry(&root, &target, "fragment", MergeStrategy::Toml),
            Arc::new(MockExecutor::new()),
        );
        let observed = resource.current_state().unwrap();
        assert!(
            matches!(observed, ResourceState::Unknown { .. }),
            "{observed:?}"
        );
        let ctx = ContextBuilder::new(root.clone())
            .overlay(None)
            .build()
            .with_dry_run(true);
        let result = process_resources_with_cache(
            &ctx,
            [resource],
            &(),
            |resource, ()| {
                resource.current_state().map(|state| match state {
                    ResourceState::Unknown { reason } => {
                        ResourceState::Incorrect { current: reason }
                    }
                    unchanged @ (ResourceState::Missing
                    | ResourceState::Correct
                    | ResourceState::Incorrect { .. }
                    | ResourceState::Invalid { .. }) => unchanged,
                })
            },
            &ProcessOpts::strict("configure"),
        )
        .unwrap();
        assert_eq!(task_batch(&result).changed_count(), 1);
        let mut executor = MockExecutor::new();
        executor.expect_execute().once().returning(|spec| {
            assert_eq!(spec.program(), "sudo");
            assert_eq!(spec.arguments()[0], "-n");
            assert!(spec.is_checked());
            Ok(ExecResult::success("already-correct\n"))
        });
        let pending = fixture_resource(
            entry(&root, &target, "fragment", MergeStrategy::Toml),
            Arc::new(executor),
        );
        assert_eq!(pending.apply().unwrap(), ResourceChange::AlreadyCorrect);
        fs::set_permissions(restricted, permissions).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), original);
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
    }
}

#[test]
fn description_is_the_target_path() {
    let root = tempfile::tempdir_in(".").unwrap();
    let target = Path::new("/etc/pacman.conf");
    let resource = fixture_resource(
        entry(root.path(), target, "pacman.conf", MergeStrategy::Ini),
        Arc::new(MockExecutor::new()),
    );

    assert_eq!(resource.description(), target.display().to_string());
}

#[test]
fn merge_strategies_preserve_unmanaged_content() {
    let toml = merge_toml(
        "other = true\n[browser]\nkeep = 1\n",
        "[browser]\nmanaged = 2\n",
    )
    .unwrap();
    assert!(toml.contains("other = true"));
    assert!(toml.contains("keep = 1"));
    assert!(contains_toml(&toml, "[browser]\nmanaged = 2\n").unwrap());

    let ini = merge_ini(
        "[boot]\ncommand=x\nsystemd=false\n",
        "[boot]\nsystemd=true\n",
    )
    .unwrap();
    assert!(ini.contains("command=x"));
    assert!(contains_ini(&ini, "[boot]\nsystemd=true\n").unwrap());

    let pam = merge_pam(
        "auth include system-auth\nauth optional old.so\n",
        "auth optional keyring.so\n",
    )
    .unwrap();
    assert!(pam.contains("auth optional old.so"));
    assert!(pam.ends_with("auth optional keyring.so\n"));
}

#[test]
fn typo_in_fragment_control_cannot_replace_a_working_login_rule() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let root = fixture.path().canonicalize().unwrap();
    let target = root.join("login");
    let installed = "# kiosk login policy\nauth required pam_permit.so\n";
    fs::write(&target, installed).unwrap();
    fs::create_dir(root.join("system")).unwrap();
    let fragment = root.join("system/login");
    let resource = fixture_resource(
        entry(&root, &target, "login", MergeStrategy::Pam),
        Arc::new(MockExecutor::new()),
    );
    for rule in [
        "auth requried pam_permit.so\n",
        "aut required pam_permit.so\n",
    ] {
        fs::write(&fragment, rule).unwrap();
        assert!(validate_fragment(MergeStrategy::Pam, rule).is_err());
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Invalid { .. }
        ));
        assert!(resource.apply().is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), installed);
    }
    fs::write(fragment, "auth required pam_permit.so\n").unwrap();
    assert_eq!(resource.apply().unwrap(), ResourceChange::AlreadyCorrect);
}

#[test]
fn malformed_fragments_are_invalid_for_missing_and_existing_targets() {
    for (merge, fragment, reason) in [
        (MergeStrategy::Toml, "managed = [\n", "parsing TOML"),
        (MergeStrategy::Ini, "managed=true\n", "outside a section"),
        (MergeStrategy::Ini, "[options]\n = value\n", "empty key"),
        (MergeStrategy::Ini, "[options]\n = \n", "empty key"),
        (
            MergeStrategy::Ini,
            "[options]\n[broken\n",
            "invalid section header",
        ),
        (
            MergeStrategy::Ini,
            "[]\nkey=value\n",
            "invalid section header",
        ),
        (MergeStrategy::Ini, "[ \t]\n", "invalid section header"),
        (MergeStrategy::Ini, "[[nested]]\n", "invalid section header"),
        (
            MergeStrategy::Ini,
            "[broken]extra]\n",
            "invalid section header",
        ),
        (
            MergeStrategy::Ini,
            "[options]\nmanaged=true\nmanaged=false\n",
            "conflicting values",
        ),
    ] {
        for existing in [false, true] {
            let root = tempfile::tempdir_in(".").unwrap();
            fs::create_dir(root.path().join("system")).unwrap();
            fs::write(root.path().join("system/fragment"), fragment).unwrap();
            let target = root.path().join("target");
            let original = "# Keep existing content\n";
            if existing {
                fs::write(&target, original).unwrap();
            }
            let resource = fixture_resource(
                entry(root.path(), &target, "fragment", merge),
                Arc::new(MockExecutor::new()),
            );

            assert!(
                matches!(
                    resource.current_state().unwrap(),
                    ResourceState::Invalid { reason: detail } if detail.contains(reason)
                ),
                "{merge:?}, existing={existing}"
            );
            let error = resource.apply().unwrap_err();
            assert!(error.to_string().contains(reason), "{error}");
            assert!(!root.path().join("staging-must-not-be-created").exists());
            if existing {
                assert_eq!(fs::read_to_string(&target).unwrap(), original);
            } else {
                assert!(!target.exists());
            }
        }
    }
}

#[test]
fn system_file_dry_runs_preserve_missing_and_existing_targets() {
    use crate::engine::{ProcessOpts, process_resources};
    use crate::test_helpers::{ContextBuilder, task_batch};

    for (merge, fragment, current) in [
        (MergeStrategy::Toml, "managed = true\n", None),
        (
            MergeStrategy::Toml,
            "managed = true\n",
            Some("managed = false\nunmanaged = 'keep'\n"),
        ),
        (MergeStrategy::Ini, "[options]\nmanaged=true\n", None),
        (
            MergeStrategy::Ini,
            "[options]\nmanaged=true\n",
            Some("[options]\nmanaged=false\n# retain comment\nunmanaged=keep\n"),
        ),
        (
            MergeStrategy::Pam,
            "auth optional keyring.so\n",
            Some("# retain comment\nauth include system-auth\n"),
        ),
    ] {
        let root = tempfile::tempdir_in(".").unwrap();
        fs::create_dir(root.path().join("system")).unwrap();
        fs::write(root.path().join("system/fragment"), fragment).unwrap();
        let parent = root.path().join("target-parent");
        let target = parent.join("target");
        if let Some(current) = current {
            fs::create_dir(&parent).unwrap();
            fs::write(&target, current).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
            }
        }
        let original_metadata = fs::metadata(&target).ok();
        let staging = root.path().join("staging-must-not-be-created");
        let resource = fixture_resource(
            entry(root.path(), &target, "fragment", merge),
            Arc::new(MockExecutor::new()),
        );
        let initial_state = resource.current_state().unwrap();
        if current.is_some() {
            assert!(
                matches!(
                    initial_state,
                    ResourceState::Incorrect { current: difference }
                        if difference.contains("managed content differs")
                ),
                "{merge:?}"
            );
        } else {
            assert_eq!(initial_state, ResourceState::Missing);
        }
        let ctx = ContextBuilder::new(root.path().to_path_buf())
            .overlay(None)
            .build()
            .with_dry_run(true);

        let result =
            process_resources(&ctx, [resource], &ProcessOpts::strict("configure")).unwrap();

        let stats = task_batch(&result);
        assert_eq!(stats.changed_count(), 1, "{merge:?}, current={current:?}");
        assert_eq!(stats.already_ok_count(), 0);
        assert_eq!(stats.failed_count(), 0);
        assert_eq!(stats.skipped_count(), 0);
        if let Some(current) = current {
            assert_eq!(fs::read_to_string(&target).unwrap(), current);
            let before = original_metadata.unwrap();
            let after = fs::metadata(&target).unwrap();
            assert_eq!(after.permissions(), before.permissions());
            assert_eq!(after.modified().unwrap(), before.modified().unwrap());
            assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
        } else {
            assert!(!target.exists());
            assert!(!parent.exists(), "preview must not create target parents");
        }
        assert!(!staging.exists());
        assert_eq!(
            fs::read_to_string(root.path().join("system/fragment")).unwrap(),
            fragment
        );
    }
}

#[test]
fn nonregular_system_file_targets_are_never_followed_or_replaced() {
    for kind in ["directory", "file link", "directory link", "dangling link"] {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = fixture.path().canonicalize().unwrap();
        fs::create_dir(root.join("system")).unwrap();
        fs::write(root.join("system/fragment"), "managed = true\n").unwrap();
        let target = root.join("target");
        let user = root.join("user");
        fs::create_dir(&user).unwrap();
        let user_file = user.join("content");
        fs::write(&user_file, "user content").unwrap();
        match kind {
            "directory" => {
                fs::create_dir(&target).unwrap();
                fs::write(target.join("content"), "target content").unwrap();
            }
            "file link" => {
                crate::infra::fs::create_native_symlink(&user_file, &target, false).unwrap();
            }
            "directory link" => {
                crate::infra::fs::create_native_symlink(&user, &target, true).unwrap();
            }
            "dangling link" => {
                crate::infra::fs::create_native_symlink(&root.join("absent"), &target, false)
                    .unwrap();
            }
            _ => panic!("unexpected target kind: {kind}"),
        }
        let original_link = fs::read_link(&target).ok();
        let resource = fixture_resource(
            entry(&root, &target, "fragment", MergeStrategy::Toml),
            Arc::new(MockExecutor::new()),
        );

        assert!(
            matches!(
                resource.current_state().unwrap(),
                ResourceState::Invalid { reason } if reason.contains("not a regular file")
            ),
            "{kind}"
        );
        let error = resource.apply().unwrap_err();
        assert!(
            error.to_string().contains("not a regular file"),
            "{kind}: {error}"
        );
        assert_eq!(fs::read_link(&target).ok(), original_link, "{kind}");
        if kind == "directory" {
            assert!(target.symlink_metadata().unwrap().is_dir());
            assert_eq!(
                fs::read_to_string(target.join("content")).unwrap(),
                "target content"
            );
        } else {
            assert!(target.symlink_metadata().unwrap().is_symlink(), "{kind}");
        }
        assert_eq!(fs::read_to_string(&user_file).unwrap(), "user content");
        assert!(!root.join("staging-must-not-be-created").exists());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 3);
    }
}

#[test]
fn system_file_discovery_errors_never_become_missing_or_mutate_files() {
    for failure in [
        "missing fragment",
        "fragment encoding",
        "target encoding",
        "target metadata",
    ] {
        let root = tempfile::tempdir_in(".").unwrap();
        fs::create_dir(root.path().join("system")).unwrap();
        let source = root.path().join("system").join("fragment");
        let target = root.path().join("target");
        let fragment = if failure == "fragment encoding" {
            &b"\xff"[..]
        } else {
            &b"managed = true\n"[..]
        };
        if failure != "missing fragment" {
            fs::write(&source, fragment).unwrap();
        }
        let current = if failure == "target encoding" {
            &b"\xff"[..]
        } else {
            &b"unmanaged = 'keep'\n"[..]
        };
        fs::write(&target, current).unwrap();
        let resource_target = if failure == "target metadata" {
            root.path().join("invalid\0target")
        } else {
            target.clone()
        };
        let resource = fixture_resource(
            entry(
                root.path(),
                &resource_target,
                "fragment",
                MergeStrategy::Toml,
            ),
            Arc::new(MockExecutor::new()),
        );

        for error in [
            resource.current_state().unwrap_err(),
            resource.apply().unwrap_err(),
        ] {
            let message = format!("{error:#}");
            let expected_path = if failure.contains("fragment") {
                source.clone()
            } else {
                crate::infra::fs::resolve_target_entry(&resource_target).unwrap()
            };
            assert!(
                message.contains(&format!("reading {}", expected_path.display()))
                    || message
                        .contains(&format!("reading metadata for {}", expected_path.display())),
                "{failure}: {message}"
            );
        }
        assert_eq!(fs::read(&target).unwrap(), current, "{failure}");
        if failure == "missing fragment" {
            assert!(!source.exists());
        } else {
            assert_eq!(fs::read(&source).unwrap(), fragment, "{failure}");
        }
        assert!(!root.path().join("staging-must-not-be-created").exists());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    }
}

#[test]
fn malformed_target_is_invalid_without_being_replaced() {
    let root = tempfile::tempdir_in(".").unwrap();
    fs::create_dir(root.path().join("system")).unwrap();
    fs::write(root.path().join("system/fragment.toml"), "managed = true\n").unwrap();
    let target = root.path().join("target.toml");
    fs::write(&target, "not = [valid\n").unwrap();
    let resource = fixture_resource(
        entry(root.path(), &target, "fragment.toml", MergeStrategy::Toml),
        Arc::new(MockExecutor::new()),
    );

    assert!(matches!(
        resource.current_state().unwrap(),
        ResourceState::Invalid { .. }
    ));
    assert!(
        resource.apply().is_err(),
        "malformed content must not be installed"
    );
    assert_eq!(fs::read_to_string(target).unwrap(), "not = [valid\n");
}
