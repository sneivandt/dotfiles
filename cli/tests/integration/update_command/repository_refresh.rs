use super::*;

#[test]
fn partial_repository_refresh_can_retry_with_fresh_configuration() {
    check_partial_repository_refresh(false);
}

#[test]
fn ignored_overlay_file_blocks_merge_and_survives_retry() {
    check_partial_repository_refresh(true);
}

#[cfg(target_os = "linux")]
#[test]
fn failed_fast_forward_with_partial_write_stops_apply_until_checkout_is_repaired() {
    use std::os::unix::fs::symlink;

    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    let repo = fixture.path().join("main");
    let publisher = fixture.path().join("publisher");
    std::fs::create_dir(&home).unwrap();
    common::setup_minimal_repo(&repo);
    std::fs::write(
        repo.join("conf/symlinks.toml"),
        "[base]\nsymlinks=['app-settings', {source='app-settings', target='.second-settings'}]\n",
    )
    .unwrap();
    let source = "symlinks/app-settings";
    let original = br#"{"appearance":"dark"}"#;
    std::fs::write(repo.join(source), original).unwrap();
    let link = home.join(".app-settings");
    symlink(repo.join(source), &link).unwrap();

    git(fixture.path(), &["init", "--bare", "-b", "main", "origin"]);
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["remote", "add", "origin", "../origin"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "initial"]);
    git(&repo, &["push", "-u", "origin", "main"]);
    let initial = git(&repo, &["rev-parse", "HEAD"]);
    git(fixture.path(), &["clone", "origin", "publisher"]);
    let upstream = format!(
        r#"{{"appearance":"dark","longSetting":"{}"}}"#,
        "x".repeat(180_000)
    );
    std::fs::write(publisher.join(source), &upstream).unwrap();
    git(&publisher, &["commit", "-am", "expand settings"]);
    git(&publisher, &["push", "origin", "main"]);

    let wrapper = install_git_wrapper(
        fixture.path(),
        r#"#!/bin/bash
if [[ $1 == merge ]]; then
    trap '' XFSZ
    ulimit -f 4 || exit
fi
exec "$FIXTURE_GIT" "$@"
"#,
    );
    let run = |tasks| common::isolated_cli_command(&repo, &home, None, "update", tasks, true);
    let failed = run("repository,symlinks,launcher")
        .envs(wrapper)
        .output()
        .unwrap();
    let text = common::combined_output(&failed);
    assert!(!failed.status.success(), "{text}");
    assert!(
        text.contains("the checkout changed during the failed merge"),
        "{text}"
    );
    assert!(
        text.contains("git status --short") && text.contains("git diff"),
        "{text}"
    );
    assert!(!home.join(".local/bin/dotfiles").exists(), "{text}");
    assert!(!text.contains("Home symlinks"), "{text}");
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), initial);
    assert_eq!(std::fs::read_link(&link).unwrap(), repo.join(source));
    assert_eq!(std::fs::read(&link).unwrap(), upstream.as_bytes()[..4096]);

    assert_partial_checkout_not_applied(&repo, &home);
    assert!(!home.join(".local/bin/dotfiles").exists());
    assert_eq!(std::fs::read(&link).unwrap(), upstream.as_bytes()[..4096]);

    let saved = home.join("saved-partial-settings");
    std::fs::copy(&link, &saved).unwrap();
    git(&repo, &["restore", "--source=HEAD", "--", source]);
    assert_eq!(std::fs::read(&link).unwrap(), original);
    std::fs::remove_file(repo.join(".git/dotfiles-merge-in-progress")).unwrap();
    let recovered = run("repository,symlinks,launcher").output().unwrap();
    assert!(recovered.status.success(), "{recovered:?}");
    assert_eq!(
        git(&repo, &["rev-parse", "HEAD"]),
        git(&publisher, &["rev-parse", "HEAD"])
    );
    assert_eq!(std::fs::read(&link).unwrap(), upstream.as_bytes());
    assert_eq!(
        std::fs::read(home.join(".second-settings")).unwrap(),
        upstream.as_bytes()
    );
    assert_eq!(std::fs::read(saved).unwrap(), upstream.as_bytes()[..4096]);
    assert!(home.join(".local/bin/dotfiles").is_file());
}

