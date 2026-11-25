use super::*;

#[test]
fn retained_history_selects_exact_runs_and_preserves_parent_and_actions() {
    let repo = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", "[base]\nsymlinks = [\"log-example\"]\n")
        .with_symlink_source_content("log-example", "example\n")
        .build();
    let home = tempfile::tempdir().unwrap();
    let overlay = tempfile::tempdir().unwrap();
    let log_dir = home.path().join("logs");
    let mut command = common::cli_command(
        repo.root_path(),
        home.path(),
        Some(overlay.path()),
        "update",
        "symlinks",
    );
    command.arg("--dry-run");
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let logs = common::retained_logs(&log_dir);
    assert_eq!(logs.len(), 1);
    let parent = logs.first().unwrap();
    let parent_id = parent.file_stem().unwrap().to_str().unwrap();
    let parent_contents = std::fs::read_to_string(parent).unwrap();
    let records: Vec<serde_json::Value> = parent_contents
        .lines()
        .filter_map(|line| {
            line.split_once(" [record] ").map(|(_, json)| {
                serde_json::from_str(json).expect("every structured record must be valid JSON")
            })
        })
        .collect();
    assert!(
        records
            .iter()
            .any(|r| r["type"] == "run_start" && r["run_id"] == parent_id)
    );
    assert!(
        records.iter().any(|r| r["type"] == "run_finish"
            && r["outcome"] == "succeeded"
            && r["exit_code"] == 0)
    );
    assert!(records.iter().any(|r| r["type"] == "action"
        && r["planned"] == true
        && r["subject"].as_str().unwrap().contains("log-example")));
    let task_id = records
        .iter()
        .find(|r| r["type"] == "task_result" && r["status"] == "dry_run")
        .unwrap()["task_id"]
        .as_str()
        .unwrap();

    // The same binary accepts explicit parent linkage without changing the preview scope.
    command.args(["--parent-run-id", parent_id]);
    assert!(command.output().unwrap().status.success());
    let history = read_retained_log(&log_dir, &["--list"]);
    assert!(
        history.contains("succeeded")
            && history.contains("base")
            && history
                .lines()
                .skip(1)
                .any(|row| row.split_whitespace().last() == Some(parent_id)),
        "{history}"
    );
    assert_eq!(
        read_retained_log(&log_dir, &["--id", parent_id, "--raw"]),
        parent_contents
    );
    let task_log = read_retained_log(&log_dir, &["--id", parent_id, "--task", task_id]);
    assert!(
        task_log.contains("log-example") && !task_log.contains("[run_start]"),
        "{task_log}"
    );
    assert!(task_log.contains("Home symlinks · Planned:"), "{task_log}");
    let normal = read_retained_log(&log_dir, &["--id", parent_id]);
    assert_eq!(normal.matches("base profile").count(), 1, "{normal}");
    assert!(normal.contains(" · overlay "), "{normal}");
    assert_eq!(
        normal.matches("Home symlinks: Planned").count(),
        1,
        "{normal}"
    );
    for internal in [task_id, "[main]", "elapsed_us", "[info]"] {
        assert!(!normal.contains(internal), "{normal}");
    }
    assert_eq!(
        common::retained_logs(&log_dir).len(),
        2,
        "viewing logs must not create a run"
    );
    assert!(
        home.path().join(".log-example").symlink_metadata().is_err(),
        "preview must not install the link"
    );
}

#[cfg(unix)]
#[test]
fn retained_actions_appear_once_for_update_and_remove() {
    let repo = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", "[base]\nsymlinks = [\"log-example\"]\n")
        .with_symlink_source_content("log-example", "example\n")
        .build();
    let home = tempfile::tempdir().unwrap();
    let logs = home.path().join("logs");
    for (verb, attempt) in [("update", "resource_apply"), ("remove", "resource_remove")] {
        let result = common::cli_command(repo.root_path(), home.path(), None, verb, "symlinks")
            .output()
            .unwrap();
        assert!(result.status.success(), "{verb}: {result:?}");
        let normal = read_retained_log(&logs, &["--task", "symlinks"]);
        assert_eq!(
            normal
                .lines()
                .filter(|line| line.contains("log-example"))
                .count(),
            1,
            "{verb}: {normal}"
        );
        let verbose = read_retained_log(&logs, &["--task", "symlinks", "-v"]);
        assert!(
            verbose.contains(&format!("[{attempt}]")),
            "{verb}: {verbose}"
        );
        assert!(verbose.contains("[action]"), "{verb}: {verbose}");
    }
}

