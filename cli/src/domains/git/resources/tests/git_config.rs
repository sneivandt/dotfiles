use super::*;

#[test]
fn grouped_update_preserves_include_precedence_and_converges() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config");
    let include = dir.path().join("included");
    let included = "[user]\nname = Included\nemail = included@test.local\n";
    std::fs::write(&include, included).unwrap();
    std::fs::write(&path, "[user]\nname = Managed\n").unwrap();
    let settings = [
        GitConfigResource::new("user.name".into(), "Managed".into()),
        GitConfigResource::new("user.email".into(), "managed@test.local".into()),
        GitConfigResource::new("include.path".into(), "included".into()),
    ];

    let plan = GitConfigPlan::inspect(&path, &settings).unwrap();
    assert_eq!(plan.changed().len(), settings.len());
    plan.apply(&path).unwrap();

    let config = open_file(&path).unwrap();
    assert_eq!(config.get_string("user.name").unwrap(), "Managed");
    assert_eq!(
        config.get_string("user.email").unwrap(),
        "managed@test.local"
    );
    assert_eq!(std::fs::read_to_string(&include).unwrap(), included);
    assert!(
        GitConfigPlan::inspect(&path, &settings)
            .unwrap()
            .changed()
            .is_empty()
    );
}

#[test]
fn grouped_update_refuses_a_stale_plan() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config");
    std::fs::write(&path, "[user]\nname = Original\n").unwrap();
    let settings = [
        GitConfigResource::new("user.name".into(), "Managed".into()),
        GitConfigResource::new("core.editor".into(), "nvim".into()),
    ];
    let plan = GitConfigPlan::inspect(&path, &settings).unwrap();
    let concurrent_edit = "[user]\nname = Edited\n";
    std::fs::write(&path, concurrent_edit).unwrap();

    let error = plan.apply(&path).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("global Git config changed during update")
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), concurrent_edit);
}

#[test]
fn config_reads_distinguish_absence_from_other_io_failures() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing");
    assert_eq!(read_config_bytes(&missing).unwrap(), None);
    std::fs::write(&missing, "fixture").unwrap();
    assert_eq!(
        read_config_bytes(&missing).unwrap(),
        Some(b"fixture".to_vec())
    );
    let error = read_config_bytes(dir.path()).unwrap_err();
    assert!(format!("{error:#}").contains("Git config is not a regular file"));
}

#[test]
fn config_metadata_errors_are_not_treated_as_missing() {
    let error = require_regular_config(Path::new("config\0invalid")).unwrap_err();
    assert!(format!("{error:#}").contains("inspecting Git config"));
}

#[test]
fn only_include_path_keys_control_config_discovery() {
    for (key, expected) in [
        ("include.path", true),
        ("includeif.gitdir:/fixture/.path", true),
        ("include.other", false),
        ("includeif.gitdir:/fixture/.other", false),
        ("core.path", false),
    ] {
        assert_eq!(is_include(key), expected, "{key}");
    }
}

