#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::wildcard_imports,
    clippy::indexing_slicing,
    reason = "panicking allowed at this trust boundary"
)]
//! Integration tests for the `update` command.
//!
//! These tests exercise the full task list produced by [`all_update_tasks`],
//! the task-selector filtering applied by the `--skip` and `--only` CLI
//! flags, and the structural properties of the update dependency graph.

mod common;

use dotfiles_cli::testing as test_api;

use test_api::config::ConfigStore;
use test_api::platform::{Os, Platform};
use test_api::tasks;

/// Build an update task list backed by a store loaded from a minimal repo.
fn update_tasks() -> Vec<Box<dyn tasks::Task>> {
    update_tasks_for_platform(Platform::detect())
}

fn update_tasks_for_platform(platform: Platform) -> Vec<Box<dyn tasks::Task>> {
    let ctx = common::IntegrationTestContext::new();
    let store = ConfigStore::from_config(ctx.load_config_for_platform("base", platform));
    tasks::all_update_tasks(&store)
}

#[test]
fn shared_home_lock_blocks_other_checkouts_before_configuration_mutations() {
    use std::io::{Read as _, Seek as _};

    let first = common::IntegrationTestContext::new();
    let second = common::IntegrationTestContext::new();
    let home = tempfile::tempdir().unwrap();
    let launcher = home.path().join(".local/bin").join(if cfg!(windows) {
        "dotfiles.cmd"
    } else {
        "dotfiles"
    });
    let lock_path = if cfg!(windows) {
        home.path().join("AppData/Local/dotfiles/home-run.lock")
    } else {
        home.path().join(".local/state/dotfiles/home-run.lock")
    };
    std::fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
    let mut lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .unwrap();
    lock.try_lock().unwrap();
    let contents = b"fixture owner";
    std::io::Write::write_all(&mut lock, contents).unwrap();

    for repo in [&first, &second] {
        for (verb, selector) in [
            ("update", "launcher"),
            ("remove", "launcher"),
            ("check", "config-files"),
        ] {
            let mut command =
                common::cli_command(repo.root_path(), home.path(), None, verb, selector);
            command.env("XDG_STATE_HOME", home.path().join("other-state"));
            command.env("LOCALAPPDATA", home.path().join("other-local"));
            let result = command.output().unwrap();
            let output = format!(
                "{}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(!result.status.success(), "{verb}: {output}");
            assert!(output.contains("another dotfiles run"), "{verb}: {output}");
            assert!(output.contains("home-run.lock"), "{verb}: {output}");
            assert!(
                !launcher.exists(),
                "a blocked command must not install the launcher"
            );
        }
    }
    lock.rewind().unwrap();
    assert_eq!(
        lock.metadata().unwrap().len(),
        u64::try_from(contents.len()).unwrap()
    );
    let mut owner = vec![0; contents.len()];
    lock.read_exact(&mut owner).unwrap();
    assert_eq!(owner, contents, "contenders must preserve lock metadata");
    drop(lock);
    let result = common::cli_command(second.root_path(), home.path(), None, "update", "launcher")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        launcher.is_file(),
        "retry must install the launcher after the lock is released"
    );
}

