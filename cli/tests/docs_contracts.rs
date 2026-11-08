#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "documentation fixture assertions"
)]
//! Contracts for examples and state transitions in the human documentation.

mod common;

use dotfiles_cli::testing as test_api;
use test_api::platform::{Os, Platform};

fn code_blocks(document: &str, language: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in document.lines() {
        if let Some(body) = &mut current {
            if line.trim() == "```" {
                blocks.push(std::mem::take(body));
                current = None;
            } else {
                body.push_str(line);
                body.push('\n');
            }
        } else if line
            .strip_prefix("```")
            .and_then(|info| info.split_whitespace().next())
            == Some(language)
        {
            current = Some(String::new());
        }
    }
    assert!(current.is_none(), "unterminated {language} example");
    assert!(!blocks.is_empty(), "missing {language} examples");
    blocks
}

const LINUX: Platform = Platform {
    os: Os::Linux,
    is_arch: false,
    is_wsl: false,
};

#[cfg(unix)]
#[allow(
    clippy::panic,
    reason = "asserting the documentation fixture result variant"
)]
fn assert_batch(result: test_api::tasks::TaskResult, changed: u32, correct: u32, failed: u32) {
    let test_api::tasks::TaskResult::Batch(stats) = result else {
        panic!("expected batch result, got {result:?}");
    };
    assert_eq!(
        (
            stats.changed_count(),
            stats.already_ok_count(),
            stats.skipped_count(),
            stats.failed_count(),
        ),
        (changed, correct, 0, failed),
        "documentation transition must preserve the expected batch outcomes",
    );
}

#[test]
fn documented_symlink_examples_use_real_sources_and_category_selection() {
    let document = include_str!("../../docs/content/reference/home-symlinks.md");
    for example in code_blocks(document, "toml") {
        let repo = common::TestContextBuilder::new()
            .with_config_file("symlinks.toml", &example)
            .with_symlink_source("config/git/config")
            .with_symlink_source("ssh/config")
            .with_symlink_source("vim/vimrc")
            .with_symlink_source("config/example-app/themes/dark.toml")
            .build();
        let config = repo.load_config_for_platform("base", LINUX);
        assert!(!config.symlinks.is_empty(), "{example}");
        assert!(config.validate(LINUX).is_empty(), "{example}");
        for link in &config.symlinks {
            assert!(
                repo.root_path()
                    .join("symlinks")
                    .join(&link.source)
                    .exists(),
                "{example}: {}",
                link.source
            );
        }
    }
}

