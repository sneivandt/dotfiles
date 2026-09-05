#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::wildcard_imports,
    clippy::indexing_slicing,
    reason = "panicking allowed at this trust boundary"
)]
//! Integration tests for the `install` command.
//!
//! These tests exercise the full task list produced by [`all_install_tasks`],
//! the task-selector filtering applied by the `--skip` and `--only` CLI
//! flags, and the structural properties of the install dependency graph.

mod common;

use dotfiles_cli::testing as test_api;

use test_api::config::ConfigStore;
use test_api::platform::{Os, Platform};
use test_api::tasks;
use test_api::tasks::filter::task_matches_filter;

/// Build an install task list backed by a store loaded from a minimal repo.
fn install_tasks() -> Vec<Box<dyn tasks::Task>> {
    install_tasks_for_platform(Platform::detect())
}

fn install_tasks_for_platform(platform: Platform) -> Vec<Box<dyn tasks::Task>> {
    let ctx = common::IntegrationTestContext::new();
    let store = ConfigStore::from_config(ctx.load_config_for_platform("base", platform));
    tasks::all_install_tasks(&store)
}

#[cfg(unix)]
#[test]
fn install_console_separates_tasks_and_keeps_no_op_compact() {
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
            let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_dotfiles"));
            command
                .args([
                    "install",
                    "--profile",
                    "base",
                    "--only",
                    "symlinks,file-permissions",
                    "--no-repo-update",
                    "--non-interactive",
                ])
                .arg("--root")
                .arg(repo.root_path())
                .arg("--overlay")
                .arg(overlay.path())
                .env("HOME", home.path())
                .env("XDG_STATE_HOME", home.path().join("state"))
                .env("XDG_CACHE_HOME", home.path().join("cache"))
                .env("DOTFILES_LOG_DIR", home.path().join("logs"))
                .env("DOTFILES_SKIP_SELF_UPDATE", "1")
                .env_remove("LOCALAPPDATA")
                .env_remove("DOTFILES_OVERLAY")
                .env_remove("DOTFILES_REEXEC_GUARD")
                .env_remove("DOTFILES_SELF_UPDATE_REEXEC_GUARD")
                .env_remove("DOTFILES_REPOSITORY_REEXEC_GUARD");
            if verbose {
                command.arg("--verbose");
            }
            if !symbols {
                command.arg("--no-symbols");
            }

            // Both runs mutate only the temporary repository and home.
            // The second run exercises hidden current rows and verbose rows.
            for changed in [true, false] {
                let output = command.output().expect("run isolated install");
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
                        (true, false) => "CHANGE",
                        (false, true) => "○",
                        (false, false) => "OK",
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
                        "2 changed"
                    } else {
                        "No changes · 2 current"
                    }),
                    "{text}"
                );
            }
        }
    }
}

