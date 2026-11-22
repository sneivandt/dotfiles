use super::*;

#[test]
fn command_restarts_require_the_parent_home_lock_and_live_repository_lease() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("repository");
    git2::Repository::init(&root).unwrap();
    let home = fixture.path().join("home");
    let env = MapEnv::new();
    let platform = Platform::detect();
    let locks = CommandLocks::acquire(&root, &home, &env, platform, "update").unwrap();
    let (continuations, mode) = locks.prepare_restart().unwrap();
    let lease = RestartLease::new(&root, &env, platform, mode).unwrap();
    let marker = lease.marker().unwrap().to_owned();
    let child = CommandLocks::restart(&root, &home, &env, platform, &marker, "update").unwrap();
    drop(child);
    drop((locks, continuations));
    let _repository = RunLock::acquire(&root, &env, platform, "fixture").unwrap();
    let error = CommandLocks::restart(&root, &home, &env, platform, &marker, "update").unwrap_err();
    assert!(
        error.to_string().contains("no active home lock"),
        "{error:#}"
    );
    let _home =
        RunLock::acquire_path(&home_lock_path(&home, platform).unwrap(), "fixture").unwrap();
    drop(lease);
    assert!(CommandLocks::restart(&root, &home, &env, platform, &marker, "update").is_err());
}

#[test]
fn legacy_repository_leases_acquire_home_lock_before_applying_configuration() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("repository");
    git2::Repository::init(&root).unwrap();
    let home = fixture.path().join("home");
    let env = MapEnv::new();
    let platform = Platform::detect();
    let _repository = RunLock::acquire(&root, &env, platform, "old binary").unwrap();
    let (marker, lease) = crate::infra::fs::TempGuard::create_unique_file_with_mode(
        &root.join(".git"),
        RESTART_PREFIX,
        "lock",
        0o600,
    )
    .unwrap();
    lease.try_lock().unwrap();
    let marker_name = marker.path().file_name().unwrap();
    let home_lock =
        CommandLocks::restart(&root, &home, &env, platform, marker_name, "update").unwrap();
    assert!(
        RunLock::acquire_path(&home_lock_path(&home, platform).unwrap(), "other clone").is_err()
    );
    let (continuations, mode) = home_lock.prepare_restart().unwrap();
    let modern = RestartLease::new(&root, &env, platform, mode).unwrap();
    drop(
        CommandLocks::restart(
            &root,
            &home,
            &env,
            platform,
            modern.marker().unwrap(),
            "update",
        )
        .unwrap(),
    );
    drop((home_lock, continuations));
    let unrelated =
        RunLock::acquire_path(&home_lock_path(&home, platform).unwrap(), "other clone").unwrap();
    assert!(CommandLocks::restart(&root, &home, &env, platform, marker_name, "update").is_err());
    drop(unrelated);
    CommandLocks::restart(&root, &home, &env, platform, marker_name, "retry").unwrap();
}

#[test]
fn shared_restart_rejects_a_legacy_lock_winner_before_work_can_resume() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("repository");
    let home = fixture.path().join("home");
    git2::Repository::init(&root).unwrap();
    let env = MapEnv::new();
    let platform = Platform::detect();
    let parent = CommandLocks::acquire(&root, &home, &env, platform, "update").unwrap();
    let (continuation, mode) = parent.prepare_restart().unwrap();
    let lease = RestartLease::new(&root, &env, platform, mode).unwrap();
    let marker = lease.marker().unwrap();
    drop(parent);

    // Simulate an older invocation taking the base lock while the
    // continuation and lease are still present.
    let repository = repository_state_path(&root, &env, platform, LOCK_NAME).unwrap();
    let legacy = open_lock(&repository, false).unwrap();
    legacy.try_lock().unwrap();
    let error = CommandLocks::restart(&root, &home, &env, platform, marker, "update").unwrap_err();
    assert!(
        error.to_string().contains("another dotfiles run"),
        "{error:#}"
    );
    drop(legacy);
    drop(continuation);
}

#[test]
fn nested_continuation_keeps_the_home_and_checkouts_locked_after_parents_exit() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("repository");
    let overlay = fixture.path().join("overlay");
    let another_root = fixture.path().join("another");
    let home = fixture.path().join("home");
    let another_home = fixture.path().join("another-home");
    for repository in [&root, &overlay, &another_root] {
        git2::Repository::init(repository).unwrap();
    }
    let env = MapEnv::new();
    let platform = Platform::detect();
    let parent = CommandLocks::acquire(&root, &home, &env, platform, "update").unwrap();
    let (parent_continuation, parent_mode) = parent.prepare_restart().unwrap();
    let parent_lease = RestartLease::new(&root, &env, platform, parent_mode).unwrap();

    // Binary self-update takes place before selecting the overlay lock.
    let mut middle = CommandLocks::restart(
        &root,
        &home,
        &env,
        platform,
        parent_lease.marker().unwrap(),
        "update",
    )
    .unwrap();
    middle
        .protect_overlay(&overlay, &env, platform, "update", None)
        .unwrap();
    drop((parent, parent_lease, parent_continuation));
    let (middle_continuation, middle_mode) = middle.prepare_restart().unwrap();
    let middle_lease = RestartLease::new(&root, &env, platform, middle_mode).unwrap();

    // Repository synchronization restarts again with the selected overlay.
    let marker = middle_lease.marker().unwrap();
    let mut child = CommandLocks::restart(&root, &home, &env, platform, marker, "update").unwrap();
    child
        .protect_overlay(&overlay, &env, platform, "update", Some((&root, marker)))
        .unwrap();
    drop((middle, middle_lease, middle_continuation));

    for (repo, destination) in [
        (&root, &another_home),
        (&another_root, &home),
        (&overlay, &another_home),
    ] {
        let error = CommandLocks::acquire(repo, destination, &env, platform, "remove").unwrap_err();
        assert!(
            error.to_string().contains("another dotfiles run"),
            "{error:#}"
        );
        assert!(error.to_string().contains(".lock"), "{error:#}");
    }
    drop(child);
    for (repo, destination) in [
        (&root, &another_home),
        (&another_root, &home),
        (&overlay, &another_home),
    ] {
        CommandLocks::acquire(repo, destination, &env, platform, "retry").unwrap();
    }
}

#[test]
fn restart_requires_both_a_live_lease_and_its_repository_lock() {
    let root = tempfile::tempdir().unwrap();
    git2::Repository::init(root.path()).unwrap();
    let env = MapEnv::new();
    let platform = Platform::detect();
    let lock = RunLock::acquire(root.path(), &env, platform, "update").unwrap();
    let lease = RestartLease::new(root.path(), &env, platform, RestartLockMode::Legacy).unwrap();
    let marker = lease.marker().unwrap().to_owned();
    verify_restart(root.path(), &env, platform, &marker).unwrap();
    for invalid in [
        "",
        "../.dotfiles-restart-lock",
        ".dotfiles-restart-fake-lock",
    ] {
        assert!(verify_restart(root.path(), &env, platform, invalid.as_ref()).is_err());
    }
    drop(lock);
    assert!(
        verify_restart(root.path(), &env, platform, &marker)
            .unwrap_err()
            .to_string()
            .contains("no active repository lock")
    );
    let path = repository_state_path(root.path(), &env, platform, LOCK_NAME).unwrap();
    let _continuation = hold_continuation(&path).unwrap();
    verify_restart(root.path(), &env, platform, &marker).unwrap();
    drop(lease);
    assert!(verify_restart(root.path(), &env, platform, &marker).is_err());
}