#[test]
fn transactions_respect_git_locks_and_retry_with_new_desired_values() {
    for include in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        let lock = dir.path().join("config.lock");
        let before = if include {
            "[include]\n path = included\n"
        } else {
            "[core]\n editor = before\n"
        };
        std::fs::write(&path, before).unwrap();
        std::fs::write(dir.path().join("included"), "[core]\n editor = inherited\n").unwrap();
        std::fs::write(&lock, "foreign writer").unwrap();
        let blocked =
            GitConfigResource::with_config_path("core.editor".into(), "first".into(), path.clone());
        assert!(format!("{:#}", blocked.apply().unwrap_err()).contains("Git config lock"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        assert_eq!(std::fs::read_to_string(&lock).unwrap(), "foreign writer");
        std::fs::remove_file(&lock).unwrap();
        for value in ["first", "second"] {
            let resource = GitConfigResource::with_config_path(
                "core.editor".into(),
                value.into(),
                path.clone(),
            );
            resource.apply().unwrap();
            assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
            assert!(
                !lock.exists(),
                "completed transactions release their Git lock"
            );
        }
        assert_eq!(
            std::fs::read_to_string(dir.path().join("included")).unwrap(),
            "[core]\n editor = inherited\n"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}

#[test]
fn failed_transactions_preserve_content_and_release_lock_before_retry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config");
    let before = "[core]\n editor = before\n";
    std::fs::write(&path, before).unwrap();
    let invalid =
        GitConfigResource::with_config_path("invalid-key".into(), "value".into(), path.clone());
    assert!(invalid.apply().is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    let valid = GitConfigResource::with_config_path("core.editor".into(), "after".into(), path);
    valid.apply().unwrap();
    assert_eq!(valid.current_state().unwrap(), ResourceState::Correct);
}

#[cfg(unix)]
#[test]
fn every_git_write_preserves_existing_modes_and_new_files_are_private() {
    use std::os::unix::fs::PermissionsExt as _;
    for include in [false, true] {
        for mode in [None, Some(0o600), Some(0o640)] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config");
            if let Some(mode) = mode {
                std::fs::write(
                    &path,
                    if include {
                        "[include]\n path = included\n"
                    } else {
                        "[core]\n editor = before\n"
                    },
                )
                .unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            }
            std::fs::write(dir.path().join("included"), "[core]\n editor = inherited\n").unwrap();
            let alias = dir.path().join("alias");
            if mode.is_some() {
                std::os::unix::fs::symlink(&path, &alias).unwrap();
            }
            let resource = GitConfigResource::with_config_path(
                "core.editor".into(),
                "after".into(),
                if mode.is_some() {
                    alias.clone()
                } else {
                    path.clone()
                },
            );
            resource.apply().unwrap();
            assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
            assert_eq!(
                path.metadata().unwrap().permissions().mode() & 0o777,
                mode.unwrap_or(0o600)
            );
            if mode.is_some() {
                assert!(alias.is_symlink());
            }
        }
    }
}

#[cfg(any(unix, windows))]
#[test]
fn git_config_is_private_before_staging_any_user_content() {
    fn assert_private(path: &Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(path.metadata().unwrap().permissions().mode() & 0o077, 0);
        }
        #[cfg(windows)]
        crate::infra::fs::check_private_fixture(path).unwrap();
    }

    struct PrivateContent<'a>(&'a Path);
    impl AsRef<[u8]> for PrivateContent<'_> {
        fn as_ref(&self) -> &[u8] {
            let entries = std::fs::read_dir(self.0).unwrap().collect::<Vec<_>>();
            assert_eq!(entries.len(), 1, "exactly one staging directory must exist");
            let directory = entries[0].as_ref().unwrap();
            assert!(directory.metadata().unwrap().is_dir());
            assert_private(&directory.path());
            let files = std::fs::read_dir(directory.path())
                .unwrap()
                .collect::<Vec<_>>();
            assert_eq!(files.len(), 1, "exactly one staging file must exist");
            let metadata = files[0].as_ref().unwrap().metadata().unwrap();
            assert_private(&files[0].as_ref().unwrap().path());
            assert_eq!(metadata.len(), 0, "privacy must precede the first write");
            b"[fixture]\n private = synthetic-value\n"
        }
    }

    let directory = tempfile::tempdir().unwrap();
    let (staged, path) = stage_config(directory.path(), PrivateContent(directory.path())).unwrap();
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"[fixture]\n private = synthetic-value\n"
    );
    open_file(&path)
        .unwrap()
        .set_str("fixture.extra", "value")
        .unwrap();
    assert_private(staged.path());
    // libgit2 committed its sibling lock file by renaming it to this path.
    // Unix relies on the enclosing 0700 directory; Windows must also
    // protect the new file itself because traversal checks may be bypassed.
    #[cfg(windows)]
    assert_private(&path);
    assert_eq!(
        open_file(&path)
            .unwrap()
            .get_string("fixture.extra")
            .unwrap(),
        "value"
    );
    let mut lock_name = path.as_os_str().to_owned();
    lock_name.push(".lock");
    let lock_path = PathBuf::from(lock_name);
    std::fs::write(&lock_path, []).unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(
        open_file(&path)
            .unwrap()
            .set_str("fixture.extra", "blocked")
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    drop(staged);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn implicit_local_value_with_includes_fails_without_rewriting_either_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config");
    let included = dir.path().join("included");
    let contents = "[core]\n autocrlf\n[include]\n path = included\n";
    std::fs::write(&path, contents).unwrap();
    std::fs::write(&included, "[core]\n autocrlf = true\n").unwrap();
    let resource =
        GitConfigResource::with_config_path("core.autocrlf".into(), "false".into(), path.clone());
    assert!(format!("{:#}", resource.apply().unwrap_err()).contains("implicit values"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
    assert_eq!(
        std::fs::read_to_string(&included).unwrap(),
        "[core]\n autocrlf = true\n"
    );
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        2,
        "failed staging must leave no temporary config"
    );
}

#[test]
fn included_values_are_preserved_and_global_overrides_converge() {
    for local in [
        "",
        "[core]\n autocrlf = true\n",
        "[core]\n autocrlf = false\n",
    ] {
        for include_first in [false, true] {
            let dir = tempfile::Builder::new()
                .prefix("git settings ")
                .tempdir()
                .unwrap();
            let path = dir.path().join("config");
            let included = dir.path().join("included");
            let content = "[core]\n autocrlf = true\n editor = user-editor\n";
            std::fs::write(&included, content).unwrap();
            let include = "[include]\n path = included\n";
            std::fs::write(
                &path,
                if include_first {
                    format!("{include}{local}")
                } else {
                    format!("{local}{include}")
                },
            )
            .unwrap();
            let resource = GitConfigResource::with_config_path(
                "core.autocrlf".into(),
                "false".into(),
                path.clone(),
            );
            assert_eq!(
                resource
                    .open_config()
                    .unwrap()
                    .get_string("core.editor")
                    .unwrap(),
                "user-editor",
                "relative includes must load from the configuration directory"
            );
            assert_eq!(
                resource.current_state().unwrap() == ResourceState::Correct,
                include_first && local.contains("false"),
                "a later include must not override a managed scalar value"
            );
            resource.apply().unwrap();
            assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
            let after = std::fs::read(&path).unwrap();
            assert_eq!(
                open_file(&path)
                    .unwrap()
                    .get_string("core.autocrlf")
                    .unwrap(),
                "false"
            );
            assert_eq!(std::fs::read_to_string(&included).unwrap(), content);
            assert_eq!(std::fs::read(&path).unwrap(), after);
            let absent =
                GitConfigResource::absent("core.autocrlf".into()).using_config_path(path.clone());
            absent.apply().unwrap();
            assert_eq!(absent.current_state().unwrap(), ResourceState::Correct);
            assert_eq!(
                open_file(&path)
                    .unwrap()
                    .get_string("core.autocrlf")
                    .unwrap(),
                "true"
            );
            assert_eq!(std::fs::read_to_string(&included).unwrap(), content);
        }
    }
}

fn git_config_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[test]
fn plan_omits_already_absent_settings_on_include_change_and_reports_claims_and_read_errors() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("gitconfig");
    let included = dir.path().join("extra.gitconfig");
    std::fs::write(&included, "[user]\n name = Alice\n").unwrap();
    std::fs::write(&config_path, "[core]\n autocrlf = false\n").unwrap();

    let include = GitConfigResource::with_config_path(
        "include.path".into(),
        git_config_path(&included),
        config_path.clone(),
    );
    let already_absent =
        GitConfigResource::absent("user.email".into()).using_config_path(config_path.clone());
    assert_eq!(include.access_claims(), vec![config_access_claim()]);

    let plan = GitConfigPlan::inspect(&config_path, &[include, already_absent]).unwrap();
    assert_eq!(plan.changed().len(), 1);
    assert_eq!(plan.changed()[0].key, "include.path");

    #[cfg(unix)]
    if !nix::unistd::Uid::effective().is_root() {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&config_path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let result = read_config_bytes(&config_path);
        std::fs::set_permissions(&config_path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            result.is_err(),
            "permission error reading git config must not be treated as absent"
        );
    }
}