#[test]
fn nested_cli_fixture_preserves_ancestor_settings_and_run_lock() {
    use std::io::{Read as _, Seek as _, Write as _};

    let ancestor = common::TestContextBuilder::new()
        .with_git_hooks_dir()
        .build();
    let repository = git2::Repository::open(ancestor.root_path()).unwrap();
    let mut settings = repository.config().unwrap();
    settings.set_str("dotfiles.profile", "desktop").unwrap();
    settings
        .set_str("dotfiles.overlay", "ancestor-overlay-sentinel")
        .unwrap();
    drop(settings);
    let config_path = repository.path().join("config");
    let original_config = std::fs::read(&config_path).unwrap();
    let lock_path = repository.path().join("dotfiles-run.lock");
    let original_lock = b"ancestor run-lock sentinel\n";
    let mut lock = std::fs::OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    lock.write_all(original_lock).unwrap();
    lock.sync_all().unwrap();

    for locked in [false, true] {
        if locked {
            lock.try_lock().expect("hold the ancestor's run lock");
        }
        let root = ancestor.root_path().join(format!("nested-{locked}"));
        common::setup_minimal_repo(&root);
        assert_eq!(
            dunce::canonicalize(git2::Repository::discover(&root).unwrap().path()).unwrap(),
            dunce::canonicalize(repository.path()).unwrap(),
            "before isolation, this fixture must discover the ancestor"
        );
        let home = tempfile::tempdir().unwrap();
        let overlay = tempfile::tempdir().unwrap();
        let output = common::cli_command(
            &root,
            home.path(),
            Some(overlay.path()),
            "update",
            "completions",
        )
        .output()
        .unwrap();
        assert!(
            output.status.success(),
            "ancestor_locked={locked}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let child = git2::Repository::open(&root).unwrap();
        let child_settings = child.config().unwrap();
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("base profile"),
            "the child must use its requested profile rather than the ancestor's desktop selection"
        );
        assert_eq!(
            child_settings.get_string("dotfiles.overlay").unwrap(),
            overlay.path().to_string_lossy(),
            "the explicit overlay must be persisted only in the child"
        );
        assert!(child.path().join("dotfiles-run.lock").is_file());
        assert_eq!(
            std::fs::read(&config_path).unwrap(),
            original_config,
            "ancestor_locked={locked}: ancestor settings must remain byte-for-byte unchanged"
        );
        lock.rewind().unwrap();
        assert_eq!(
            lock.metadata().unwrap().len(),
            u64::try_from(original_lock.len()).unwrap(),
            "ancestor_locked={locked}: ancestor lock length must not change"
        );
        let mut contents = vec![0; original_lock.len()];
        lock.read_exact(&mut contents).unwrap();
        assert_eq!(
            contents, original_lock,
            "ancestor_locked={locked}: ancestor lock must not be overwritten"
        );
        if locked {
            lock.unlock().unwrap();
        }
    }
}

#[cfg(unix)]
#[test]
fn update_console_separates_tasks_and_keeps_no_op_compact() {
    use std::os::unix::fs::PermissionsExt as _;

    for verbose in [false, true] {
        for symbols in [false, true] {
            let repo = common::TestContextBuilder::new()
                .with_config_file("symlinks.toml", "[base]\nsymlinks = [\"example\"]\n")
                .with_symlink_source_content("example", "example\n")
                .with_config_file(
                    "chmod.toml",
                    "[base]\npermissions = [{ path = \"example\", mode = \"600\" }]\n",
                )
                .build();
            std::fs::set_permissions(
                repo.root_path().join("symlinks/example"),
                std::fs::Permissions::from_mode(0o644),
            )
            .unwrap();
            let home = tempfile::tempdir().unwrap();
            let overlay = tempfile::tempdir().unwrap();
            let mut command = common::cli_command(
                repo.root_path(),
                home.path(),
                Some(overlay.path()),
                "update",
                "symlinks,file-permissions",
            );
            if verbose {
                command.arg("--verbose");
            }
            if !symbols {
                command.arg("--no-symbols");
            }

            // Both runs mutate only the temporary repository and home.
            // The second run exercises hidden current rows and verbose rows.
            for changed in [true, false] {
                let output = command.output().expect("run isolated update");
                let text = String::from_utf8(output.stdout).unwrap();
                assert!(
                    output.status.success(),
                    "{text}\n{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(
                    !text.contains('\u{1b}'),
                    "piped output must be plain: {text:?}"
                );
                assert!(!text.contains("\n\n\n"), "extra blank line: {text:?}");
                let blocks: Vec<_> = text.trim().split("\n\n").collect();
                assert_eq!(
                    blocks.len(),
                    if changed || verbose { 4 } else { 2 },
                    "{text}"
                );
                if changed || verbose {
                    let status = match (changed, symbols) {
                        (true, true) => "✓",
                        (true, false) => "CHANGED",
                        (false, true) => "○",
                        (false, false) => "CURRENT",
                    };
                    for (block, name) in blocks[1..3]
                        .iter()
                        .zip(["Home symlinks", "File permissions"])
                    {
                        assert!(block.starts_with(&format!("{status} {name}")), "{text}");
                    }
                }
                assert!(
                    blocks.last().unwrap().starts_with(if changed {
                        "2 tasks changed"
                    } else {
                        "No changes · 2 tasks current"
                    }),
                    "{text}"
                );
            }
        }
    }
}