#[test]
fn documented_systemd_and_agent_settings_examples_decode_on_both_platforms() {
    for platform in [
        LINUX,
        Platform {
            os: Os::Windows,
            is_arch: false,
            is_wsl: false,
        },
    ] {
        for (document, filename, count) in [
            (
                include_str!("../../docs/content/reference/systemd-units.md"),
                "systemd-units.toml",
                if platform.is_linux() { 3 } else { 0 },
            ),
            (
                include_str!("../../docs/content/reference/agent-settings.md"),
                "agent-settings.toml",
                2,
            ),
        ] {
            for example in code_blocks(document, "toml") {
                let repo = common::TestContextBuilder::new()
                    .with_config_file(filename, &example)
                    .build();
                let config = repo.load_config_for_platform("base", platform);
                let actual = if filename == "systemd-units.toml" {
                    config.units.len()
                } else {
                    config.agent_settings.len()
                };
                assert_eq!(actual, count, "{platform:?}: {example}");
                assert!(config.validate(platform).is_empty(), "{example}");
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn documented_system_file_example_includes_its_required_fragment() {
    let examples = code_blocks(
        include_str!("../../docs/content/reference/system-configuration-files.md"),
        "toml",
    );
    assert_eq!(examples.len(), 2, "declaration and fragment");
    let repo = common::TestContextBuilder::new()
        .with_config_file("system-files.toml", &examples[0])
        .build();
    let source = repo.root_path().join("system/example-app/settings.toml");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, &examples[1]).unwrap();
    let config = repo.load_config_for_platform("base", LINUX);
    assert_eq!(config.system_files.len(), 1);
    assert!(config.validate(LINUX).is_empty());
}

#[test]
fn documented_overlay_declarations_keep_origin_and_replace_exact_targets() {
    let examples = code_blocks(
        include_str!("../../docs/content/guides/private-overlays.md"),
        "toml",
    );
    assert_eq!(
        examples.len(),
        2,
        "Git setting and linked application example"
    );
    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "symlinks.toml",
            "[base]\nsymlinks = ['config/git/config']\n",
        )
        .with_symlink_source("config/git/config")
        .build();
    let overlay = tempfile::tempdir().unwrap();
    std::fs::create_dir(overlay.path().join("conf")).unwrap();
    std::fs::create_dir_all(overlay.path().join("symlinks/config/git")).unwrap();
    std::fs::write(overlay.path().join("conf/git-config.toml"), &examples[0]).unwrap();
    std::fs::write(overlay.path().join("conf/symlinks.toml"), &examples[1]).unwrap();
    std::fs::write(
        overlay.path().join("symlinks/config/git/config"),
        "private fixture",
    )
    .unwrap();
    let profile = test_api::config::profiles::resolve("base", LINUX).unwrap();
    let config =
        test_api::config::Config::load(repo.root_path(), &profile, LINUX, Some(overlay.path()))
            .unwrap();
    assert_eq!(config.symlinks.len(), 1);
    assert_eq!(config.symlinks[0].origin.as_deref(), Some(overlay.path()));
    assert_eq!(config.git_settings.len(), 1);
    assert!(config.validate(LINUX).is_empty());
}

#[cfg(unix)]
#[test]
fn documented_desktop_to_base_transition_preserves_copies_until_moved_aside() {
    use test_api::tasks::Task as _;
    use test_api::tasks::files::symlinks::{InstallSymlinks, UninstallSymlinks};

    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "symlinks.toml",
            "[base]\nsymlinks = ['base']\n[desktop]\nsymlinks = ['desktop']\n",
        )
        .with_symlink_source_content("base", "shared preferences")
        .with_symlink_source_content("desktop", "desktop preferences")
        .build();
    let ec = repo.make_context("desktop");
    let install = InstallSymlinks::new(ec.store.symlinks.clone());
    assert_batch(install.run(&ec.ctx).unwrap(), 2, 0, 0);
    let remove = UninstallSymlinks::new(ec.store.symlinks.clone());
    let base_path = ec.ctx.home().join(".base");
    let desktop_path = ec.ctx.home().join(".desktop");
    assert_batch(remove.run(&ec.ctx.with_dry_run(true)).unwrap(), 2, 0, 0);
    assert!(base_path.symlink_metadata().unwrap().is_symlink());
    assert_batch(remove.run(&ec.ctx).unwrap(), 2, 0, 0);
    assert!(!base_path.symlink_metadata().unwrap().is_symlink());

    let config = repo.load_config_for_platform("base", LINUX);
    let base = InstallSymlinks::new(test_api::config::ConfigHandle::new(config.symlinks));
    assert_batch(base.run(&ec.ctx).unwrap(), 0, 0, 1);
    assert_eq!(
        std::fs::read_to_string(&base_path).unwrap(),
        "shared preferences"
    );
    let backup = ec.ctx.home().join("preserved-base");
    std::fs::rename(&base_path, &backup).unwrap();
    assert_batch(base.run(&ec.ctx.with_dry_run(true)).unwrap(), 1, 0, 0);
    assert!(!base_path.exists(), "preview must not recreate the link");
    for (changed, correct) in [(1, 0), (0, 1)] {
        assert_batch(base.run(&ec.ctx).unwrap(), changed, correct, 0);
        assert!(base_path.symlink_metadata().unwrap().is_symlink());
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            "shared preferences"
        );
        assert_eq!(
            std::fs::read_to_string(&desktop_path).unwrap(),
            "desktop preferences"
        );
    }
}

