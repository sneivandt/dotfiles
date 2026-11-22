use super::*;

#[cfg(unix)]
#[test]
fn lock_owner_validation_rejects_another_users_file_without_writing() {
    let fixture = tempfile::tempdir().unwrap();
    let target = fixture.path().join("lock");
    std::fs::write(&target, "foreign owner data").unwrap();
    let metadata = target.metadata().unwrap();
    let uid = nix::unistd::Uid::effective().as_raw();
    check_owner(&metadata, uid).unwrap();
    assert!(check_owner(&metadata, uid.wrapping_add(1)).is_err());
    assert_eq!(
        std::fs::read_to_string(target).unwrap(),
        "foreign owner data"
    );
}

#[test]
fn lock_rejects_hard_links_and_retries_after_the_alias_is_removed() {
    let fixture = tempfile::tempdir().unwrap();
    let target = fixture.path().join("unrelated");
    let lock = fixture.path().join("run.lock");
    std::fs::write(&target, "unrelated user data").unwrap();
    std::fs::hard_link(&target, &lock).unwrap();
    let error = RunLock::acquire_path(&lock, "check").unwrap_err();
    assert!(
        format!("{error:#}").contains("multiply linked"),
        "{error:#}"
    );
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "unrelated user data"
    );
    std::fs::remove_file(&lock).unwrap();
    let holder = RunLock::acquire_path(&lock, "check").unwrap();
    assert!(RunLock::acquire_path(&lock, "update").is_err());
    drop(holder);
    drop(RunLock::acquire_path(&lock, "update").unwrap());
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "unrelated user data"
    );
}

#[test]
fn independent_checkouts_share_home_locks_even_with_different_state_overrides() {
    let fixture = tempfile::tempdir().unwrap();
    let first = fixture.path().join("first");
    let second = fixture.path().join("second");
    git2::Repository::init(&first).unwrap();
    git2::Repository::init(&second).unwrap();
    for os in [Os::Linux, Os::Windows] {
        let platform = Platform::new(os, false);
        let home = fixture.path().join(os.to_string());
        let env = MapEnv::new();
        let holder = CommandLocks::acquire(&first, &home, &env, platform, "update").unwrap();
        let other_env = MapEnv::new()
            .with("XDG_STATE_HOME", fixture.path().join("different-state"))
            .with("LOCALAPPDATA", fixture.path().join("different-local"));
        let home_conflict =
            CommandLocks::acquire(&second, &home, &other_env, platform, "remove").unwrap_err();
        assert!(
            home_conflict.to_string().contains("home-run.lock"),
            "{home_conflict:#}"
        );
        assert!(
            home_conflict.to_string().contains("another dotfiles run"),
            "{home_conflict:#}"
        );

        // A failed home-lock acquisition must release its repository lock.
        let other_home = fixture.path().join(format!("other-{os}"));
        let independent =
            CommandLocks::acquire(&second, &other_home, &env, platform, "check").unwrap();
        let repository_conflict =
            CommandLocks::acquire(&first, &other_home, &env, platform, "update").unwrap_err();
        assert!(
            repository_conflict
                .to_string()
                .contains("dotfiles-run.lock"),
            "{repository_conflict:#}"
        );
        drop((holder, independent));
        CommandLocks::acquire(&second, &home, &env, platform, "retry").unwrap();
    }
}

#[test]
fn exported_overlay_needs_no_git_lock_storage() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("main");
    let overlay = fixture.path().join("export");
    let home = fixture.path().join("home");
    git2::Repository::init(&root).unwrap();
    std::fs::create_dir(&overlay).unwrap();
    let env = MapEnv::new().with("HOME", home.as_os_str());
    let windows = Platform::new(Os::Windows, false);
    let mut locks = CommandLocks::acquire(&root, &home, &env, windows, "check").unwrap();
    locks
        .protect_overlay(&overlay, &env, windows, "check", None)
        .unwrap();
    assert!(std::fs::read_dir(&overlay).unwrap().next().is_none());
}