#[cfg(unix)]
#[test]
fn repository_update_preserves_tracked_edits_made_during_fetch_with_autostash_enabled() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    let root = fixture.path().join("main");
    std::fs::create_dir(&home).unwrap();
    common::setup_minimal_repo(&root);
    git(fixture.path(), &["init", "--bare", "-b", "main", "origin"]);
    git(&root, &["init", "-b", "main"]);
    git(&root, &["remote", "add", "origin", "../origin"]);
    std::fs::write(root.join("settings"), "initial\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "initial"]);
    git(&root, &["push", "-u", "origin", "main"]);
    let initial = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["config", "merge.autoStash", "true"]);
    git(fixture.path(), &["clone", "origin", "publisher"]);
    let publisher = fixture.path().join("publisher");
    std::fs::write(publisher.join("settings"), "upstream\n").unwrap();
    git(&publisher, &["commit", "-am", "change settings"]);
    git(&publisher, &["push", "origin", "main"]);

    let wrapper = install_git_wrapper(
        fixture.path(),
        r#"#!/bin/sh
if [ "$1" = fetch ]; then
    "$FIXTURE_GIT" "$@" || exit "$?"
    printf 'local edit after fetch\n' > settings
else
    exec "$FIXTURE_GIT" "$@"
fi
"#,
    );
    let run =
        || common::isolated_cli_command(&root, &home, None, "update", "repository,launcher", true);
    let output = run().envs(&wrapper).output().unwrap();
    let text = common::combined_output(&output);
    assert!(
        text.contains("tracked files changed during update"),
        "{text}"
    );
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), initial);
    assert_eq!(git(&root, &["stash", "list"]), "");
    assert_eq!(git(&root, &["ls-files", "--unmerged"]), "");
    assert_eq!(
        std::fs::read_to_string(root.join("settings")).unwrap(),
        "local edit after fetch\n"
    );
    // Repeat the race with a locally ahead branch: planning can skip its merge,
    // but that is not permission to deploy inputs captured before the edit.
    git(&root, &["restore", "settings"]);
    git(&root, &["merge", "--ff-only", "origin/main"]);
    git(&root, &["commit", "--allow-empty", "-m", "ahead"]);
    let ahead = run().envs(wrapper).output().unwrap();
    assert!(!ahead.status.success(), "{ahead:?}");
    assert!(!home.join(".local/bin/dotfiles").exists(), "{ahead:?}");
    // A new run starts with this user's dirty checkout, not a half-written merge.
    let dirty = run().output().unwrap();
    assert!(dirty.status.success(), "{dirty:?}");
    assert!(home.join(".local/bin/dotfiles").is_file(), "{dirty:?}");
}

#[cfg(target_os = "linux")]
fn assert_partial_checkout_not_applied(repo: &std::path::Path, home: &std::path::Path) {
    for (only, sync_repositories) in [
        ("repository,symlinks,launcher", true),
        ("symlinks", true),
        ("symlinks", false),
    ] {
        let retry =
            common::isolated_cli_command(repo, home, None, "update", only, sync_repositories)
                .output()
                .unwrap();
        assert!(!retry.status.success(), "{retry:?}");
        assert!(
            String::from_utf8_lossy(&retry.stderr).contains("recovery marker"),
            "{retry:?}"
        );
        assert!(
            !home.join(".second-settings").exists(),
            "blocked retry must not apply overlay symlinks"
        );
    }
}