#[test]
fn nested_include_defining_include_path_updates_and_removes_depth_zero_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config");
    let outer = dir.path().join("outer.gitconfig");
    let inner = dir.path().join("inner.gitconfig");
    let work = dir.path().join("work.gitconfig");
    let updated = dir.path().join("updated.gitconfig");

    let inner_content = "[user]\n\tname = Inner\n";
    let outer_content = format!("[include]\n\tpath = {}\n", git_config_path(&inner));
    let work_content = "[user]\n\temail = work@test.local\n";
    let updated_content = "[user]\n\tname = Updated\n";
    std::fs::write(&inner, inner_content).unwrap();
    std::fs::write(&outer, &outer_content).unwrap();
    std::fs::write(&work, work_content).unwrap();
    std::fs::write(&updated, updated_content).unwrap();

    let initial = format!(
        "[include]\n\tpath = {}\n[includeIf \"gitdir:/work/\"]\n\tpath = {}\n[user]\n\tname = Managed\n",
        git_config_path(&outer),
        git_config_path(&work)
    );
    std::fs::write(&config_path, &initial).unwrap();

    // Update depth-0 `include.path` alongside a managed scalar value while
    // the currently included `outer.gitconfig` also defines `include.path`.
    let settings = vec![
        GitConfigResource::with_config_path(
            "include.path".into(),
            git_config_path(&updated),
            config_path.clone(),
        ),
        GitConfigResource::with_config_path(
            "user.name".into(),
            "Managed".into(),
            config_path.clone(),
        ),
    ];
    let plan = GitConfigPlan::inspect(&config_path, &settings).unwrap();
    assert_eq!(plan.apply(&config_path).unwrap(), ResourceChange::Applied);

    let rewritten = std::fs::read_to_string(&config_path).unwrap();
    let include_pos = rewritten
        .find("[include]")
        .expect("depth-0 [include] section must remain");
    let include_if_pos = rewritten
        .find("[includeIf \"gitdir:/work/\"]")
        .expect("depth-0 [includeIf] section must remain");
    let user_pos = rewritten
        .rfind("[user]")
        .expect("managed [user] section must remain");
    assert!(
        include_pos < include_if_pos,
        "in-place edit must preserve [include] before [includeIf]:\n{rewritten}"
    );
    assert!(
        include_if_pos < user_pos,
        "managed values must stay after include directives:\n{rewritten}"
    );
    for setting in &settings {
        assert_eq!(setting.current_state().unwrap(), ResourceState::Correct);
    }
    assert_eq!(std::fs::read_to_string(&outer).unwrap(), outer_content);
    assert_eq!(std::fs::read_to_string(&inner).unwrap(), inner_content);

    // Removing depth-0 `include.path` must also succeed when the included
    // file itself defines `include.path`.
    std::fs::write(&config_path, &initial).unwrap();
    let remove =
        GitConfigResource::absent("include.path".into()).using_config_path(config_path.clone());
    assert_eq!(remove.apply().unwrap(), ResourceChange::Applied);
    assert_eq!(remove.current_state().unwrap(), ResourceState::Correct);
    let after_remove = std::fs::read_to_string(&config_path).unwrap();
    assert!(
        after_remove.contains("[includeIf \"gitdir:/work/\"]"),
        "sibling includeIf must remain after removing depth-0 include.path:\n{after_remove}"
    );
    assert_eq!(std::fs::read_to_string(&outer).unwrap(), outer_content);
    assert_eq!(std::fs::read_to_string(&inner).unwrap(), inner_content);
}

