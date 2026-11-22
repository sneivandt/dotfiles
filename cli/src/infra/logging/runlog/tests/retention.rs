use super::*;

#[cfg(unix)]
#[test]
fn new_logs_and_lock_sidecars_are_private_in_existing_directories() {
    use std::os::unix::fs::PermissionsExt as _;

    let fixture = tempfile::tempdir().unwrap();
    fs::set_permissions(fixture.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let (path, _file, _lease) =
        create_run_log_file("check", fixture.path(), "20200101T000000Z").unwrap();
    for private_path in [
        path.clone(),
        lease_path(&path),
        fixture.path().join(RETENTION_LOCK),
    ] {
        assert_eq!(
            fs::metadata(private_path).unwrap().permissions().mode() & 0o777,
            0o600,
            "new diagnostic files must be owner-only even in an existing directory"
        );
    }
}

#[test]
fn creation_skips_closed_log_collisions_without_truncating_them() {
    let fixture = tempfile::tempdir().unwrap();
    let stamp = "20200101T000000Z";
    let (first_path, first_file, first_lease) =
        create_run_log_file("check", fixture.path(), stamp).unwrap();
    drop(first_file);
    drop(first_lease);
    fs::write(&first_path, "preserve original").unwrap();
    let (second_path, _, _) = create_run_log_file("check", fixture.path(), stamp).unwrap();
    assert_ne!(first_path, second_path);
    assert_eq!(fs::read_to_string(first_path).unwrap(), "preserve original");
}

#[test]
fn an_operational_log_creation_failure_stops_and_removes_the_unused_lease() {
    use super::super::retention::create_run_log_file_with;

    let fixture = tempfile::tempdir().unwrap();
    let attempts = std::cell::Cell::new(0);
    let result = create_run_log_file_with("check", fixture.path(), "20200101T000000Z", |path| {
        assert!(lease_path(path).is_file());
        attempts.set(attempts.get() + 1);
        Err(std::io::ErrorKind::PermissionDenied.into())
    });
    assert!(result.is_none());
    assert_eq!(attempts.get(), 1);
    let remaining: Vec<_> = fs::read_dir(fixture.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(remaining, [std::ffi::OsString::from(RETENTION_LOCK)]);
}

#[test]
fn nonregular_log_entries_do_not_count_toward_retention() {
    let fixture = tempfile::tempdir().unwrap();
    let blocked = fixture.path().join("20200101T000000Z-update-1.log");
    let removable = fixture.path().join("20200102T000000Z-update-2.log");
    fs::create_dir(&blocked).unwrap();
    fs::write(blocked.join("keep"), "foreign content").unwrap();
    fs::write(&removable, "# Dotfiles test\nfinished").unwrap();
    prune_run_logs(fixture.path(), 1);
    assert!(blocked.join("keep").exists());
    assert!(
        removable.exists(),
        "unrelated directories must not reduce the number of retained logs"
    );
}

#[test]
fn concurrent_log_creators_preserve_live_logs_until_their_leases_end() {
    let fixture = tempfile::tempdir().unwrap();
    let workers = 8;
    let barrier = std::sync::Barrier::new(workers + 1);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let barrier = &barrier;
            let dir = fixture.path();
            scope.spawn(move || {
                let run = RunLog::new_retaining("check", dir, Instant::now(), 1);
                barrier.wait();
                barrier.wait();
                let run = run.expect("concurrent log creation must succeed");
                run.emit(LogEvent::Info, "still writable");
                assert!(run.path().exists(), "retention deleted a live log");
            });
        }
        barrier.wait();
        let count = run_log_names(fixture.path()).len();
        barrier.wait();
        assert_eq!(
            count, workers,
            "active logs may temporarily exceed retention"
        );
    });
    prune_run_logs(fixture.path(), 1);
    assert_eq!(
        run_log_names(fixture.path()).len(),
        1,
        "released leases must be pruneable"
    );
}