#[cfg(unix)]
#[test]
fn checkout_changed_before_repository_discovery_rejects_the_stale_configuration() {
    for change_overlay in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("main");
        let overlay = fixture.path().join("overlay");
        let home = fixture.path().join("home");
        std::fs::create_dir(&home).unwrap();
        common::setup_minimal_repo(&root);
        std::fs::create_dir_all(overlay.join("conf")).unwrap();
        let changed = if change_overlay { &overlay } else { &root };
        std::fs::create_dir_all(changed.join("symlinks")).unwrap();
        let original = "[base]\nsymlinks = [{ source = 'original', target = '.fixture' }]\n";
        std::fs::write(changed.join("conf/symlinks.toml"), original).unwrap();
        std::fs::write(changed.join("symlinks/original"), "old contents").unwrap();
        std::fs::write(changed.join("symlinks/refreshed"), "fresh contents").unwrap();
        std::fs::write(overlay.join("tracked"), "initial").unwrap();
        for (checkout, remote) in [(&root, "main-origin"), (&overlay, "overlay-origin")] {
            initialize_local_remote(fixture.path(), checkout, remote);
        }
        let publisher = fixture.path().join("publish");
        let remote = if change_overlay {
            "overlay-origin"
        } else {
            "main-origin"
        };
        git(fixture.path(), &["clone", remote, "publish"]);
        std::fs::write(
            publisher.join("conf/symlinks.toml"),
            original.replace("original", "refreshed"),
        )
        .unwrap();
        git(&publisher, &["commit", "-am", "change desired symlink"]);
        git(&publisher, &["push", "origin", "main"]);
        let wrapper = install_git_wrapper(
            fixture.path(),
            r#"#!/bin/sh
if [ "$1" = symbolic-ref ] && [ "$PWD" -ef "$FIXTURE_CHECKOUT" ]; then
    "$FIXTURE_GIT" fetch origin main || exit $?
    "$FIXTURE_GIT" merge --ff-only origin/main >&2 || exit $?
fi
exec "$FIXTURE_GIT" "$@"
"#,
        );
        let update = |sync| {
            common::isolated_cli_command(
                &root,
                &home,
                Some(&overlay),
                "update",
                if sync {
                    "repository,symlinks"
                } else {
                    "symlinks"
                },
                sync,
            )
        };
        let stale = update(true)
            .env("FIXTURE_CHECKOUT", changed)
            .envs(wrapper)
            .output()
            .unwrap();
        assert!(!stale.status.success(), "{stale:?}");
        assert!(
            String::from_utf8_lossy(&stale.stderr)
                .contains("Git checkout changed while preparing tasks"),
            "{stale:?}"
        );
        assert_eq!(
            git(changed, &["rev-parse", "HEAD"]),
            git(&publisher, &["rev-parse", "HEAD"]),
        );
        assert!(std::fs::symlink_metadata(home.join(".fixture")).is_err());
        // A deliberately dirty checkout still works when its committed inputs
        // remain stable and the user requests a scoped update.
        std::fs::write(overlay.join("tracked"), "local edit").unwrap();
        let retry = update(false).output().unwrap();
        assert!(retry.status.success(), "{retry:?}");
        assert_eq!(
            std::fs::read_to_string(home.join(".fixture")).unwrap(),
            "fresh contents"
        );
        assert_eq!(
            std::fs::read_to_string(overlay.join("tracked")).unwrap(),
            "local edit"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn repository_merge_process_death_only_blocks_the_affected_worktree() {
    use std::os::unix::process::ExitStatusExt as _;

    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("main");
    let linked = fixture.path().join("linked");
    let home = fixture.path().join("home");
    std::fs::create_dir(&home).unwrap();
    common::setup_minimal_repo(&root);
    git(fixture.path(), &["init", "--bare", "-b", "main", "origin"]);
    git(&root, &["init", "-b", "main"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "initial"]);
    git(&root, &["remote", "add", "origin", "../origin"]);
    git(&root, &["push", "-u", "origin", "main"]);
    git(&root, &["worktree", "add", "-b", "linked", "../linked"]);
    git(&linked, &["branch", "-u", "origin/main"]);
    git(&root, &["commit", "--allow-empty", "-m", "advance"]);
    git(&root, &["push", "origin", "main"]);

    // Kill only our fixture CLI after the real merge but before it can verify
    // the index. Even an apparently clean, current checkout needs review.
    let wrapper = install_git_wrapper(
        fixture.path(),
        r#"#!/bin/bash
if [[ $1 == merge ]]; then
    "$FIXTURE_GIT" "$@" || exit $?
    [[ /proc/$PPID/exe -ef "$FIXTURE_CLI" ]] || exit 91
    kill -KILL "$PPID"
    exit
fi
exec "$FIXTURE_GIT" "$@"
"#,
    );
    let path = std::env::var_os("PATH").unwrap();
    // The usual fixture helper initializes .git; this checkout uses a linked gitfile.
    let run = |repo: &std::path::Path, only: &str| {
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_dotfiles"));
        cmd.env_clear()
            .env("HOME", &home)
            .env("PATH", &path)
            .env("TMPDIR", fixture.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", home.join(".gitconfig"))
            .env("DOTFILES_LOG_DIR", home.join("logs"))
            .env("DOTFILES_SKIP_SELF_UPDATE", "1")
            .current_dir(repo)
            .args([
                "update",
                "--non-interactive",
                "--profile",
                "base",
                "--only",
                only,
                "--root",
            ])
            .arg(repo);
        cmd
    };
    let dead = run(&linked, "repository,launcher")
        .env("FIXTURE_CLI", env!("CARGO_BIN_EXE_dotfiles"))
        .envs(wrapper)
        .output()
        .unwrap();
    assert_eq!(dead.status.signal(), Some(9), "{dead:?}");
    assert_eq!(git(&linked, &["status", "--porcelain"]), "");
    assert_eq!(
        git(&linked, &["rev-parse", "HEAD"]),
        git(&root, &["rev-parse", "HEAD"])
    );
    let marker = git2::Repository::open(&linked)
        .unwrap()
        .path()
        .join("dotfiles-merge-in-progress");
    assert!(marker.is_file());
    for extra in [None, Some("--dry-run")] {
        let mut retry = run(&linked, "repository,launcher");
        retry.args(extra);
        let retry = retry.output().unwrap();
        assert!(!retry.status.success() && marker.is_file(), "{retry:?}");
        assert!(!home.join(".local/bin/dotfiles").exists());
    }
    let sibling = run(&root, "repository").output().unwrap();
    assert!(sibling.status.success() && marker.is_file(), "{sibling:?}");
    std::fs::remove_file(marker).unwrap();
    let repaired = run(&linked, "repository,launcher").output().unwrap();
    assert!(repaired.status.success(), "{repaired:?}");
    assert!(home.join(".local/bin/dotfiles").is_file());
}

#[expect(
    clippy::too_many_lines,
    reason = "real Git remotes and update recovery"
)]
fn check_partial_repository_refresh(ignored: bool) {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    let root = fixture.path().join("main");
    let overlay = fixture.path().join("overlay");
    std::fs::create_dir(&home).unwrap();
    common::setup_minimal_repo(&root);
    std::fs::create_dir(&overlay).unwrap();
    for (repo, remote_name) in [(&root, "main-upstream"), (&overlay, "overlay-upstream")] {
        git(
            fixture.path(),
            &["init", "--bare", "-b", "main", remote_name],
        );
        git(repo, &["init", "-b", "main"]);
        std::fs::write(repo.join("README"), "initial").unwrap();
        if ignored && repo == &overlay {
            std::fs::write(repo.join(".gitignore"), "collision\n").unwrap();
        }
        git(repo, &["add", "."]);
        git(repo, &["commit", "-m", "initial"]);
        let remote = fixture.path().join(remote_name);
        git(repo, &["remote", "add", "origin", remote.to_str().unwrap()]);
        git(repo, &["push", "-u", "origin", "main"]);
    }
    let initial_main = git(&root, &["rev-parse", "HEAD"]);
    let initial_overlay = git(&overlay, &["rev-parse", "HEAD"]);
    std::fs::write(
        root.join("conf/symlinks.toml"),
        "[base]\nsymlinks = [\"fresh\"]\n",
    )
    .unwrap();
    std::fs::write(root.join("symlinks/fresh"), "new configuration").unwrap();
    std::fs::write(overlay.join("collision"), "upstream").unwrap();
    git(&overlay, &["add", "-f", "collision"]);
    std::fs::create_dir(overlay.join("conf")).unwrap();
    std::fs::create_dir(overlay.join("symlinks")).unwrap();
    std::fs::write(
        overlay.join("conf/symlinks.toml"),
        "[base]\nsymlinks = [\"second\"]\n",
    )
    .unwrap();
    std::fs::write(overlay.join("symlinks/second"), "fresh overlay").unwrap();
    for (repo, initial) in [(&root, &initial_main), (&overlay, &initial_overlay)] {
        git(repo, &["add", "."]);
        git(repo, &["commit", "-m", "advance"]);
        git(repo, &["push", "origin", "main"]);
        git(repo, &["reset", "--hard", initial]);
    }
    let obstruction = overlay.join("collision");
    std::fs::write(&obstruction, "local untracked data").unwrap();

    if ignored {
        let preview = common::isolated_cli_command(
            &root,
            &home,
            Some(&overlay),
            "update",
            "repository",
            true,
        )
        .arg("--dry-run")
        .output()
        .unwrap();
        assert!(preview.status.success(), "{preview:?}");
        assert!(
            String::from_utf8_lossy(&preview.stdout)
                .contains("local merge eligibility not checked"),
            "preview must disclose the unchecked local collision: {preview:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&obstruction).unwrap(),
            "local untracked data",
            "preview must preserve the ignored file"
        );
    }

    let update = || {
        let output = common::isolated_cli_command(
            &root,
            &home,
            Some(&overlay),
            "update",
            "repository,symlinks",
            true,
        )
        .output()
        .unwrap();
        let text = common::combined_output(&output);
        (output.status, text)
    };
    let (status, text) = update();
    assert!(!status.success(), "{text}");
    assert!(text.contains("Already advanced repository"), "{text}");
    let history = read_retained_log(&home.join("logs"), &["--task", "repository", "--raw"]);
    let outcome = history
        .lines()
        .filter_map(|line| line.split_once(" [record] "))
        .map(|(_, json)| serde_json::from_str::<serde_json::Value>(json).unwrap())
        .find(|r| r["type"] == "task_result")
        .expect("recorded repository result");
    assert_eq!(outcome["status"], "failed", "{outcome}");
    assert_eq!(outcome["actions"]["applied"], 1, "{outcome}");
    assert_eq!(outcome["actions"]["failed"], 1, "{outcome}");
    assert_ne!(
        git(&root, &["rev-parse", "HEAD"]),
        initial_main,
        "the main checkout should advance before the overlay blocks"
    );
    assert_eq!(
        git(&overlay, &["rev-parse", "HEAD"]),
        initial_overlay,
        "the blocked overlay should keep its original HEAD"
    );
    assert_eq!(
        std::fs::read_to_string(&obstruction).unwrap(),
        "local untracked data",
        "the overlay merge must preserve the local obstruction"
    );
    assert!(
        !home.join(".fresh").exists(),
        "convergence must wait for repository sync"
    );

    let saved = home.join("saved-collision");
    std::fs::rename(obstruction, &saved).unwrap();
    // A skip on the first checkout must not bypass recovery in the second.
    std::fs::write(root.join("README"), "intentional local edit").unwrap();
    let (blocked, note) = update();
    assert!(
        !blocked.success() && note.contains("recovery marker"),
        "{note}"
    );
    assert!(
        !home.join(".fresh").exists(),
        "blocked overlay recovery must prevent convergence"
    );
    git(&root, &["restore", "README"]);
    std::fs::remove_file(overlay.join(".git/dotfiles-merge-in-progress")).unwrap();
    let (retry_status, retry_text) = update();
    assert!(retry_status.success(), "{retry_text}");
    assert_eq!(
        std::fs::read_to_string(saved).unwrap(),
        "local untracked data",
        "saved collision file must retain original contents"
    );
    assert_ne!(
        git(&overlay, &["rev-parse", "HEAD"]),
        initial_overlay,
        "the overlay should advance after the obstruction is removed"
    );
    assert_eq!(
        std::fs::read_to_string(home.join(".second")).unwrap(),
        "fresh overlay",
        "the retry must apply the refreshed overlay configuration"
    );
    assert_eq!(
        std::fs::read_to_string(home.join(".fresh")).unwrap(),
        "new configuration",
        "the retry must apply the refreshed main configuration"
    );

    git(&root, &["commit", "--allow-empty", "-m", "local only"]);
    let local_head = git(&root, &["rev-parse", "HEAD"]);
    let check_status = |expected: &str, dry_run| {
        let mut command =
            common::isolated_cli_command(&root, &home, None, "update", "repository", true);
        if dry_run {
            command.arg("--dry-run");
        }
        let output = command.output().unwrap();
        let status_text = common::combined_output(&output);
        if dry_run {
            assert!(output.status.success(), "{status_text}");
        }
        assert!(!status_text.contains("would change"), "{status_text}");
        assert!(status_text.contains(expected), "{status_text}");
        assert_eq!(
            git(&root, &["rev-parse", "HEAD"]),
            local_head,
            "ahead checkout HEAD must stay unchanged"
        );
    };
    check_status("local branch is ahead of upstream", true);
    check_status("local branch is ahead of upstream", false);

    let publisher = fixture.path().join("publisher");
    git(
        fixture.path(),
        &["clone", "--no-local", "main-upstream", "publisher"],
    );
    git(
        &publisher,
        &["commit", "--allow-empty", "-m", "upstream only"],
    );
    git(&publisher, &["push", "origin", "main"]);
    let tracking_before = git(&root, &["rev-parse", "@{u}"]);
    check_status("cannot determine fast-forward eligibility", true);
    assert_eq!(
        git(&root, &["rev-parse", "@{u}"]),
        tracking_before,
        "preview must not update the local tracking ref"
    );

    git(&root, &["fetch", "origin"]);
    check_status("local branch diverged from upstream", true);
    check_status("local branch diverged from upstream", false);
}

#[test]
fn repository_update_isolates_checkout_and_honors_system_git_configuration() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    let root = fixture.path().join("repo");
    let upstream = fixture.path().join("upstream");
    std::fs::create_dir(&home).unwrap();
    common::setup_minimal_repo(&root);
    git(&root, &["init", "-b", "main"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "initial"]);
    let initial = git(&root, &["rev-parse", "HEAD"]);
    git(
        fixture.path(),
        &["clone", root.to_str().unwrap(), upstream.to_str().unwrap()],
    );
    git(
        &root,
        &["remote", "add", "origin", upstream.to_str().unwrap()],
    );
    git(&root, &["fetch", "origin"]);
    git(&root, &["branch", "--set-upstream-to=origin/main"]);
    // Publish only after establishing the old tracking ref so the first apply
    // must parse a real fast-forward line with a blank porcelain status.
    std::fs::write(upstream.join("new-file"), "upstream content").unwrap();
    git(&upstream, &["add", "new-file"]);
    git(&upstream, &["commit", "-m", "advance"]);
    let expected = git(&upstream, &["rev-parse", "HEAD"]);
    git(
        &root,
        &["remote", "set-url", "origin", "audit-system://upstream"],
    );
    let system_config = fixture.path().join("system.gitconfig");
    git(
        &root,
        &[
            "config",
            "--file",
            system_config.to_str().unwrap(),
            &format!("url.{}.insteadOf", upstream.to_str().unwrap()),
            "audit-system://upstream",
        ],
    );
    let unrelated = fixture.path().join("unrelated");
    git(
        fixture.path(),
        &[
            "clone",
            "--branch",
            "main",
            root.to_str().unwrap(),
            "unrelated",
        ],
    );
    let unrelated_index = unrelated.join(".git/index");
    let original_index = std::fs::read(&unrelated_index).unwrap();
    for dry_run in [true, false] {
        let mut command =
            common::isolated_cli_command(&root, &home, None, "update", "repository", true);
        command
            .env("GIT_CONFIG_SYSTEM", &system_config)
            .env("GIT_CONFIG_NOSYSTEM", "0")
            .env("GIT_DIR", unrelated.join(".git"))
            .env("GIT_COMMON_DIR", unrelated.join(".git"))
            .env("GIT_WORK_TREE", &unrelated)
            .env("GIT_INDEX_FILE", &unrelated_index)
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "branch.main.remote")
            .env("GIT_CONFIG_VALUE_0", "wrong-remote");
        if dry_run {
            command.arg("--dry-run");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "dry_run={dry_run}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            git(&root, &["rev-parse", "HEAD"]),
            if dry_run {
                initial.as_str()
            } else {
                expected.as_str()
            }
        );
        assert_eq!(
            git(&root, &["rev-parse", "@{u}"]),
            if dry_run {
                initial.as_str()
            } else {
                expected.as_str()
            },
            "preview preserves tracking state and the first apply advances it"
        );
        assert_eq!(git(&unrelated, &["rev-parse", "HEAD"]), initial);
        assert_eq!(std::fs::read(&unrelated_index).unwrap(), original_index);
        assert!(!unrelated.join(".git/FETCH_HEAD").exists());
        assert!(!unrelated.join("new-file").exists());
    }
}

