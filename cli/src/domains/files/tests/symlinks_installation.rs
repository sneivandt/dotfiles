use super::*;

#[test]
fn custom_config_home_rejects_install_and_preview_before_writing() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    let ctx = make_linux_context(fixture.path().into(), None)
        .with_home(home.clone())
        .with_env(
            crate::infra::env::MapEnv::new()
                .with("XDG_CONFIG_HOME", fixture.path().join("custom-config"))
                .into_handle(),
        );
    for target in [None, Some("./.config/git/config")] {
        let task = InstallSymlinks::new(handle(vec![symlink("config/git/config", target)]));
        for dry_run in [true, false] {
            let error = task.run(&ctx.with_dry_run(dry_run)).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("symlink.unsupported-config-home")
            );
            assert!(!home.exists());
        }
    }
}

#[cfg(unix)]
#[test]
fn file_capability_is_checked_at_execution_without_delegating_home_links() {
    use crate::engine::SkipKind;
    use crate::test_helpers::make_windows_context;

    for (name, capability, dry_run, file_created) in [
        ("Developer Mode enabled by predecessor", true, false, true),
        ("Developer Mode unavailable", false, false, false),
        ("preview without Developer Mode", false, true, false),
    ] {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = crate::infra::fs::canonicalize(fixture.path()).unwrap();
        let home = root.join("original-user");
        let admin_home = root.join("administrator");
        std::fs::create_dir_all(root.join("symlinks/directory")).unwrap();
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir(&admin_home).unwrap();
        std::fs::write(root.join("symlinks/file"), "file").unwrap();
        let config = handle(
            ["file", "directory"]
                .map(|source| Symlink {
                    source: source.into(),
                    target: None,
                    allow_external_target: false,
                    origin: None,
                })
                .to_vec(),
        );
        let task = InstallSymlinks::new(config);
        let ctx = make_windows_context(root, None)
            .with_home(home.clone())
            .with_dry_run(dry_run);
        assert!(!task.needs_elevation(&ctx), "{name}");
        assert!(
            !task.assess(&ctx, &|_| false).requires_elevation(),
            "{name}: phase assessment must not cache missing file capability"
        );
        let result =
            install_resources(&ctx, build_resources(&ctx, task.config.get()), capability).unwrap();
        assert_eq!(home.join(".file").exists(), file_created, "{name}");
        assert_eq!(home.join(".directory").exists(), !dry_run, "{name}");
        assert_eq!(
            std::fs::read_dir(admin_home).unwrap().count(),
            0,
            "{name}: a different administrator's home must remain untouched"
        );
        if !capability && !dry_run {
            assert!(
                matches!(
                    result,
                    TaskResult::Skipped {
                        kind: SkipKind::UnmetWork,
                        ..
                    }
                ),
                "{name}: pending file work must block dependents"
            );
        } else {
            assert!(matches!(result, TaskResult::Batch(_)), "{name}");
        }
        if file_created {
            let repeated =
                install_resources(&ctx, build_resources(&ctx, task.config.get()), false).unwrap();
            let TaskResult::Batch(stats) = repeated else {
                panic!("converged links must remain runnable")
            };
            assert_eq!(stats.already_ok_count(), 2);
            assert_eq!(stats.changed_count(), 0);
        }
    }
}

#[cfg(unix)]
#[test]
fn unavailable_file_links_do_not_hide_other_resource_failures() {
    use crate::test_helpers::make_windows_context;

    let fixture = tempfile::tempdir_in(".").unwrap();
    let root = crate::infra::fs::canonicalize(fixture.path()).unwrap();
    let home = root.join("home");
    std::fs::create_dir_all(root.join("symlinks")).unwrap();
    std::fs::create_dir(&home).unwrap();
    std::fs::write(root.join("symlinks/file"), "managed file").unwrap();
    let ctx = make_windows_context(root, None).with_home(home.clone());
    let config = [symlink("file", None), symlink("absent", None)];
    let result = install_resources(&ctx, build_resources(&ctx, &config), false).unwrap();
    let TaskResult::Batch(stats) = result else {
        panic!("an invalid source must remain a failed batch, not an elevation skip");
    };
    assert_eq!(stats.failed_count(), 1);
    assert!(!home.join(".file").exists());
}

#[test]
fn uninstall_should_run_false_when_no_symlinks_configured() {
    let ctx = make_linux_context(PathBuf::from("/tmp"), None);
    assert!(!UninstallSymlinks::new(handle(vec![])).should_run(&ctx));
}