#[test]
fn finished_run_releases_its_lease_even_when_another_handle_keeps_the_file_open() {
    let fixture = tempfile::tempdir().unwrap();
    let run = RunLog::new_retaining("check", fixture.path(), Instant::now(), 1).unwrap();
    let path = run.path().to_path_buf();
    let inherited = run.lease.try_clone().unwrap();
    prune_run_logs(fixture.path(), 0);
    assert!(path.exists(), "a live logger must retain its lease");
    drop(run);
    prune_run_logs(fixture.path(), 0);
    assert!(
        !path.exists(),
        "a duplicated handle must not keep a finished run leased"
    );
    drop(inherited);
}

fn run_log_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("read log dir")
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name != RETENTION_LOCK && !name.ends_with(".log.lock"))
        .collect();
    names.sort_unstable();
    names
}

#[test]
fn run_log_file_name_round_trips() {
    for command in ["install", "test-install-dry-run", "behavioral-ci"] {
        let name = run_log_file_name(command, "20260731T154210Z", 4321);
        let parsed = parse_run_log_file_name(&name).expect("name should parse");
        assert_eq!(parsed.stamp, "20260731T154210Z");
        assert_eq!(parsed.command, command);
    }
}

#[test]
fn parse_run_log_file_name_rejects_foreign_names() {
    for name in [
        "install.log",
        "notes.txt",
        "backup-notes-1.log",
        "20260731T15421Z-install-1.log",
        "20260731T1542100Z-install-1.log",
        "20260731X154210Z-install-1.log",
        "20260731T154210X-install-1.log",
        "2026073aT154210Z-install-1.log",
        "20260731T154210Z-install.log",
        "20260731T154210Z-install-notapid.log",
        "20260731T154210Z-install-1-extra.log",
        "-install-1.log",
    ] {
        assert!(
            parse_run_log_file_name(name).is_none(),
            "{name} should not parse as a run log"
        );
    }
}

#[test]
fn each_run_writes_its_own_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let first = RunLog::new("test", tmp.path(), Instant::now()).expect("first run log");
    let second = RunLog::new("test", tmp.path(), Instant::now()).expect("second run log");

    assert_ne!(
        first.path(),
        second.path(),
        "a second run must not reuse the first run's file"
    );
    assert_eq!(
        run_log_names(tmp.path()).len(),
        2,
        "both runs should be retained"
    );
}

#[test]
fn prune_keeps_the_newest_runs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    for stamp in ["20260731T154210Z", "20260731T154211Z", "20260731T154212Z"] {
        fs::write(
            tmp.path().join(run_log_file_name("install", stamp, 1)),
            "# Dotfiles test\n",
        )
        .expect("write log");
    }

    prune_run_logs(tmp.path(), 2);

    assert_eq!(
        run_log_names(tmp.path()),
        vec![
            "20260731T154211Z-install-1.log".to_string(),
            "20260731T154212Z-install-1.log".to_string(),
        ],
        "the oldest run should be pruned first"
    );
}

#[test]
fn retention_includes_hyphenated_test_runs_and_preserves_foreign_files() {
    let tmp = tempfile::tempdir().unwrap();
    let old = "20260730T154210Z-test-install-dry-run-1.log";
    let newest = "20260731T154210Z-behavioral-ci-2.log";
    for name in [old, newest] {
        fs::write(tmp.path().join(name), "# Dotfiles test\n").unwrap();
    }
    let foreign = [
        "backup-notes-1.log",
        "20260731T154210Z-test-command-notapid.log",
        "20260101T100000Z-backup-7.log",
    ];
    for name in foreign {
        fs::write(tmp.path().join(name), "my backup report").unwrap();
    }
    for _ in 0..2 {
        prune_run_logs(tmp.path(), 1);
        assert!(!tmp.path().join(old).exists());
        assert!(tmp.path().join(newest).exists());
        for name in foreign {
            assert_eq!(
                fs::read_to_string(tmp.path().join(name)).unwrap(),
                "my backup report"
            );
            assert!(!lease_path(&tmp.path().join(name)).exists());
        }
    }
}

#[test]
fn prune_orders_same_second_runs_by_precise_start_time() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let older = "20260731T154210Z-install-900.log";
    let newer = "20260731T154210Z-install-1000.log";
    fs::write(
        tmp.path().join(older),
        "# Dotfiles test 2026-07-31T15:42:10.000001Z\n",
    )
    .expect("write older log");
    fs::write(
        tmp.path().join(newer),
        "# Dotfiles test 2026-07-31T15:42:10.000002Z\n",
    )
    .expect("write newer log");

    prune_run_logs(tmp.path(), 1);

    assert_eq!(
        run_log_names(tmp.path()),
        vec![newer],
        "retention must use start time rather than command name or PID"
    );
}

