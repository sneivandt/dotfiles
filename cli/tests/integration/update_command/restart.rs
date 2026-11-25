use super::*;

#[test]
fn restarted_overlay_update_keeps_a_validated_exclusion_when_its_script_is_retired() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("main");
    let overlay = fixture.path().join("overlay");
    let home = fixture.path().join("home");
    std::fs::create_dir(&home).unwrap();
    common::setup_minimal_repo(&root);
    std::fs::create_dir_all(overlay.join("conf")).unwrap();
    for (repo, remote) in [(&root, "main-origin"), (&overlay, "overlay-origin")] {
        git(fixture.path(), &["init", "--bare", "-b", "main", remote]);
        git(repo, &["init", "-b", "main"]);
        let path = fixture.path().join(remote);
        git(repo, &["remote", "add", "origin", path.to_str().unwrap()]);
    }

    let overlay_scripts = overlay.join("conf/scripts.toml");
    std::fs::write(
        &overlay_scripts,
        "[base]\nscripts = [{ name = 'Retire fixture', path = 'retired.sh' }]\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        std::fs::create_dir(overlay.join("symlinks")).unwrap();
        std::fs::write(
            overlay.join("conf/symlinks.toml"),
            "[base]\nsymlinks = ['retired-source', 'foreign-replacement']\n",
        )
        .unwrap();
        for name in ["retired-source", "foreign-replacement"] {
            std::fs::write(overlay.join("symlinks").join(name), "managed\n").unwrap();
        }
        std::os::unix::fs::symlink(
            overlay.join("symlinks/retired-source"),
            home.join(".retired-source"),
        )
        .unwrap();
        std::fs::write(home.join("foreign-source"), "personal\n").unwrap();
        std::os::unix::fs::symlink("foreign-source", home.join(".foreign-replacement")).unwrap();
    }
    for repo in [&root, &overlay] {
        git(repo, &["add", "."]);
        git(repo, &["commit", "-m", "initial"]);
        git(repo, &["push", "-u", "origin", "main"]);
    }
    let initial = git(&overlay, &["rev-parse", "HEAD"]);
    // Publish the retirement from a separate clone so the tested checkout remains clean.
    git(fixture.path(), &["clone", "overlay-origin", "publisher"]);
    let publisher = fixture.path().join("publisher");
    std::fs::write(publisher.join("conf/scripts.toml"), "").unwrap();
    #[cfg(unix)]
    git(
        &publisher,
        &[
            "rm",
            "conf/symlinks.toml",
            "symlinks/retired-source",
            "symlinks/foreign-replacement",
        ],
    );
    git(&publisher, &["commit", "-am", "retire fixture script"]);
    git(&publisher, &["push", "origin", "main"]);

    let output = common::isolated_cli_command(
        &root,
        &home,
        Some(&overlay),
        "update",
        "repository,launcher",
        true,
    )
    .args(["--skip", "script-retire-fixture"])
    .output()
    .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_ne!(git(&overlay, &["rev-parse", "HEAD"]), initial);
    #[cfg(unix)]
    {
        let text = common::combined_output(&output);
        assert!(text.contains("points to a missing source"), "{text}");
        assert!(!text.contains("foreign-replacement"), "{text}");
        assert_eq!(
            std::fs::read_link(home.join(".retired-source")).unwrap(),
            overlay.join("symlinks/retired-source"),
        );
        assert_eq!(
            std::fs::read_to_string(home.join(".foreign-replacement")).unwrap(),
            "personal\n",
        );
    }
    let launcher = if cfg!(windows) {
        "dotfiles.cmd"
    } else {
        "dotfiles"
    };
    assert!(home.join(".local/bin").join(launcher).is_file());
}

#[cfg(unix)]
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "real repository restart, parent death, and concurrent command fixture"
)]
fn running_repository_restart_keeps_shared_overlay_and_home_locked_after_parent_death() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("main");
    let overlay = fixture.path().join("overlay");
    let home = fixture.path().join("home");
    let other_home = fixture.path().join("other-home");
    for directory in [&home, &other_home, &overlay] {
        std::fs::create_dir(directory).unwrap();
    }
    common::setup_minimal_repo(&root);
    std::fs::create_dir(overlay.join("conf")).unwrap();
    std::fs::write(
        overlay.join("conf/scripts.toml"),
        "[base]\nscripts = [{ name = 'Hold fixture', path = 'hold.sh' }]\n",
    )
    .unwrap();
    std::fs::write(
        overlay.join("hold.sh"),
        r#"#!/bin/sh
case "$1" in
    --check) test -f "$HOME/running"; exit $?;;
    --remove) touch "$HOME/removed"; exit $?;;
esac
touch "$HOME/running" || exit 1
tries=0
while [ ! -f "$HOME/proceed" ] && [ "$tries" -lt 300 ]; do
    tries=$((tries + 1))
    sleep 0.1