#[test]
fn repository_only_update_recovers_bad_config_and_refuses_a_deleted_upstream() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("checkout");
    let home = fixture.path().join("home");
    let remote = fixture.path().join("remote");
    common::setup_minimal_repo(&root);
    std::fs::create_dir(&home).unwrap();
    let config = root.join("conf/symlinks.toml");
    std::fs::write(&config, "[invalid").unwrap();
    git(
        fixture.path(),
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    git(&root, &["init", "-b", "main"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "broken configuration"]);
    let broken = git(&root, &["rev-parse", "HEAD"]);
    git(
        &root,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&root, &["push", "-u", "origin", "main"]);
    std::fs::write(&config, "").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "repair configuration"]);
    let fixed = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["push", "origin", "main"]);
    git(&root, &["reset", "--hard", &broken]);
    git(&root, &["config", "branch.main.mergeOptions", "--squash"]);

    let overlay = tempfile::tempdir().unwrap();
    let update = |selector| {
        common::isolated_cli_command(&root, &home, Some(overlay.path()), "update", selector, true)
    };
    let config_file = std::fs::File::options().write(true).open(&config).unwrap();
    config_file
        .set_times(std::fs::FileTimes::new().set_modified(
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000),
        ))
        .unwrap();
    drop(config_file);
    let index = root.join(".git/index");
    let index_before = std::fs::read(&index).unwrap();
    let preview = update("repository").arg("--dry-run").output().unwrap();
    assert!(preview.status.success(), "{preview:?}");
    let settings = git2::Repository::open(&root).unwrap().config().unwrap();
    assert_eq!(
        settings.get_string("dotfiles.overlay").unwrap(),
        overlay.path().to_string_lossy(),
        "repository-only preview saves the explicit overlay despite invalid aggregate configuration"
    );
    assert_eq!(
        std::fs::read(&index).unwrap(),
        index_before,
        "repository preview must leave the Git index unchanged"
    );
    assert!(
        String::from_utf8_lossy(&preview.stdout).contains("local merge eligibility not checked"),
        "repository-only preview must reach Git despite the invalid config: {preview:?}"
    );
    assert_eq!(
        git(&root, &["rev-parse", "HEAD"]),
        broken,
        "this operation must leave HEAD at its original commit"
    );
    let repair = update("Dotfiles repository").output().unwrap();
    assert!(repair.status.success(), "{repair:?}");
    assert!(
        String::from_utf8_lossy(&repair.stdout).contains("Tasks · 1 changed"),
        "repository-only repair must report the change: {repair:?}"
    );
    assert_eq!(
        git(&root, &["rev-parse", "HEAD"]),
        fixed,
        "must fast-forward to the repair commit even with squash configured"
    );
    let status = git(&root, &["status", "--porcelain", "-uno"]);
    assert!(status.is_empty(), "expected a clean checkout: {status}");
    let content = std::fs::read(&config).unwrap();
    assert!(content.is_empty(), "must restore the valid config");

    git(&root, &["reset", "--hard", &broken]);
    git(&remote, &["update-ref", "-d", "refs/heads/main"]);
    assert_eq!(
        git(&root, &["rev-parse", "@{u}"]),
        fixed,
        "the deleted branch must remain available through the stale tracking ref"
    );
    let missing = update("repository").output().unwrap();
    assert!(!missing.status.success(), "{missing:?}");
    assert_eq!(
        git(&root, &["rev-parse", "HEAD"]),
        broken,
        "this operation must leave HEAD at its original commit"
    );
}
