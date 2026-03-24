//! Task: generate and install shell completions for the dotfiles CLI.
use anyhow::{Context as _, Result};
use clap::CommandFactory;

use crate::cli::Cli;
use crate::phases::{Context, Task, TaskPhase, TaskResult, task_deps};

/// Filename of the generated zsh completion script.
const ZSH_COMPLETION_FILENAME: &str = "_dotfiles";

/// Relative path within the symlinks directory to the zsh completions folder.
const ZSH_COMPLETIONS_SUBDIR: &str = "config/zsh/completions";

/// Generate shell completions for the dotfiles CLI and write them into the
/// repo's symlinks directory so they are available through the existing
/// symlink at `~/.config/zsh/completions/`.
///
/// On non-Linux platforms the task is a no-op because the managed zsh
/// completions directory is only present in the Linux symlink layout.
#[derive(Debug)]
pub struct GenerateCompletions;

impl Task for GenerateCompletions {
    fn name(&self) -> &'static str {
        "Generate shell completions"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Repository
    }

    task_deps![crate::phases::repository::update::UpdateRepository];

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.is_linux()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let dest = ctx
            .symlinks_dir()
            .join(ZSH_COMPLETIONS_SUBDIR)
            .join(ZSH_COMPLETION_FILENAME);

        // Generate the completion content into an in-memory buffer.
        let mut buf = Vec::new();
        let mut cmd = Cli::command();
        clap_complete::generate(clap_complete::Shell::Zsh, &mut cmd, "dotfiles", &mut buf);
        let content =
            String::from_utf8(buf).context("generated zsh completion script is not valid UTF-8")?;

        // Check whether the file is already up to date (idempotency).
        if dest.exists()
            && let Ok(existing) = std::fs::read_to_string(&dest)
            && existing == content
        {
            ctx.log.debug("zsh completions already up to date");
            return Ok(TaskResult::Ok);
        }

        if ctx.dry_run {
            ctx.log.dry_run(&format!("write {}", dest.display()));
            return Ok(TaskResult::DryRun);
        }

        // Ensure the parent directory exists before writing.
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating directory {}", parent.display()))?;
        }

        std::fs::write(&dest, content).with_context(|| format!("writing {}", dest.display()))?;

        ctx.log.info("zsh completions written");
        Ok(TaskResult::Ok)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::phases::Task;
    use crate::phases::test_helpers::{ContextBuilder, empty_config, make_linux_context};
    use crate::platform::Os;
    use std::path::PathBuf;

    // ------------------------------------------------------------------
    // should_run
    // ------------------------------------------------------------------

    #[test]
    fn should_run_false_on_windows() {
        let config = empty_config(PathBuf::from("/repo"));
        let ctx = ContextBuilder::new(config).os(Os::Windows).build();
        assert!(!GenerateCompletions.should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_linux() {
        let config = empty_config(PathBuf::from("/repo"));
        let ctx = make_linux_context(config);
        assert!(GenerateCompletions.should_run(&ctx));
    }

    // ------------------------------------------------------------------
    // dependencies
    // ------------------------------------------------------------------

    #[test]
    fn depends_on_update_repository() {
        use std::any::TypeId;
        assert!(GenerateCompletions.dependencies().contains(&TypeId::of::<
            crate::phases::repository::update::UpdateRepository,
        >()));
    }

    // ------------------------------------------------------------------
    // run — real filesystem
    // ------------------------------------------------------------------

    #[test]
    fn run_writes_completion_file() {
        let dir = tempfile::tempdir().unwrap();
        let completions_dir = dir.path().join("symlinks").join(ZSH_COMPLETIONS_SUBDIR);
        std::fs::create_dir_all(&completions_dir).unwrap();

        let config = empty_config(dir.path().to_path_buf());
        let ctx = make_linux_context(config);

        let result = GenerateCompletions.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Ok));

        let dest = completions_dir.join(ZSH_COMPLETION_FILENAME);
        assert!(dest.exists(), "completion file should be written");

        let content = std::fs::read_to_string(&dest).unwrap();
        assert!(
            content.contains("dotfiles"),
            "generated script should reference the binary name"
        );
    }

    #[test]
    fn run_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let completions_dir = dir.path().join("symlinks").join(ZSH_COMPLETIONS_SUBDIR);
        std::fs::create_dir_all(&completions_dir).unwrap();

        let config = empty_config(dir.path().to_path_buf());
        let ctx = make_linux_context(config);

        // First run writes the file.
        let _ = GenerateCompletions.run(&ctx).unwrap();
        let mtime1 = std::fs::metadata(completions_dir.join(ZSH_COMPLETION_FILENAME))
            .unwrap()
            .modified()
            .unwrap();

        // Second run should be a no-op (same content → same mtime).
        let _ = GenerateCompletions.run(&ctx).unwrap();
        let mtime2 = std::fs::metadata(completions_dir.join(ZSH_COMPLETION_FILENAME))
            .unwrap()
            .modified()
            .unwrap();

        assert_eq!(mtime1, mtime2, "second run should not modify the file");
    }

    #[test]
    fn run_creates_parent_directory_if_missing() {
        let dir = tempfile::tempdir().unwrap();
        // Do NOT pre-create the completions directory.
        let config = empty_config(dir.path().to_path_buf());
        let ctx = make_linux_context(config);

        let result = GenerateCompletions.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Ok));

        let dest = dir
            .path()
            .join("symlinks")
            .join(ZSH_COMPLETIONS_SUBDIR)
            .join(ZSH_COMPLETION_FILENAME);
        assert!(dest.exists(), "completion file should be created");
    }

    #[test]
    fn run_dry_run_returns_dry_run_result_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let config = empty_config(dir.path().to_path_buf());
        let ctx = make_linux_context(config).with_dry_run(true);

        let result = GenerateCompletions.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::DryRun));

        let dest = dir
            .path()
            .join("symlinks")
            .join(ZSH_COMPLETIONS_SUBDIR)
            .join(ZSH_COMPLETION_FILENAME);
        assert!(!dest.exists(), "dry-run should not write the file");
    }
}