#[cfg(unix)]
#[test]
fn read_only_overlay_coordinates_with_a_writable_checkout_alias() {
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    if nix::unistd::Uid::effective().is_root() {
        return;
    }
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let overlay = fixture.path().join("overlay");
    let alias = fixture.path().join("alias");
    let home = fixture.path().join("home");
    let other_home = fixture.path().join("other-home");
    for repository in [&root, &overlay] {
        git2::Repository::init(repository).unwrap();
    }
    symlink(&overlay, &alias).unwrap();

    let platform = Platform::detect();
    let env = MapEnv::new();
    let mut active = CommandLocks::acquire(&root, &home, &env, platform, "preview").unwrap();
    let git_dir = overlay.join(".git");
    std::fs::set_permissions(&git_dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    let protection = active.protect_overlay(&overlay, &env, platform, "preview", None);
    std::fs::set_permissions(&git_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    protection.unwrap();
    assert!(!git_dir.join(LOCK_NAME).exists());

    let conflict =
        CommandLocks::acquire(&alias, &other_home, &env, platform, "update").unwrap_err();
    assert!(
        conflict.to_string().contains("another dotfiles run"),
        "{conflict:#}"
    );
    drop(active);
    drop(CommandLocks::acquire(&alias, &other_home, &env, platform, "retry").unwrap());

    // A prior writable run leaves an original lock file. A later restart
    // must preserve its protection without creating another Git file.
    assert!(git_dir.join(LOCK_NAME).is_file());
    let mut parent = CommandLocks::acquire(&root, &home, &env, platform, "update").unwrap();
    std::fs::set_permissions(&git_dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    let mut restart = || -> Result<()> {
        parent.protect_overlay(&overlay, &env, platform, "update", None)?;
        let (_guards, mode) = parent.prepare_restart()?;
        let lease = RestartLease::new(&root, &env, platform, mode)?;
        let marker = lease.marker()?;
        let mut child = CommandLocks::restart(&root, &home, &env, platform, marker, "update")?;
        child.protect_overlay(&overlay, &env, platform, "update", Some((&root, marker)))
    };
    let result = restart();
    std::fs::set_permissions(&git_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    result.unwrap();
    assert!(!continuation_path(&git_dir.join(LOCK_NAME)).exists());
}

#[test]
fn first_login_waits_for_the_repository_or_home_lock() {
    use std::sync::mpsc::{RecvTimeoutError, channel};
    use std::time::Duration;

    let fixture = tempfile::tempdir().unwrap();
    let first = fixture.path().join("first");
    let second = fixture.path().join("second");
    let home = fixture.path().join("home");
    git2::Repository::init(&first).unwrap();
    git2::Repository::init(&second).unwrap();
    let platform = Platform::new(Os::Linux, false);
    let env = MapEnv::new();
    let service_env = MapEnv::new().with("DOTFILES_FIRST_LOGIN_WAIT_FOR_LOCK", "1");

    for root in [&first, &second] {
        std::thread::scope(|scope| {
            let holder = CommandLocks::acquire(&first, &home, &env, platform, "update").unwrap();
            let (started, start) = channel();
            let (finished, finish) = channel();
            let home = &home;
            let service_env = &service_env;
            scope.spawn(move || {
                started.send(()).unwrap();
                let result =
                    CommandLocks::acquire(root, home, service_env, platform, "first login");
                finished.send(result).unwrap();
            });
            start.recv().unwrap();
            let attempt = finish.recv_timeout(Duration::from_millis(100));
            drop(holder);
            assert!(matches!(attempt, Err(RecvTimeoutError::Timeout)));
            drop(
                finish
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
                    .unwrap(),
            );
        });
    }
}

#[cfg(unix)]
#[test]
fn home_aliases_contend_for_the_same_destination_lock() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let alias = fixture.path().join("alias");
    std::os::unix::fs::symlink(&home, &alias).unwrap();
    let platform = Platform::detect();
    let path = home_lock_path(&home, platform).unwrap();
    assert_eq!(home_lock_path(&alias, platform).unwrap(), path);
    let _lock = RunLock::acquire_path(&path, "update").unwrap();
    let error =
        RunLock::acquire_path(&home_lock_path(&alias, platform).unwrap(), "remove").unwrap_err();
    assert!(error.to_string().contains("another dotfiles run"));
}

#[test]
fn home_lock_resolution_rejects_relative_paths_and_preserves_io_errors() {
    let platform = Platform::detect();
    assert!(home_lock_path(Path::new("relative-home"), platform).is_err());
    let fixture = tempfile::tempdir().unwrap();
    let invalid = fixture.path().join("invalid\0home");
    assert!(home_lock_path(&invalid, platform).is_err());
    assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn lock_rejects_a_symlink_without_writing_its_target() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("local-file");
    let lock = dir.path().join("lock");
    std::fs::write(&target, "local data").unwrap();
    std::os::unix::fs::symlink(&target, &lock).unwrap();

    assert!(RunLock::acquire_path(&lock, "fixture").is_err());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "local data");
}

#[test]
fn operational_lock_errors_are_not_reported_as_contention() {
    let path = Path::new("fixture.lock");
    for kind in [
        std::io::ErrorKind::Unsupported,
        std::io::ErrorKind::PermissionDenied,
        std::io::ErrorKind::Other,
    ] {
        let error = lock_error(
            path,
            TryLockError::Error(std::io::Error::new(kind, "fixture failure")),
        );
        assert_eq!(error.to_string(), "locking run lock fixture.lock");
        assert_eq!(error.downcast_ref::<std::io::Error>().unwrap().kind(), kind);
        assert!(!format!("{error:#}").contains("another dotfiles run"));
    }
}

#[test]
fn prevents_overlapping_runs_and_releases_on_drop() {
    let root = tempfile::tempdir().expect("tempdir");
    git2::Repository::init(root.path()).expect("init repository");
    let env = MapEnv::new();

    let platform = Platform::new(Os::Windows, false);
    let mut first = RunLock::acquire(root.path(), &env, platform, "install").expect("first lock");
    let path = repository_state_path(root.path(), &env, platform, "dotfiles-run.lock").unwrap();
    // Windows excludes other handles from reading the locked region.
    first.file.rewind().unwrap();
    let mut owner_before = String::new();
    first.file.read_to_string(&mut owner_before).unwrap();
    let owner: serde_json::Value = serde_json::from_str(&owner_before).unwrap();
    assert_eq!(owner["pid"], std::process::id());
    assert_eq!(owner["command"], "install");
    assert!(owner["started_unix_seconds"].as_u64().unwrap() > 0);
    let error = RunLock::acquire(root.path(), &env, platform, "update").expect_err("second lock");
    assert!(
        error.to_string().contains("another dotfiles run"),
        "unexpected error: {error:#}"
    );
    assert!(error.to_string().contains(&path.display().to_string()));
    assert!(
        error
            .to_string()
            .contains("wait for it to finish and retry")
    );
    #[cfg(not(windows))]
    assert!(
        error.to_string().contains(&owner_before),
        "contention should identify the current owner"
    );
    drop(first);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        owner_before,
        "a failed contender must not truncate the holder's metadata"
    );

    let next = RunLock::acquire(root.path(), &env, platform, "check").expect("lock after release");
    drop(next);
    let replacement_owner: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(
        replacement_owner["command"], "check",
        "replace the longer stale owner record"
    );
}

#[test]
fn linked_worktrees_share_a_lock_but_independent_repositories_do_not() {
    let root = tempfile::tempdir().unwrap();
    let repository = git2::Repository::init(root.path().join("main")).unwrap();
    let tree_id = repository.index().unwrap().write_tree().unwrap();
    let tree = repository.find_tree(tree_id).unwrap();
    let signature = git2::Signature::now("Test", "test@test.local").unwrap();
    repository
        .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
        .unwrap();
    let linked = root.path().join("linked");
    repository.worktree("linked", &linked, None).unwrap();
    let independent = root.path().join("independent");
    git2::Repository::init(&independent).unwrap();
    let env = MapEnv::new();
    let platform = Platform::new(Os::Linux, false);
    let _main = RunLock::acquire(repository.workdir().unwrap(), &env, platform, "install").unwrap();

    let error = RunLock::acquire(&linked, &env, platform, "update").unwrap_err();
    assert!(error.to_string().contains("another dotfiles run"));
    let _independent = RunLock::acquire(&independent, &env, platform, "check").unwrap();
}

#[test]
fn exported_tree_never_replaces_ancestor_lock_or_falls_back_on_bad_metadata() {
    let fixture = tempfile::tempdir().unwrap();
    let outer = git2::Repository::init(fixture.path()).unwrap();
    let outer_lock = outer.commondir().join(LOCK_NAME);
    std::fs::write(&outer_lock, "outer state").unwrap();
    let export = fixture.path().join("export");
    std::fs::create_dir(&export).unwrap();
    let env = MapEnv::new().with("HOME", fixture.path().join("fixture-home"));
    let platform = Platform::new(Os::Linux, false);
    let marker = export.join(".git");
    std::fs::write(&marker, "gitdir: unavailable\n").unwrap();
    assert!(RunLock::acquire(&export, &env, platform, "check").is_err());
    assert!(!fixture.path().join("fixture-home").exists());
    std::fs::remove_file(marker).unwrap();
    let holder = RunLock::acquire(&export, &env, platform, "check").unwrap();
    assert!(RunLock::acquire(&export, &env, platform, "retry").is_err());
    drop(holder);
    drop(RunLock::acquire(&export, &env, platform, "retry").unwrap());
    assert_eq!(std::fs::read(outer_lock).unwrap(), b"outer state");
}

#[test]
fn non_repository_state_paths_use_platform_precedence_and_repository_identity() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first");
    let second = root.path().join("second");
    for path in [&first, &second] {
        std::fs::create_dir(path).unwrap();
    }
    let local = root.path().join("local");
    let state = root.path().join("state");
    let home = root.path().join("home");
    for (name, os, env, base) in [
        (
            "windows explicit",
            Os::Windows,
            MapEnv::new()
                .with("LOCALAPPDATA", &local)
                .with("USERPROFILE", &home)
                .with("XDG_STATE_HOME", &state),
            local,
        ),
        (
            "windows fallback",
            Os::Windows,
            MapEnv::new().with("USERPROFILE", &home),
            home.join("AppData").join("Local"),
        ),
        (
            "linux explicit",
            Os::Linux,
            MapEnv::new()
                .with("XDG_STATE_HOME", &state)
                .with("HOME", &home),
            state,
        ),
        (
            "linux fallback",
            Os::Linux,
            MapEnv::new().with("HOME", &home),
            home.join(".local").join("state"),
        ),
    ] {
        let platform = Platform::new(os, false);
        let first_path = repository_state_path(&first, &env, platform, "lock").unwrap();
        let second_path = repository_state_path(&second, &env, platform, "lock").unwrap();
        assert!(
            first_path.starts_with(base.join("dotfiles").join("repositories")),
            "{name}"
        );
        assert_eq!(first_path.file_name().unwrap(), "lock", "{name}");
        assert_ne!(
            first_path, second_path,
            "{name}: repositories must not collide"
        );
        assert_eq!(
            repository_state_path(&first, &env, platform, "state")
                .unwrap()
                .parent(),
            first_path.parent(),
            "{name}: all state files share a repository namespace"
        );
        assert!(
            !base.exists(),
            "{name}: path resolution must not create state"
        );
    }
    for os in [Os::Linux, Os::Windows] {
        let error = repository_state_path(&first, &MapEnv::new(), Platform::new(os, false), "lock")
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "cannot determine platform state directory for the run lock",
            "{os}"
        );
    }
}