#[test]
fn conflicting_overlay_desired_state_stops_update_before_selected_tasks_run() {
    let mut cases = vec![(
        "git-config.toml",
        "[base]\nsettings = [{ key = \"core.editor\", value = \"vim\" }]\n",
        "[base]\nsettings = [\
         { key = \"core.editor\", value = \"vim\" },\
         { key = \"CORE.EDITOR\", value = \"nano\" }]\n",
        "git.conflicting-values",
    )];
    if cfg!(windows) {
        cases.push((
            "registry.toml",
            "[console]\npath = 'HKCU:\\Console'\n[console.values]\nFontSize = 14\n",
            "[console]\npath = 'HKCU:\\Console'\n[console.values]\nFontSize = 14\n\
             [other]\npath = 'hkcu:\\console'\n[other.values]\nfontsize = 15\n",
            "registry.conflicting-values",
        ));
    }
    for (file, main, overlay_content, code) in cases {
        for dry_run in [false, true] {
            let repo = common::TestContextBuilder::new()
                .with_config_file(file, main)
                .with_config_file(
                    "symlinks.toml",
                    "[base]\nsymlinks = [\"conflict-sentinel\"]\n",
                )
                .with_symlink_source_content("conflict-sentinel", "must not be installed")
                .build();
            let overlay = tempfile::tempdir().unwrap();
            let home = tempfile::tempdir().unwrap();
            std::fs::create_dir(overlay.path().join("conf")).unwrap();
            std::fs::write(overlay.path().join("conf").join(file), overlay_content).unwrap();
            let mut command = common::cli_command(
                repo.root_path(),
                home.path(),
                Some(overlay.path()),
                "update",
                "symlinks",
            );
            if dry_run {
                command.arg("--dry-run");
            }
            let output = command.output().expect("run isolated update");
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                !output.status.success(),
                "{file}, dry_run={dry_run}: {text}"
            );
            assert!(text.contains(code), "{file}, dry_run={dry_run}: {text}");
            assert!(
                text.contains(&overlay.path().join("conf").join(file).display().to_string()),
                "{text}"
            );
            assert!(
                home.path()
                    .join(".conflict-sentinel")
                    .symlink_metadata()
                    .is_err(),
                "even an unrelated selected task must not mutate before validation succeeds"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Structural invariants
// ---------------------------------------------------------------------------

#[test]
fn update_task_catalog_satisfies_structural_contract() {
    let tasks = update_tasks();
    common::assert_task_catalog_contract("update", &tasks);
}

// ---------------------------------------------------------------------------
// Task-selector filtering
// ---------------------------------------------------------------------------

#[test]
fn update_filters_preview_exactly_the_selected_tasks_without_mutation() {
    let cases: &[(&str, &str, &str, &[&str])] = &[
        ("single only", "symlinks", "", &["symlinks"]),
        (
            "only union",
            "symlinks,git-hooks",
            "",
            &["git-hooks", "symlinks"],
        ),
        (
            "single skip",
            "symlinks,git-hooks",
            "symlinks",
            &["git-hooks"],
        ),
        (
            "multiple skips",
            "symlinks,git-hooks",
            "symlinks,completions",
            &["git-hooks"],
        ),
        (
            "outside selected subset",
            "symlinks,git-hooks",
            "registry",
            &["git-hooks", "symlinks"],
        ),
        (
            "skip packages",
            "symlinks,git-hooks",
            "packages",
            &["git-hooks", "symlinks"],
        ),
        (
            "redundant repository skip",
            "symlinks",
            "repository",
            &["symlinks"],
        ),
        (
            "normalized display name",
            "HOME_SYMLINKS",
            "",
            &["symlinks"],
        ),
    ];
    for parallel in [false, true] {
        for (case, only, skip, expected) in cases {
            let repo = common::TestContextBuilder::new()
                .with_config_file("symlinks.toml", "[base]\nsymlinks = ['example']\n")
                .with_symlink_source_content("example", "source must not change\n")
                .with_hook_source("pre-commit", "#!/bin/sh\nexit 0\n")
                .with_git_hooks_dir()
                .build();
            let home = tempfile::tempdir().unwrap();
            let mut command =
                common::cli_command(repo.root_path(), home.path(), None, "update", only);
            command.args(["--dry-run", "--format", "jsonl"]);
            if !skip.is_empty() {
                command.args(["--skip", skip]);
            }
            if !parallel {
                command.arg("--no-parallel");
            }
            let output = command.output().unwrap();
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(
                output.status.success(),
                "{case}, parallel={parallel}: {text}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_previewed_tasks(&text, expected, case, parallel);
            assert!(home.path().join(".example").symlink_metadata().is_err());
            assert!(!repo.root_path().join(".git/hooks/pre-commit").exists());
            assert_eq!(
                std::fs::read(repo.root_path().join("symlinks/example")).unwrap(),
                b"source must not change\n"
            );
            assert_eq!(
                std::fs::read(repo.root_path().join("hooks/pre-commit")).unwrap(),
                b"#!/bin/sh\nexit 0\n"
            );
        }
    }
}

#[track_caller]
fn assert_previewed_tasks(text: &str, expected: &[&str], case: &str, parallel: bool) {
    let records: Vec<serde_json::Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).expect("valid JSON record"))
        .collect();
    let mut outcomes: Vec<_> = records
        .iter()
        .filter(|record| record["type"] == "task_result")
        .map(|record| {
            (
                record["selector"].as_str().expect("task selector"),
                record["status"].as_str().expect("task status"),
            )
        })
        .collect();
    outcomes.sort_unstable();
    let expected: Vec<_> = expected
        .iter()
        .map(|selector| (*selector, "dry_run"))
        .collect();
    assert_eq!(outcomes, expected, "{case}, parallel={parallel}: {text}");
}

// ---------------------------------------------------------------------------
// Dry-run: task list from a minimal repository
// ---------------------------------------------------------------------------

#[test]
fn minimal_update_catalog_applicability_is_platform_specific_and_scheduler_independent() {
    let platforms = [
        Platform {
            os: Os::Linux,
            is_arch: false,
            is_wsl: false,
        },
        Platform {
            os: Os::Windows,
            is_arch: false,
            is_wsl: false,
        },
    ];

    for platform in platforms {
        for parallel in [false, true] {
            let ctx = common::TestContextBuilder::new().build();
            let ec = ctx.make_context_with_executor(
                "base",
                platform,
                tasks::ContextOpts {
                    dry_run: true,
                    parallel,
                    is_ci: Some(false),
                },
                std::sync::Arc::new(common::StubExecutor),
            );
            let mut applicable: Vec<_> = tasks::all_update_tasks(&ec.store)
                .into_iter()
                .filter(|task| task.should_run(&ec.ctx))
                .map(|task| task.selector().to_string())
                .collect();
            applicable.sort_unstable();
            let mut expected = vec![
                "agent-settings",
                "completions",
                "git",
                "launcher",
                "path",
                "symlinks",
            ];
            if platform.os == Os::Windows {
                expected.extend(["developer-mode", "registry"]);
            } else {
                expected.extend(["file-permissions", "login-shell"]);
            }
            expected.sort_unstable();
            assert_eq!(applicable, expected, "{platform:?}, parallel={parallel}");
        }
    }
}

// ---------------------------------------------------------------------------
// Expected task presence
// ---------------------------------------------------------------------------

#[test]
fn update_task_catalog_contains_required_tasks() {
    let tasks = update_tasks();
    let selectors: Vec<&str> = tasks.iter().map(|task| task.selector()).collect();
    for required in ["repository", "symlinks", "git-hooks", "git", "system-files"] {
        assert!(
            selectors.contains(&required),
            "update task catalog is missing required selector '{required}'"
        );
    }
}

// ---------------------------------------------------------------------------
// update::run: full dry-run pipeline
// ---------------------------------------------------------------------------

/// Calling `update::run` with `--only` matching no selector must explain how
/// to discover valid selectors.
#[test]
fn update_run_unknown_filters_return_actionable_errors() {
    let cases: &[(&str, &[&str], &[&str])] = &[
        ("--only", &[], &["zzznomatch"]),
        ("--only", &[], &["symlinks", "zzznomatch"]),
        ("--only", &[], &["update"]),
        ("--only", &[], &["report"]),
        ("--skip", &["zzznomatch"], &["symlinks"]),
    ];
    for (flag, skip, only) in cases {
        let result = common::run_update_dry_run(skip, only, false);
        let error = result.expect_err("an unknown selector should fail");
        let message = error.to_string();
        assert!(
            message.contains(&format!("{flag} did not match a task selector")),
            "{message}"
        );
        assert!(message.contains("dotfiles list"), "{message}");
    }
}

/// Calling `update::run` with contradictory `--skip` and `--only` selectors
/// must fail instead of reporting a successful no-op.
#[test]
fn update_run_rejects_filters_that_select_no_tasks() {
    let result = common::run_update_dry_run(&["symlinks"], &["symlinks"], false);
    let error = result.expect_err("contradictory task filters should fail");
    assert!(error.to_string().contains("selected no tasks"));
}

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
            && history.contains(&format!("/ {parent_id}")),
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

fn read_retained_log(log_dir: &std::path::Path, args: &[&str]) -> String {
    let home = log_dir.parent().unwrap();
    let result = common::cli_command(home, home, None, "log", "")
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
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
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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

#[test]
fn partial_repository_refresh_can_retry_with_fresh_configuration() {
    check_partial_repository_refresh(false);
}

#[test]
fn ignored_overlay_file_blocks_merge_and_survives_retry() {
    check_partial_repository_refresh(true);
}

#[allow(
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
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        (output.status, text)
    };
    let (status, text) = update();
    assert!(!status.success(), "{text}");
    assert!(text.contains("Already advanced repository"), "{text}");
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

    std::fs::remove_file(obstruction).unwrap();
    let (retry_status, retry_text) = update();
    assert!(retry_status.success(), "{retry_text}");
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
    let check_preview = |expected: &str| {
        let output = common::isolated_cli_command(&root, &home, None, "update", "repository", true)
            .arg("--dry-run")
            .output()
            .unwrap();
        let preview_text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success(), "{preview_text}");
        assert!(!preview_text.contains("would change"), "{preview_text}");
        assert!(preview_text.contains(expected), "{preview_text}");
    };
    check_preview("local branch is ahead of upstream");

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
    check_preview("cannot determine fast-forward eligibility");
    assert_eq!(
        git(&root, &["rev-parse", "@{u}"]),
        tracking_before,
        "preview must not update the local tracking ref"
    );

    git(&root, &["fetch", "origin"]);
    check_preview("local branch diverged from upstream");
}