#[test]
fn startup_failure_writes_finished_history_and_an_exact_diagnostic_hint() {
    let repo = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", "this is invalid toml")
        .build();
    let home = tempfile::tempdir().unwrap();
    let overlay = tempfile::tempdir().unwrap();
    let log_dir = home.path().join("logs");
    let output = common::cli_command(
        repo.root_path(),
        home.path(),
        Some(overlay.path()),
        "update",
        "symlinks",
    )
    .arg("--dry-run")
    .output()
    .unwrap();
    assert!(!output.status.success());
    let text = common::combined_output(&output);
    let id = text
        .split_once("Run 'dotfiles log --id ")
        .unwrap()
        .1
        .split_once(" -v'")
        .unwrap()
        .0;
    let raw = read_retained_log(&log_dir, &["--id", id, "--raw"]);
    assert!(
        raw.contains("\"type\":\"run_finish\"") && raw.contains("\"outcome\":\"failed\""),
        "{raw}"
    );
    assert!(read_retained_log(&log_dir, &["--list"]).contains("failed"));
}

#[cfg(unix)]
#[test]
fn repository_update_uses_each_fetch_result_when_a_later_fetch_overwrites_fetch_head() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    let root = fixture.path().join("main");
    let overlay = fixture.path().join("overlay");
    for dir in [&home, &overlay] {
        std::fs::create_dir(dir).unwrap();
    }
    common::setup_minimal_repo(&root);

    let mut expected = Vec::new();
    for (repo, name) in [(&root, "main-remote"), (&overlay, "overlay-remote")] {
        git(fixture.path(), &["init", "--bare", "-b", "main", name]);
        git(repo, &["init", "-b", "main"]);
        git(repo, &["add", "."]);
        git(repo, &["commit", "--allow-empty", "-m", "initial"]);
        let initial = git(repo, &["rev-parse", "HEAD"]);
        let remote = fixture.path().join(name);
        git(repo, &["remote", "add", "origin", remote.to_str().unwrap()]);
        git(repo, &["push", "-u", "origin", "main"]);
        git(repo, &["commit", "--allow-empty", "-m", name]);
        let upstream = git(repo, &["rev-parse", "HEAD"]);
        git(repo, &["push", "origin", "main"]);
        git(repo, &["commit", "--allow-empty", "-m", "unrelated"]);
        let unrelated = git(repo, &["rev-parse", "HEAD"]);
        git(repo, &["push", "origin", "HEAD:refs/heads/unrelated"]);
        git(repo, &["reset", "--hard", &initial]);
        expected.push((repo, upstream, unrelated));
    }

    let wrapper = install_git_wrapper(
        fixture.path(),
        r#"#!/bin/sh
if [ "$1" = fetch ]; then
    "$FIXTURE_GIT" "$@" || exit "$?"
    "$FIXTURE_GIT" fetch --quiet --no-tags origin refs/heads/unrelated
else
    exec "$FIXTURE_GIT" "$@"
fi
"#,
    );
    let output =
        common::isolated_cli_command(&root, &home, Some(&overlay), "update", "repository", true)
            .envs(wrapper)
            .output()
            .unwrap();
    assert!(output.status.success(), "{output:?}");
    for (repo, upstream, unrelated) in expected {
        assert_eq!(git(repo, &["rev-parse", "FETCH_HEAD"]), unrelated);
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), upstream);
        assert_eq!(git(repo, &["rev-parse", "@{u}"]), upstream);
    }
}
