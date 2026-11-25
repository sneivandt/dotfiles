use super::*;

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
fn update_task_catalog_satisfies_structural_contract() {
    let tasks = update_tasks();
    common::assert_task_catalog_contract("update", &tasks);
}

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
                expected.push("login-shell");
            }
            expected.sort_unstable();
            assert_eq!(applicable, expected, "{platform:?}, parallel={parallel}");
        }
    }
}

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
