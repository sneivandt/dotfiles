use super::*;

#[cfg(unix)]
#[test]
fn uninstall_run_materializes_symlink_content() {
    let repo_dir = tempfile::tempdir().unwrap();
    let home_dir = tempfile::tempdir().unwrap();

    // Create source file
    let symlinks_dir = repo_dir.path().join("symlinks");
    std::fs::create_dir(&symlinks_dir).unwrap();
    std::fs::write(symlinks_dir.join("bashrc"), "# bash config").unwrap();

    // Pre-create the symlink at $HOME/.bashrc
    let link = home_dir.path().join(".bashrc");
    std::os::unix::fs::symlink(symlinks_dir.join("bashrc"), &link).unwrap();

    let ctx = make_linux_context(repo_dir.path().to_path_buf(), None)
        .with_home(home_dir.path().to_path_buf());

    let task = UninstallSymlinks::new(handle(vec![symlink("bashrc", None)]));
    let result = task.run(&ctx).unwrap();
    assert!(matches!(
        result,
        TaskResult::Batch(stats) if stats.changed_count() == 1
    ));

    // Must be a regular file (materialized), not a symlink
    let meta = std::fs::symlink_metadata(&link).unwrap();
    assert!(
        !meta.is_symlink(),
        "target should be materialized to a regular file"
    );
    assert_eq!(std::fs::read_to_string(&link).unwrap(), "# bash config");
}

#[test]
fn uninstall_run_materializes_content_when_target_is_missing() {
    let repo_dir = tempfile::tempdir().unwrap();
    let home_dir = tempfile::tempdir().unwrap();
    let symlinks_dir = repo_dir.path().join("symlinks/config/git");
    std::fs::create_dir_all(&symlinks_dir).unwrap();
    std::fs::write(symlinks_dir.join("config"), "# git config").unwrap();

    let target = home_dir.path().join(".config/git/config");
    let ctx = make_linux_context(repo_dir.path().to_path_buf(), None)
        .with_home(home_dir.path().to_path_buf());

    let config = handle(vec![symlink("config/git/config", None)]);
    let default = UninstallSymlinks::new(config.clone()).run(&ctx).unwrap();
    assert!(matches!(default, TaskResult::Batch(stats) if stats.changed_count() == 0));
    assert!(!target.exists());
    let result = UninstallSymlinks::with_restore_missing(config, true)
        .run(&ctx)
        .unwrap();

    assert!(matches!(
        result,
        TaskResult::Batch(stats) if stats.changed_count() == 1
    ));
    assert_eq!(std::fs::read_to_string(target).unwrap(), "# git config");
}

#[cfg(unix)]
#[test]
fn uninstall_run_parallel_materializes_similar_file_names() {
    let repo_dir = tempfile::tempdir().unwrap();
    let home_dir = tempfile::tempdir().unwrap();

    let symlinks_dir = repo_dir.path().join("symlinks/config/systemd/user");
    std::fs::create_dir_all(&symlinks_dir).unwrap();
    std::fs::write(
        symlinks_dir.join("clean-home-tmp.service"),
        "[Service]\nExecStart=/bin/true\n",
    )
    .unwrap();
    std::fs::write(
        symlinks_dir.join("clean-home-tmp.timer"),
        "[Timer]\nOnCalendar=daily\n",
    )
    .unwrap();

    let target_dir = home_dir.path().join(".config/systemd/user");
    std::fs::create_dir_all(&target_dir).unwrap();
    let service_link = target_dir.join("clean-home-tmp.service");
    let timer_link = target_dir.join("clean-home-tmp.timer");
    std::os::unix::fs::symlink(symlinks_dir.join("clean-home-tmp.service"), &service_link).unwrap();
    std::os::unix::fs::symlink(symlinks_dir.join("clean-home-tmp.timer"), &timer_link).unwrap();

    let ctx = make_linux_context(repo_dir.path().to_path_buf(), None)
        .with_home(home_dir.path().to_path_buf())
        .with_parallel(true);

    let task = UninstallSymlinks::new(handle(vec![
        symlink("config/systemd/user/clean-home-tmp.service", None),
        symlink("config/systemd/user/clean-home-tmp.timer", None),
    ]));
    let result = task.run(&ctx).unwrap();
    assert!(matches!(
        result,
        TaskResult::Batch(stats) if stats.changed_count() == 2
    ));

    let service_meta = std::fs::symlink_metadata(&service_link).unwrap();
    let timer_meta = std::fs::symlink_metadata(&timer_link).unwrap();
    assert!(!service_meta.is_symlink());
    assert!(!timer_meta.is_symlink());
    assert_eq!(
        std::fs::read_to_string(&service_link).unwrap(),
        "[Service]\nExecStart=/bin/true\n"
    );
    assert_eq!(
        std::fs::read_to_string(&timer_link).unwrap(),
        "[Timer]\nOnCalendar=daily\n"
    );
}