fn git(root: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@test.local",
        ])
        .args([
            "-c",
            "core.hooksPath=unused-hooks",
            "-c",
            "commit.gpgSign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", root.join("no-global-config"))
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
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
    let result =
        common::isolated_cli_command(&root, &home, None, "update", "repository,symlinks", true)
            .output()
            .unwrap();
    assert!(result.status.success(), "{result:?}");
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

    let update = || common::isolated_cli_command(&root, &home, None, "update", "repository", true);
    let config_file = std::fs::File::options().write(true).open(&config).unwrap();
    config_file
        .set_times(std::fs::FileTimes::new().set_modified(
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000),
        ))
        .unwrap();
    drop(config_file);
    let index = root.join(".git/index");
    let index_before = std::fs::read(&index).unwrap();
    let preview = update().arg("--dry-run").output().unwrap();
    assert!(preview.status.success(), "{preview:?}");
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
    let repair = update().output().unwrap();
    assert!(repair.status.success(), "{repair:?}");
    assert!(
        String::from_utf8_lossy(&repair.stdout).contains("1 task changed"),
        "repository-only repair must report the change: {repair:?}"
    );
    assert_eq!(
        git(&root, &["rev-parse", "HEAD"]),
        fixed,
        "must reach the repair commit"
    );
    assert!(
        std::fs::read(&config).unwrap().is_empty(),
        "must restore the valid config"
    );

    git(&root, &["reset", "--hard", &broken]);
    git(&remote, &["update-ref", "-d", "refs/heads/main"]);
    assert_eq!(
        git(&root, &["rev-parse", "@{u}"]),
        fixed,
        "the deleted branch must remain available through the stale tracking ref"
    );
    let missing = update().output().unwrap();
    assert!(!missing.status.success(), "{missing:?}");
    assert_eq!(
        git(&root, &["rev-parse", "HEAD"]),
        broken,
        "this operation must leave HEAD at its original commit"
    );
}