#[cfg(unix)]
#[test]
fn documented_overlay_switch_preserves_the_old_link_until_moved_aside() {
    use test_api::engine::{ProcessOpts, process_resources};
    use test_api::resources::symlink::SymlinkResource;
    use test_api::resources::{IntrinsicState as _, ResourceState};
    use test_api::tasks::Task as _;
    use test_api::tasks::files::symlinks::InstallSymlinks;

    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "symlinks.toml",
            "[base]\nsymlinks = ['config/git/config']\n",
        )
        .with_symlink_source_content("config/git/config", "base identity")
        .build();
    let ec = repo.make_context("base");
    assert_batch(
        InstallSymlinks::new(ec.store.symlinks.clone())
            .run(&ec.ctx)
            .unwrap(),
        1,
        0,
        0,
    );
    let overlay = tempfile::tempdir().unwrap();
    let source = overlay.path().join("config");
    std::fs::write(&source, "overlay identity").unwrap();
    let target = ec.ctx.home().join(".config/git/config");
    let old_source = std::fs::read_link(&target).unwrap();
    let resource =
        SymlinkResource::new(&source, &target, std::sync::Arc::new(common::StubExecutor))
            .with_display_roots(ec.ctx.home(), overlay.path());
    assert!(matches!(
        resource.current_state().unwrap(),
        ResourceState::Invalid { .. }
    ));
    let options = ProcessOpts::strict("link");
    assert_batch(
        process_resources(&ec.ctx, [resource.clone()], &options).unwrap(),
        0,
        0,
        1,
    );
    assert_eq!(std::fs::read_link(&target).unwrap(), old_source);
    let preserved = ec.ctx.home().join("old-git-link");
    std::fs::rename(&target, &preserved).unwrap();
    assert_eq!(std::fs::read_link(&preserved).unwrap(), old_source);
    assert_eq!(
        std::fs::read_to_string(&preserved).unwrap(),
        "base identity"
    );
    assert_batch(
        process_resources(&ec.ctx.with_dry_run(true), [resource.clone()], &options).unwrap(),
        1,
        0,
        0,
    );
    assert!(!target.exists(), "preview must not create the overlay link");
    assert_batch(
        process_resources(&ec.ctx, [resource.clone()], &options).unwrap(),
        1,
        0,
        0,
    );
    assert_batch(
        process_resources(&ec.ctx, [resource.clone()], &options).unwrap(),
        0,
        1,
        0,
    );
    assert!(matches!(
        resource.current_state().unwrap(),
        ResourceState::Correct
    ));
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "overlay identity"
    );
}

#[test]
fn documented_json_lines_excerpt_matches_real_serialization() {
    let repo = common::TestContextBuilder::new().build();
    let home = tempfile::tempdir().unwrap();
    let output = common::cli_command(
        repo.root_path(),
        home.path(),
        None,
        "check",
        "config-diagnostics",
    )
    .args(["--format", "jsonl"])
    .output()
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let records: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let examples = code_blocks(
        include_str!("../../docs/content/reference/cli-reference.md"),
        "json",
    );
    for line in examples.iter().flat_map(|block| block.lines()) {
        let expected: serde_json::Value = serde_json::from_str(line).unwrap();
        let mut actual = records
            .iter()
            .find(|record| record["type"] == expected["type"])
            .expect("documented record type is emitted")
            .clone();
        actual["run_id"] = serde_json::json!("fixture-run");
        if actual["type"] == "task_result" {
            assert_eq!(actual["context"], actual["task_id"]);
            actual["task_id"] = serde_json::json!("fixture-task");
            actual["context"] = serde_json::json!("fixture-task");
        }
        if actual.get("elapsed_us").is_some() {
            assert!(actual["elapsed_us"].is_u64());
            actual["elapsed_us"] = serde_json::json!(0);
        }
        assert_eq!(actual, expected);
    }
}