#[test]
fn open_failure_names_the_key() {
    let dir = tempfile::tempdir().unwrap();
    // A directory can never be opened as a config file.
    let resource = GitConfigResource::with_config_path(
        "core.autocrlf".to_string(),
        "false".to_string(),
        dir.path().to_path_buf(),
    );

    let error = resource
        .apply()
        .expect_err("opening a directory as a config file must fail");

    let rendered = format!("{:#}", anyhow::Error::new(error));
    assert!(
        rendered.contains("core.autocrlf"),
        "error must name the failing key, got: {rendered}"
    );
}

#[test]
fn description_format() {
    let resource = GitConfigResource::new("core.autocrlf".to_string(), "false".to_string());
    assert_eq!(resource.description(), "core.autocrlf = false");

    let absent = GitConfigResource::absent("core.autocrlf".to_string());
    assert_eq!(absent.description(), "core.autocrlf is unset");
}

// ------------------------------------------------------------------
// state_from_config
// ------------------------------------------------------------------

#[test]
fn configured_value_state_matches_current_config() {
    for (label, current, expected) in [
        ("matching value", Some("false"), ResourceState::Correct),
        ("missing key", None, ResourceState::Missing),
        (
            "different value",
            Some("true"),
            ResourceState::Incorrect {
                current: "true".to_string(),
            },
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut config = git2::Config::open(&dir.path().join("config")).unwrap();
        if let Some(value) = current {
            config.set_str("core.autocrlf", value).unwrap();
        }
        let resource = GitConfigResource::new("core.autocrlf".to_string(), "false".to_string());
        assert_eq!(
            resource.state_from_config(&config).unwrap(),
            expected,
            "{label}"
        );
    }
}

#[test]
fn absent_state_is_correct_only_when_key_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config");
    let mut config = git2::Config::open(&path).unwrap();
    let resource = GitConfigResource::absent("core.autocrlf".to_string());

    assert_eq!(
        resource.state_from_config(&config).unwrap(),
        ResourceState::Correct
    );

    config.set_str("core.autocrlf", "false").unwrap();
    assert_eq!(
        resource.state_from_config(&config).unwrap(),
        ResourceState::Incorrect {
            current: "false".to_string()
        }
    );
}