#[test]
fn uninstall_should_run_true_when_symlinks_configured() {
    let ctx = make_linux_context(PathBuf::from("/tmp"), None);
    let task = UninstallSymlinks::new(handle(vec![symlink("bashrc", None)]));
    assert!(task.should_run(&ctx));
}

#[cfg(unix)]
#[test]
fn redirected_home_target_requires_opt_in_for_preview_apply_and_remove() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("repo");
    let home = fixture.path().join("home");
    let outside = fixture.path().join("outside");
    std::fs::create_dir_all(root.join("symlinks")).unwrap();
    std::fs::create_dir(&home).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let source = root.join("symlinks/bashrc");
    std::fs::write(&source, "managed content").unwrap();
    std::os::unix::fs::symlink(&outside, home.join(".config")).unwrap();
    let target = outside.join("nested/config");
    let ctx = make_linux_context(root.clone(), None).with_home(home.clone());
    let mut entry = symlink("bashrc", Some(".config/nested/config"));
    let blocked = build_resource(&entry, &root, &home, &ctx.executor_arc(), None);

    assert!(matches!(
        blocked.current_state().unwrap(),
        ResourceState::Invalid { reason }
            if reason.contains("allow_external_target") && reason.contains(&*target.to_string_lossy())
    ));
    assert!(crate::engine::Resource::apply(&blocked).is_err());
    assert!(crate::engine::RemovableResource::remove(&blocked).is_err());
    assert!(!outside.join("nested").exists());

    entry.allow_external_target = true;
    let allowed = build_resource(&entry, &root, &home, &ctx.executor_arc(), None);
    assert_eq!(allowed.current_state().unwrap(), ResourceState::Missing);
    assert!(crate::engine::Resource::description(&allowed).contains(&*target.to_string_lossy()));
    crate::engine::Resource::apply(&allowed).unwrap();
    assert_eq!(std::fs::read_link(&target).unwrap(), source);
    assert!(matches!(
        blocked.current_state().unwrap(),
        ResourceState::Invalid { .. }
    ));
    assert!(crate::engine::RemovableResource::remove(&blocked).is_err());
    assert!(target.is_symlink());
    crate::engine::RemovableResource::remove(&allowed).unwrap();
    assert!(!target.is_symlink());
    assert_eq!(std::fs::read_to_string(target).unwrap(), "managed content");
}

#[cfg(unix)]
#[test]
fn install_run_creates_symlink_for_configured_source() {
    let repo_dir = tempfile::tempdir().unwrap();
    let home_dir = tempfile::tempdir().unwrap();

    // Create symlinks/ dir and a source file inside it
    let symlinks_dir = repo_dir.path().join("symlinks");
    std::fs::create_dir(&symlinks_dir).unwrap();
    std::fs::write(symlinks_dir.join("bashrc"), "# bash config").unwrap();

    let ctx = make_linux_context(repo_dir.path().to_path_buf(), None)
        .with_home(home_dir.path().to_path_buf());

    let task = InstallSymlinks::new(handle(vec![symlink("bashrc", None)]));
    let result = task.run(&ctx).unwrap();
    assert!(matches!(
        result,
        TaskResult::Batch(stats) if stats.changed_count() == 1
    ));

    // Symlink must exist at $HOME/.bashrc and point to symlinks/bashrc
    let link = home_dir.path().join(".bashrc");
    assert!(
        link.symlink_metadata().is_ok(),
        "symlink should exist at ~/.bashrc"
    );
    let target = std::fs::read_link(&link).unwrap();
    assert_eq!(target, symlinks_dir.join("bashrc"));
}

#[cfg(unix)]
#[test]
fn install_run_skips_source_path_traversal() {
    let repo_dir = tempfile::tempdir().unwrap();
    let home_dir = tempfile::tempdir().unwrap();
    let outside = repo_dir.path().join("outside");
    std::fs::write(&outside, "outside").unwrap();

    let ctx = make_linux_context(repo_dir.path().to_path_buf(), None)
        .with_home(home_dir.path().to_path_buf());

    let task = InstallSymlinks::new(handle(vec![symlink("../outside", Some("escaped-source"))]));
    let result = task.run(&ctx).unwrap();
    assert!(matches!(
        result,
        TaskResult::Batch(stats) if stats.failed_count() == 1
    ));
    assert!(!home_dir.path().join("escaped-source").exists());
}