#[test]
fn lock_open_errors_preserve_context_and_existing_contents() {
    let root = tempfile::tempdir().unwrap();
    let repository = git2::Repository::init(root.path()).unwrap();
    let path = repository.commondir().join("dotfiles-run.lock");
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("keep"), "preserve").unwrap();

    let error = RunLock::acquire(
        root.path(),
        &MapEnv::new(),
        Platform::new(Os::Windows, false),
        "install",
    )
    .unwrap_err();

    assert_eq!(
        error.to_string(),
        format!("opening run lock {}", path.display())
    );
    assert!(error.downcast_ref::<std::io::Error>().is_some());
    assert_eq!(
        std::fs::read_to_string(path.join("keep")).unwrap(),
        "preserve"
    );
}

#[test]
fn non_repository_locks_ignore_blank_and_relative_state_directories() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let root = std::path::absolute(fixture.path()).unwrap();
    let home = root.join("home");

    for (os, state_key, home_key) in [
        (Os::Linux, "XDG_STATE_HOME", "HOME"),
        (Os::Windows, "LOCALAPPDATA", "USERPROFILE"),
    ] {
        let platform = Platform::new(os, false);
        let fallback = MapEnv::new().with(home_key, &home);
        let expected = repository_state_path(&root, &fallback, platform, "lock").unwrap();
        for invalid in ["", "relative-state"] {
            let env = fallback.clone().with(state_key, invalid);
            let actual = repository_state_path(&root, &env, platform, "lock").unwrap();
            assert_eq!(actual, expected, "{os}: {state_key}={invalid:?}");
            assert!(actual.is_absolute(), "{os}: lock must not depend on cwd");
        }
    }
    assert!(!home.exists(), "path resolution must remain read-only");
}