// ------------------------------------------------------------------
// apply_to_config
// ------------------------------------------------------------------

#[test]
fn repeated_git_config_values_converge_for_setting_and_removal() {
    for (key, section, variable, values, desired) in [
        (
            "core.autocrlf",
            "core",
            "autocrlf",
            ["false", "true"],
            Some("false"),
        ),
        (
            "core.autocrlf",
            "core",
            "autocrlf",
            ["true", "false"],
            Some("false"),
        ),
        (
            "credential.helper",
            "credential",
            "helper",
            ["old-helper", "manager"],
            Some("manager"),
        ),
        (
            "credential.helper",
            "credential",
            "helper",
            ["manager", "old-helper"],
            Some("manager"),
        ),
        ("core.autocrlf", "core", "autocrlf", ["false", "true"], None),
    ] {
        let dir = tempfile::tempdir_in(".").unwrap();
        let path = dir.path().join("config");
        std::fs::write(
            &path,
            format!("[{section}]\n\t{variable} = {}\n\t{variable} = {}\n[core]\n\teditor = fixture-editor\n", values[0], values[1]),
        )
        .unwrap();
        let resource = desired
            .map_or_else(
                || GitConfigResource::absent(key.into()),
                |value| GitConfigResource::new(key.into(), value.into()),
            )
            .using_config_path(path.clone());

        assert!(
            matches!(
                resource.current_state().unwrap(),
                ResourceState::Incorrect { .. }
            ),
            "duplicate settings are not yet in the desired state {desired:?}"
        );
        assert_eq!(
            resource.apply().unwrap(),
            ResourceChange::Applied,
            "must converge even when Git has repeated values for {desired:?}"
        );
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);

        let observed = git2::Config::open(&path).unwrap();
        assert_eq!(
            observed.get_string("core.editor").unwrap(),
            "fixture-editor"
        );
        if let Some(value) = desired {
            let mut entries = observed.multivar(key, None).unwrap();
            let mut actual_values = Vec::new();
            while let Some(entry) = entries.next() {
                actual_values.push(entry.unwrap().value().unwrap().to_string());
            }
            assert!(!actual_values.is_empty());
            assert!(
                actual_values.iter().all(|current| current == value),
                "{actual_values:?}"
            );
        } else {
            assert_eq!(
                observed.get_string(key).unwrap_err().code(),
                git2::ErrorCode::NotFound
            );
        }
    }
}

#[test]
fn implicit_and_non_utf8_values_are_not_silently_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config");
    let resource =
        GitConfigResource::with_config_path("core.autocrlf".into(), "false".into(), path.clone());
    std::fs::write(&path, "[core]\n\tautocrlf\n").unwrap();
    assert!(matches!(
        resource.current_state().unwrap(),
        ResourceState::Incorrect { .. }
    ));
    resource.apply().unwrap();
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);

    std::fs::write(&path, b"[core]\n\tautocrlf = \xff\n\tautocrlf = false\n").unwrap();
    let error = resource.current_state().unwrap_err();
    assert!(format!("{error:#}").contains("reading git config core.autocrlf"));
}

#[test]
fn malformed_config_is_rejected_without_rewriting_user_content() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let path = dir.path().join("config");
    let content = "[core\n  autocrlf = true\n";
    std::fs::write(&path, content).unwrap();
    let resource =
        GitConfigResource::with_config_path("core.autocrlf".into(), "false".into(), path.clone());

    for error in [
        resource.current_state().unwrap_err(),
        resource.apply().unwrap_err(),
    ] {
        assert!(format!("{error:#}").contains("core.autocrlf"), "{error:#}");
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
}