done
test -f "$HOME/proceed" && cat revision > "$HOME/result"
"#,
    )
    .unwrap();
    std::fs::write(overlay.join("revision"), "original").unwrap();
    for (checkout, remote) in [(&root, "main-origin"), (&overlay, "overlay-origin")] {
        initialize_local_remote(fixture.path(), checkout, remote);
    }
    let other = fixture.path().join("other");
    git(fixture.path(), &["clone", "main-origin", "other"]);
    git(
        &other,
        &["commit", "--allow-empty", "-m", "trigger restart"],
    );
    git(&other, &["push", "origin", "main"]);

    let mut parent = common::isolated_cli_command(
        &root,
        &home,
        Some(&overlay),
        "update",
        "repository,script-hold-fixture",
        true,
    )
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while !home.join("running").is_file() {
        assert!(Instant::now() < deadline, "restarted script never ran");
        assert!(parent.try_wait().unwrap().is_none(), "update exited early");
        std::thread::sleep(Duration::from_millis(25));
    }
    let initial_overlay = git(&overlay, &["rev-parse", "HEAD"]);
    git(
        fixture.path(),
        &["clone", "overlay-origin", "publish-overlay"],
    );
    let publisher = fixture.path().join("publish-overlay");
    std::fs::write(publisher.join("revision"), "updated").unwrap();
    git(&publisher, &["commit", "-am", "advance overlay"]);
    git(&publisher, &["push", "origin", "main"]);

    let sync_overlay = || {
        common::isolated_cli_command(
            &other,
            &other_home,
            Some(&overlay),
            "update",
            "repository",
            true,
        )
        .output()
        .unwrap()
    };
    let remove_script = || {
        common::cli_command(
            &root,
            &home,
            Some(&overlay),
            "remove",
            "script-hold-fixture",
        )
        .output()
        .unwrap()
    };
    let while_parent_alive = sync_overlay();
    parent.kill().unwrap();
    assert!(!parent.wait().unwrap().success());
    // Older binaries attempt to lock these original files exclusively without
    // checking the continuation files.
    let legacy_blocked = [
        root.join(".git/dotfiles-run.lock"),
        home.join(".local/state/dotfiles/home-run.lock"),
        overlay.join(".git/dotfiles-run.lock"),
    ]
    .map(|path| {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .unwrap();
        matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock))
    });
    let while_child_alive = sync_overlay();
    let remove_while_child_alive = remove_script();
    let held_overlay = git(&overlay, &["rev-parse", "HEAD"]);
    let removed_early = home.join("removed").exists();

    std::fs::write(home.join("proceed"), "").unwrap();
    let output = parent.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Repository synced · restarting"),
        "{output:?}"
    );
    assert!(home.join("result").is_file(), "{output:?}");
    assert_eq!(
        std::fs::read_to_string(home.join("result")).unwrap(),
        "original"
    );
    for blocked in [
        while_parent_alive,
        while_child_alive,
        remove_while_child_alive,
    ] {
        assert!(!blocked.status.success(), "{blocked:?}");
        assert!(
            String::from_utf8_lossy(&blocked.stderr).contains("another dotfiles run"),
            "{blocked:?}"
        );
    }
    assert_eq!(held_overlay, initial_overlay);
    assert_eq!(legacy_blocked, [true; 3]);
    assert!(!removed_early);
    let finished_sync = sync_overlay();
    assert!(finished_sync.status.success(), "{finished_sync:?}");
    assert_ne!(git(&overlay, &["rev-parse", "HEAD"]), initial_overlay);
    let finished_remove = remove_script();
    assert!(finished_remove.status.success(), "{finished_remove:?}");
    assert!(home.join("removed").exists());
}

#[cfg(unix)]
#[test]
fn repository_restart_preserves_non_unicode_paths_and_loads_new_configuration() {
    use std::os::unix::ffi::OsStringExt as _;
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture
        .path()
        .join(std::ffi::OsString::from_vec(b"checkout-\xff".to_vec()));
    let home = fixture.path().join("home");
    let remote = fixture.path().join("upstream");
    std::fs::create_dir(&home).unwrap();
    common::setup_minimal_repo(&root);
    git(
        fixture.path(),
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "initial"]);
    let initial = git(&root, &["rev-parse", "HEAD"]);
    git(
        &root,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&root, &["push", "-u", "origin", "main"]);
    std::fs::write(
        root.join("conf/symlinks.toml"),
        "[base]\nsymlinks = [\"fresh\"]\n",
    )
    .unwrap();
    std::fs::write(root.join("symlinks/fresh"), "new configuration\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "new configuration"]);
    let updated = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["push", "origin", "main"]);
    git(&root, &["reset", "--hard", &initial]);
    let run_update = || {
        common::isolated_cli_command(&root, &home, None, "update", "repository,symlinks", true)
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    };
    let result = run_update();
    assert!(result.status.success(), "{result:?}");
    let stdout = String::from_utf8(result.stdout).unwrap();
    let after_restart = stdout
        .split_once("Repository synced · restarting to load configuration\n")
        .expect("expected the repository restart notice")
        .1;
    assert!(
        after_restart.starts_with('\n') && !after_restart.starts_with("\n\n"),
        "expected one separator after the restart notice in {stdout:?}"
    );
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), updated);
    assert_eq!(
        std::fs::read_to_string(home.join(".fresh")).unwrap(),
        "new configuration\n"
    );
    assert_eq!(
        std::fs::read_link(home.join(".fresh")).unwrap(),
        root.join("symlinks/fresh")
    );
    let logs = common::retained_logs(&home.join("logs"));
    assert_eq!(logs.len(), 2, "restart must create a linked child run");
    let child = logs
        .iter()
        .map(|log| std::fs::read_to_string(log).unwrap())
        .any(|log| log.contains("\"parent_run_id\":\""));
    assert!(child, "restart must retain parent-run linkage");

    // Repeat the fast-forward with the managed link already in place.
    git(&root, &["reset", "--hard", &initial]);
    let repeat = run_update();
    assert!(repeat.status.success(), "{repeat:?}");
    let repeat_stdout = String::from_utf8(repeat.stdout).unwrap();
    assert!(
        repeat_stdout.contains(
            "Repository synced · restarting to load configuration\n\nTasks · 1 unchanged ·"
        ),
        "expected the no-change summary after one separator in {repeat_stdout:?}"
    );
}