#[test]
fn non_repository_locks_reject_missing_absolute_home_and_state_directories() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let root = std::path::absolute(fixture.path()).unwrap();

    for (os, state_key, home_key) in [
        (Os::Linux, "XDG_STATE_HOME", "HOME"),
        (Os::Windows, "LOCALAPPDATA", "USERPROFILE"),
    ] {
        for invalid in ["", "relative-home"] {
            let env = MapEnv::new().with(state_key, "").with(home_key, invalid);
            let error =
                repository_state_path(&root, &env, Platform::new(os, false), "lock").unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("cannot determine platform state directory"),
                "{os}: {home_key}={invalid:?}: {error}"
            );
        }
    }
}

#[test]
fn blank_state_override_and_home_fallback_contend_for_the_same_run_lock() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let root = std::path::absolute(fixture.path()).unwrap();
    for (os, state_key, home_key) in [
        (Os::Linux, "XDG_STATE_HOME", "HOME"),
        (Os::Windows, "LOCALAPPDATA", "USERPROFILE"),
    ] {
        let fallback = MapEnv::new().with(home_key, root.join(os.to_string()));
        let platform = Platform::new(os, false);
        let _holder = RunLock::acquire(&root, &fallback, platform, "install").unwrap();
        let expected =
            repository_state_path(&root, &fallback, platform, "dotfiles-run.lock").unwrap();
        let contender = fallback.with(state_key, "");
        assert_eq!(
            repository_state_path(&root, &contender, platform, "dotfiles-run.lock").unwrap(),
            expected,
            "{os}: resolve inside the fixture before attempting the contender"
        );
        let error = RunLock::acquire(&root, &contender, platform, "update").unwrap_err();
        assert!(
            error.to_string().contains("another dotfiles run"),
            "{os}: a blank override must not bypass an existing lock: {error}"
        );
    }
}