#[test]
fn repeated_implicit_values_fail_safely_without_entering_native_regex_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config");
    let content = "[core]\n\tautocrlf\n\tautocrlf = false\n";
    std::fs::write(&path, content).unwrap();
    for resource in [
        GitConfigResource::new("core.autocrlf".into(), "false".into()),
        GitConfigResource::absent("core.autocrlf".into()),
    ] {
        let resource = resource.using_config_path(path.clone());
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Incorrect { .. }
        ));
        assert!(
            format!("{:#}", resource.apply().unwrap_err()).contains("repeated implicit values")
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
    }
}

#[test]
fn apply_removes_obsolete_value() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config");
    let mut config = git2::Config::open(&path).unwrap();
    config.set_str("core.autocrlf", "false").unwrap();
    config.set_str("core.editor", "fixture-editor").unwrap();
    let resource = GitConfigResource::absent("core.autocrlf".to_string());

    assert_eq!(
        resource.apply_to_config(&mut config).unwrap(),
        ResourceChange::Applied
    );
    assert_eq!(
        config.get_string("core.autocrlf").unwrap_err().code(),
        git2::ErrorCode::NotFound,
    );
    assert_eq!(config.get_string("core.editor").unwrap(), "fixture-editor");
    assert_eq!(
        resource.state_from_config(&config).unwrap(),
        ResourceState::Correct
    );
}

#[test]
fn explicit_config_path_is_used_for_state_and_apply() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config");
    let mut config = git2::Config::open(&path).unwrap();
    config.set_str("core.editor", "fixture-editor").unwrap();
    let resource = GitConfigResource::with_config_path(
        "core.autocrlf".to_string(),
        "false".to_string(),
        path.clone(),
    );

    assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);

    let observed = git2::Config::open(&path).unwrap();
    assert_eq!(observed.get_string("core.autocrlf").unwrap(), "false");
    assert_eq!(
        observed.get_string("core.editor").unwrap(),
        "fixture-editor"
    );
}

#[test]
fn managed_includes_converge_without_oscillating() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config");
    let resources = vec![
        GitConfigResource::with_config_path(
            "include.path".into(),
            "~/.gitconfig.base".into(),
            path.clone(),
        ),
        GitConfigResource::with_config_path(
            "includeIf.gitdir:~/work/.path".into(),
            "~/.gitconfig.work".into(),
            path.clone(),
        ),
        GitConfigResource::with_config_path("core.autocrlf".into(), "false".into(), path.clone()),
    ];

    let plan = GitConfigPlan::inspect(&path, &resources).unwrap();
    assert_eq!(plan.changed().len(), 3);
    plan.apply(&path).unwrap();

    let second_plan = GitConfigPlan::inspect(&path, &resources).unwrap();
    assert!(
        second_plan.changed().is_empty(),
        "repeat inspection must converge without oscillating: {:?}",
        second_plan.changed()
    );
    for resource in &resources {
        assert_eq!(
            resource.current_state().unwrap(),
            ResourceState::Correct,
            "managed setting {} must report Correct after one apply",
            resource.key
        );
    }
}

#[test]
fn managed_include_preserves_precedence_before_existing_include_if() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config");
    std::fs::write(
        &path,
        "[include]\n\tpath = ~/.gitconfig.old\n[includeIf \"gitdir:~/work/\"]\n\tpath = ~/.gitconfig.work\n",
    )
    .unwrap();

    let resource = GitConfigResource::with_config_path(
        "include.path".into(),
        "~/.gitconfig.base".into(),
        path.clone(),
    );
    assert!(matches!(
        resource.current_state().unwrap(),
        ResourceState::Incorrect { .. }
    ));
    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);

    let contents = std::fs::read_to_string(&path).unwrap();
    let include_pos = contents
        .find("~/.gitconfig.base")
        .expect("updated include.path must be present");
    let include_if_pos = contents
        .find("[includeIf \"gitdir:~/work/\"]")
        .expect("existing includeIf section must be preserved");
    assert!(
        include_pos < include_if_pos,
        "managed include.path must stay before existing includeIf to preserve precedence:\n{contents}"
    );
}