#[test]
fn run_log_sort_stamp_preserves_legacy_and_incomplete_logs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("fixture.log");
    let fallback = concat!("20260731T154210", ".000000Z");
    for contents in [
        "",
        "legacy log content\n",
        "# Dotfiles test 2026-07-31T15:42:10Z\n",
        "# Dotfiles test 2026-07-31T15:42:10.notnumZ\n",
        "# Dotfiles test incomplete\n",
    ] {
        fs::write(&path, contents).expect("write legacy log");
        assert_eq!(
            run_log_sort_stamp(&path, "20260731T154210Z"),
            fallback,
            "unsupported headers must preserve filename ordering: {contents:?}"
        );
    }
    fs::remove_file(&path).expect("remove fixture");
    assert_eq!(
        run_log_sort_stamp(&path, "20260731T154210Z"),
        fallback,
        "a concurrently removed log must not fail discovery"
    );
}

#[test]
fn prune_ignores_files_that_are_not_run_logs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::write(tmp.path().join("notes.txt"), "keep me").expect("write note");
    fs::write(tmp.path().join("backup-notes-1.log"), "keep me").expect("write backup");
    fs::write(tmp.path().join("install.log"), "legacy").expect("write legacy log");
    fs::write(
        tmp.path()
            .join(run_log_file_name("install", "20260731T154210Z", 1)),
        "# Dotfiles test\n",
    )
    .expect("write log");

    prune_run_logs(tmp.path(), 0);

    assert_eq!(
        run_log_names(tmp.path()),
        vec![
            "backup-notes-1.log".to_string(),
            "install.log".to_string(),
            "notes.txt".to_string(),
        ],
        "only run logs should be pruned"
    );
}

#[test]
fn new_prunes_older_runs_beyond_retention() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::write(
        tmp.path()
            .join(run_log_file_name("install", "20200101T000000Z", 1)),
        "# Dotfiles test\nold",
    )
    .expect("write old log");

    let run_log = RunLog::new_retaining("test", tmp.path(), Instant::now(), 1).expect("run log");

    assert_eq!(
        run_log_names(tmp.path()),
        vec![
            run_log
                .path()
                .file_name()
                .expect("file name")
                .to_string_lossy()
                .into_owned()
        ],
        "only the current run should survive a retention of one"
    );
}

#[test]
fn retention_preserves_live_logs_and_prunes_them_after_drop() {
    let tmp = tempfile::tempdir().unwrap();
    let first = RunLog::new_retaining("first", tmp.path(), Instant::now(), 1).unwrap();
    let first_path = first.path().to_path_buf();
    let second = RunLog::new_retaining("second", tmp.path(), Instant::now(), 1).unwrap();
    first.emit(LogEvent::Info, "still writing");
    assert!(
        fs::read_to_string(&first_path)
            .unwrap()
            .contains("still writing")
    );
    assert_eq!(
        run_log_names(tmp.path()).len(),
        2,
        "live logs may exceed retention"
    );

    drop(first);
    prune_run_logs(tmp.path(), 1);
    assert!(!first_path.exists());
    assert!(
        !lease_path(&first_path).exists(),
        "pruning also removes the released lease"
    );
    assert!(second.path().exists());
    second.emit(LogEvent::Info, "survived retention");
    assert!(
        fs::read_to_string(second.path())
            .unwrap()
            .contains("survived retention")
    );
}

#[test]
fn retention_does_not_preserve_crashed_runs_forever() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("20200101T000000Z-update-1.log");
    fs::write(
        &path,
        "# Dotfiles test\nunfinished run without a finish record",
    )
    .unwrap();
    drop(open_lock(&lease_path(&path)).unwrap());
    let current = RunLog::new_retaining("update", tmp.path(), Instant::now(), 1).unwrap();
    assert!(!path.exists(), "an unlocked lease is not live");
    assert!(!lease_path(&path).exists());
    assert!(current.path().exists());
}