#[test]
fn conflicting_desired_state_stops_install_before_selected_tasks_run() {
    let mut cases = vec![(
        "git-config.toml",
        "[base]\nsettings = [{ key = \"core.editor\", value = \"vim\" }]\n",
        "[base]\nsettings = [{ key = \"CORE.EDITOR\", value = \"nano\" }]\n",
        "git.conflicting-values",
    )];
    if cfg!(windows) {
        cases.push((
            "registry.toml",
            "[console]\npath = 'HKCU:\\Console'\n[console.values]\nFontSize = 14\n",
            "[console]\npath = 'hkcu:\\console'\n[console.values]\nfontsize = 15\n",
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
            let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_dotfiles"));
            command
                .args([
                    "install",
                    "--profile",
                    "base",
                    "--only",
                    "symlinks",
                    "--no-repo-update",
                    "--non-interactive",
                ])
                .arg("--root")
                .arg(repo.root_path())
                .arg("--overlay")
                .arg(overlay.path())
                .env("HOME", home.path())
                .env("USERPROFILE", home.path())
                .env("DOTFILES_SKIP_SELF_UPDATE", "1")
                .env_remove("DOTFILES_OVERLAY")
                .env_remove("DOTFILES_REEXEC_GUARD")
                .env_remove("DOTFILES_SELF_UPDATE_REEXEC_GUARD")
                .env_remove("DOTFILES_REPOSITORY_REEXEC_GUARD");
            if dry_run {
                command.arg("--dry-run");
            }
            let output = command.output().expect("run isolated install");
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
            // The CLI canonicalizes --root, expanding Windows short path names.
            let root = dunce::canonicalize(repo.root_path()).expect("canonicalize repository root");
            assert!(
                text.contains(&root.join("conf").join(file).display().to_string()),
                "{text}"
            );
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
fn install_task_catalog_satisfies_structural_contract() {
    let tasks = install_tasks();
    common::assert_task_catalog_contract("install", &tasks);
}

// ---------------------------------------------------------------------------
// --skip filter
// ---------------------------------------------------------------------------

/// Tasks matching the skip selector must be excluded from the filtered list.
#[test]
fn skip_filter_excludes_matching_tasks() {
    let all_tasks = install_tasks();
    let skip_keyword = "packages";

    let filtered: Vec<&str> = all_tasks
        .iter()
        .filter(|t| !task_matches_filter(t.as_ref(), skip_keyword))
        .map(|t| t.name())
        .collect();

    for task in all_tasks
        .iter()
        .filter(|task| filtered.contains(&task.name()))
    {
        assert!(
            !task_matches_filter(task.as_ref(), skip_keyword),
            "task '{}' should have been excluded by --skip {skip_keyword}",
            task.name(),
        );
    }
    // At least one task was removed
    assert!(
        filtered.len() < all_tasks.len(),
        "--skip packages should remove at least one task"
    );
}

/// When the skip keyword does not match any task name the full list is returned.
#[test]
fn skip_filter_with_no_match_returns_all_tasks() {
    let all_tasks = install_tasks();
    let skip_keyword = "zzznomatch";
    let total = all_tasks.len();

    let filtered_count = all_tasks
        .iter()
        .filter(|t| !task_matches_filter(t.as_ref(), skip_keyword))
        .count();

    assert_eq!(
        filtered_count, total,
        "--skip with non-matching keyword should leave task count unchanged"
    );
}

// ---------------------------------------------------------------------------
// --only filter
// ---------------------------------------------------------------------------

/// Only tasks matching the `--only` selector should remain.
#[test]
fn only_filter_includes_only_matching_tasks() {
    let all_tasks = install_tasks();
    let only_keyword = "symlinks";

    let filtered: Vec<&str> = all_tasks
        .iter()
        .filter(|t| task_matches_filter(t.as_ref(), only_keyword))
        .map(|t| t.name())
        .collect();

    assert_eq!(
        filtered,
        vec!["Home symlinks"],
        "--only symlinks should return exactly one task"
    );
}

/// Canonical selectors disambiguate similar task names.
#[test]
fn only_filter_disambiguates_update_tasks() {
    let all_tasks = install_tasks();
    let filtered: Vec<&str> = all_tasks
        .iter()
        .filter(|t| task_matches_filter(t.as_ref(), "repository"))
        .map(|t| t.name())
        .collect();

    assert_eq!(filtered, vec!["Dotfiles repository"]);

    let unmatched = all_tasks
        .iter()
        .any(|t| task_matches_filter(t.as_ref(), "update"));

    assert!(
        !unmatched,
        "ambiguous selectors like 'update' should not match any task"
    );
}

/// Internal task labels do not create heuristic selectors.
#[test]
fn only_filter_does_not_match_internal_report_task_by_keyword() {
    let all_tasks = install_tasks();
    let no_match = !all_tasks
        .iter()
        .any(|t| task_matches_filter(t.as_ref(), "report"));

    assert!(no_match);
}

/// When `--only` matches nothing the result is an empty list.
#[test]
fn only_filter_with_no_match_returns_empty() {
    let all_tasks = install_tasks();
    let only_keyword = "zzznomatch";

    let any_match = all_tasks
        .iter()
        .any(|t| task_matches_filter(t.as_ref(), only_keyword));

    assert!(
        !any_match,
        "--only with non-matching keyword should return empty list"
    );
}

// ---------------------------------------------------------------------------
// Dry-run: task list from a minimal repository
// ---------------------------------------------------------------------------

#[test]
fn install_tasks_assess_on_linux_and_windows() {
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
        let ctx = common::TestContextBuilder::new().build();
        let ec = ctx.make_system_context(
            "base",
            platform,
            tasks::ContextOpts {
                dry_run: true,
                parallel: false,
                is_ci: None,
            },
        );

        for task in tasks::all_install_tasks(&ec.store) {
            let _ = task.should_run(&ec.ctx);
        }
    }
}

// ---------------------------------------------------------------------------
// Expected task presence
// ---------------------------------------------------------------------------

#[test]
fn install_task_catalog_contains_required_tasks() {
    let tasks = install_tasks();
    let selectors: Vec<&str> = tasks.iter().map(|task| task.selector()).collect();
    for required in ["symlinks", "git-hooks", "git"] {
        assert!(
            selectors.contains(&required),
            "install task catalog is missing required selector '{required}'"
        );
    }
}

// ---------------------------------------------------------------------------
// --skip filter: multiple keywords
// ---------------------------------------------------------------------------

/// When multiple keywords are provided, tasks matching any one of them must
/// be excluded.
#[test]
fn skip_with_multiple_keywords_excludes_all_matching() {
    let all_tasks = install_tasks();
    let skip_keywords = ["packages", "registry"];

    let filtered: Vec<&str> = all_tasks
        .iter()
        .filter(|t| {
            !skip_keywords
                .iter()
                .any(|kw| task_matches_filter(t.as_ref(), kw))
        })
        .map(|t| t.name())
        .collect();

    for task in all_tasks
        .iter()
        .filter(|task| filtered.contains(&task.name()))
    {
        for kw in &skip_keywords {
            assert!(
                !task_matches_filter(task.as_ref(), kw),
                "task '{}' should have been excluded by --skip {kw}",
                task.name(),
            );
        }
    }
    assert!(
        filtered.len() < all_tasks.len(),
        "--skip with multiple keywords should remove at least one task"
    );
}

// ---------------------------------------------------------------------------
// --only filter: multiple keywords
// ---------------------------------------------------------------------------

/// When multiple selectors are provided, tasks matching any one of them must
/// all be included (union, not intersection).
#[test]
fn only_with_multiple_keywords_includes_all_matching() {
    let all_tasks = install_tasks();
    let only_keywords = ["symlinks", "git-hooks"];

    let filtered: Vec<&str> = all_tasks
        .iter()
        .filter(|t| {
            only_keywords
                .iter()
                .any(|kw| task_matches_filter(t.as_ref(), kw))
        })
        .map(|t| t.name())
        .collect();

    assert!(filtered.contains(&"Home symlinks"));
    assert!(filtered.contains(&"Git hooks"));

    for task in all_tasks
        .iter()
        .filter(|task| filtered.contains(&task.name()))
    {
        assert!(
            only_keywords
                .iter()
                .any(|kw| task_matches_filter(task.as_ref(), kw)),
            "task '{}' should not have been included",
            task.name()
        );
    }
}

// ---------------------------------------------------------------------------
// ApplyFilePermissions: real filesystem chmod
// ---------------------------------------------------------------------------

/// `ApplyFilePermissions.run()` must set the declared mode on an existing file.
///
/// Creates `$HOME/.ssh/config` with permissions `0o644`, then runs the task
/// and asserts that the permissions are updated to `0o600`.
#[cfg(unix)]
#[test]
fn apply_file_permissions_run_sets_mode_on_unix() {
    use std::os::unix::fs::PermissionsExt;

    use test_api::tasks::Task;

    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "chmod.toml",
            "[base]\npermissions = [{ mode = \"600\", path = \"ssh/config\" }]\n",
        )
        .build();

    let platform = Platform {
        os: Os::Linux,
        is_arch: false,
        is_wsl: false,
    };
    let ec = ctx.make_system_context(
        "base",
        platform,
        tasks::ContextOpts {
            dry_run: false,
            parallel: false,
            is_ci: Some(false),
        },
    );

    // Create $HOME/.ssh/config with mode 0o644.
    let ssh_dir = ec.ctx.home().join(".ssh");
    std::fs::create_dir_all(&ssh_dir).expect("create .ssh dir");
    let ssh_config = ssh_dir.join("config");
    std::fs::write(&ssh_config, "").expect("create ssh config");
    std::fs::set_permissions(&ssh_config, std::fs::Permissions::from_mode(0o644))
        .expect("set initial permissions");

    let result = tasks::files::chmod::ApplyFilePermissions::new(ec.store.chmod.clone())
        .run(&ec.ctx)
        .expect("apply file permissions run");
    assert!(
        matches!(result, tasks::TaskResult::Batch(ref stats) if stats.changed_count() > 0),
        "apply file permissions should succeed"
    );

    let perms = std::fs::metadata(&ssh_config)
        .expect("read file metadata")
        .permissions();
    assert_eq!(
        perms.mode() & 0o777,
        0o600,
        "file permissions should be 0o600 after applying chmod"
    );
}

// ---------------------------------------------------------------------------
// install::run: full dry-run pipeline
// ---------------------------------------------------------------------------

/// Calling `commands::install::run` with `dry_run: true` must return `Ok(())`
/// without making any filesystem changes.
#[test]
fn install_run_dry_run_returns_ok() {
    let result = common::run_install_dry_run(vec![], vec![], false);
    assert!(
        result.is_ok(),
        "dry-run install should return Ok: {result:?}"
    );
}

/// Calling `install::run` with `--only symlinks` in dry-run mode must return
/// `Ok(())` and execute only matching tasks.
#[test]
fn install_run_dry_run_with_only_filter_returns_ok() {
    let result = common::run_install_dry_run(vec![], vec!["symlinks".to_string()], false);
    assert!(
        result.is_ok(),
        "dry-run install with --only symlinks should return Ok: {result:?}"
    );
}

/// Calling `install::run` with `--skip packages` in dry-run mode must return
/// `Ok(())` and skip matching tasks.
#[test]
fn install_run_dry_run_with_skip_filter_returns_ok() {
    let result = common::run_install_dry_run(vec!["packages".to_string()], vec![], false);
    assert!(
        result.is_ok(),
        "dry-run install with --skip packages should return Ok: {result:?}"
    );
}

/// Calling `install::run` with `--only` matching no selector must explain how
/// to discover valid selectors.
#[test]
fn install_run_dry_run_with_only_no_match_returns_an_actionable_error() {
    let result = common::run_install_dry_run(vec![], vec!["zzznomatch".to_string()], false);
    let error = result.expect_err("an unknown selector should fail");
    let message = error.to_string();
    assert!(message.contains("--only did not match a task selector"));
    assert!(message.contains("dotfiles tasks"));
}

/// Calling `install::run` with `--only symlinks` in parallel dry-run mode
/// must return `Ok(())`.
#[test]
fn install_run_dry_run_with_only_filter_parallel_returns_ok() {
    let result = common::run_install_dry_run(vec![], vec!["symlinks".to_string()], true);
    assert!(
        result.is_ok(),
        "parallel dry-run with --only symlinks should return Ok: {result:?}"
    );
}

/// Calling `install::run` with both `--skip` and `--only` simultaneously:
/// a task must satisfy `--only` and must not match `--skip`.
#[test]
fn install_run_dry_run_with_skip_and_only_together() {
    // Matching tasks are still excluded when they also match --skip.
    let result = common::run_install_dry_run(
        vec!["symlinks".to_string()],
        vec!["symlinks".to_string()],
        false,
    );
    assert!(
        result.is_ok(),
        "dry-run with --skip and --only should return Ok: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Parallel execution: should_run with parallel enabled
// ---------------------------------------------------------------------------

/// `should_run` must not panic for any install task when `parallel` is `true`.
///
/// This exercises the scheduler path that dispatches resources to Rayon
/// without needing a real system.
#[test]
fn install_tasks_should_run_with_parallel_enabled() {
    let ctx = common::TestContextBuilder::new().build();
    let ec = ctx.make_system_context(
        "base",
        Platform::detect(),
        tasks::ContextOpts {
            dry_run: true,
            parallel: true,
            is_ci: Some(false),
        },
    );

    let all_tasks = install_tasks();
    for task in &all_tasks {
        let _ = task.should_run(&ec.ctx);
    }
}