#[cfg(unix)]
#[test]
fn install_run_skips_source_symlink_outside_root() {
    let repo_dir = tempfile::tempdir().unwrap();
    let home_dir = tempfile::tempdir().unwrap();
    let outside_dir = tempfile::tempdir().unwrap();
    let symlinks_dir = repo_dir.path().join("symlinks");
    std::fs::create_dir(&symlinks_dir).unwrap();
    let outside = outside_dir.path().join("outside");
    std::fs::write(&outside, "outside").unwrap();
    std::os::unix::fs::symlink(&outside, symlinks_dir.join("escaped")).unwrap();

    let ctx = make_linux_context(repo_dir.path().to_path_buf(), None)
        .with_home(home_dir.path().to_path_buf());
    let task = InstallSymlinks::new(handle(vec![symlink("escaped", None)]));

    let result = task.run(&ctx).unwrap();

    assert!(matches!(
        result,
        TaskResult::Batch(stats) if stats.failed_count() == 1
    ));
    assert!(!home_dir.path().join(".escaped").exists());
}

#[cfg(unix)]
#[test]
fn historical_editor_migration_rechecks_a_retargeted_source_and_can_retry() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().canonicalize().unwrap();
    let home = root.join("home");
    let sources = root.join("symlinks");
    let editor = sources.join("vim");
    let managed = sources.join("managed-editor");
    let outside = root.join("user-editor");
    let historical = sources.join("config/nvim");
    let installed = home.join(".config/nvim");
    std::fs::create_dir_all(historical.parent().unwrap()).unwrap();
    std::fs::create_dir_all(installed.parent().unwrap()).unwrap();
    for (dir, text) in [(&managed, "managed"), (&outside, "custom")] {
        std::fs::create_dir(dir).unwrap();
        std::fs::write(dir.join("init.vim"), text).unwrap();
    }
    std::os::unix::fs::symlink(&managed, &editor).unwrap();
    std::os::unix::fs::symlink(&historical, &installed).unwrap();
    let ctx = make_linux_context(root, None).with_home(home);
    let resource = build_resources(&ctx, &[symlink("vim", Some(".config/nvim"))])
        .pop()
        .unwrap();
    assert!(matches!(
        resource.current_state().unwrap(),
        ResourceState::Incorrect { .. }
    ));
    assert_eq!(resource.removal_state().unwrap(), ResourceState::Correct);

    // A checkout edit between planning and mutation is not authorization
    // to link to or import the user's separate editor configuration.
    std::fs::remove_file(&editor).unwrap();
    std::os::unix::fs::symlink(&outside, &editor).unwrap();
    for result in [resource.apply(), resource.remove()] {
        assert!(format!("{:#}", result.unwrap_err()).contains("outside its symlinks directory"));
        assert_eq!(std::fs::read_link(&installed).unwrap(), historical);
    }
    assert_eq!(
        std::fs::read_to_string(outside.join("init.vim")).unwrap(),
        "custom"
    );
    assert_eq!(
        std::fs::read_dir(installed.parent().unwrap())
            .unwrap()
            .count(),
        1
    );

    std::fs::remove_file(&editor).unwrap();
    std::os::unix::fs::symlink(&managed, &editor).unwrap();
    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    assert_eq!(std::fs::read_link(&installed).unwrap(), editor);
    assert_eq!(resource.apply().unwrap(), ResourceChange::AlreadyCorrect);
    assert_eq!(resource.remove().unwrap(), ResourceChange::Applied);
    assert!(!installed.is_symlink());
    assert_eq!(
        std::fs::read_to_string(installed.join("init.vim")).unwrap(),
        "managed"
    );
}

#[cfg(unix)]
#[test]
fn install_run_skips_target_path_traversal() {
    let repo_dir = tempfile::tempdir().unwrap();
    let home_dir = tempfile::tempdir().unwrap();
    let symlinks_dir = repo_dir.path().join("symlinks");
    std::fs::create_dir(&symlinks_dir).unwrap();
    std::fs::write(symlinks_dir.join("bashrc"), "# bash config").unwrap();
    let outside_target = home_dir.path().join("..").join("outside-target");
    let ctx = make_linux_context(repo_dir.path().to_path_buf(), None)
        .with_home(home_dir.path().to_path_buf());

    let task = InstallSymlinks::new(handle(vec![symlink("bashrc", Some("../outside-target"))]));
    let result = task.run(&ctx).unwrap();
    assert!(matches!(
        result,
        TaskResult::Batch(stats) if stats.failed_count() == 1
    ));
    assert!(!outside_target.exists());
}
