//! Task: generate and install shell completions for the dotfiles CLI.
use anyhow::{Context as _, Result};
use clap::CommandFactory;

use crate::cli::Cli;
use crate::tasks::{
    Context, Domain, Operation, OperationState, Task, TaskPhase, TaskResult, process_operation,
    task_metadata,
};

/// Filename of the generated zsh completion script.
const ZSH_COMPLETION_FILENAME: &str = "_dotfiles";

/// Relative path within the symlinks directory to the zsh completions folder.
const ZSH_COMPLETIONS_SUBDIR: &str = "config/zsh/completions";

#[derive(Debug, Clone, Copy)]
struct ZshCompletionOperation;

#[derive(Debug)]
struct ZshCompletionPlan {
    destination: std::path::PathBuf,
    content: String,
}

impl ZshCompletionOperation {
    fn destination(ctx: &Context) -> std::path::PathBuf {
        ctx.paths()
            .symlinks_dir()
            .join(ZSH_COMPLETIONS_SUBDIR)
            .join(ZSH_COMPLETION_FILENAME)
    }

    fn content() -> Result<String> {
        let mut buf = Vec::new();
        let mut cmd = Cli::command();
        clap_complete::generate(clap_complete::Shell::Zsh, &mut cmd, "dotfiles", &mut buf);
        String::from_utf8(buf).context("generated zsh completion script is not valid UTF-8")
    }
}

impl Operation for ZshCompletionOperation {
    type Plan = ZshCompletionPlan;

    fn current_state(&self, ctx: &Context) -> Result<OperationState<Self::Plan>> {
        let dest = Self::destination(ctx);
        let content = Self::content()?;

        if dest.exists()
            && let Ok(existing) = std::fs::read_to_string(&dest)
            && existing == content
        {
            return Ok(OperationState::Complete);
        }

        Ok(OperationState::needs_run(
            format!("write {}", dest.display()),
            ZshCompletionPlan {
                destination: dest,
                content,
            },
        ))
    }

    fn preview(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        ctx.log
            .dry_run(&format!("write {}", plan.destination.display()));
        Ok(TaskResult::DryRun)
    }

    fn apply(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        crate::fs::write_with_parent(&plan.destination, &plan.content)?;
        ctx.log.info("zsh completions written");
        Ok(TaskResult::Ok)
    }
}

/// Install shell completions for the dotfiles CLI by generating the zsh
/// completion script and writing it into the repo's symlinks directory so it
/// is available through the existing symlink at `~/.config/zsh/completions/`.
///
/// On non-Linux platforms the task is a no-op because the managed zsh
/// completions directory is only present in the Linux symlink layout.
#[derive(Debug)]
pub struct GenerateCompletions;

impl Task for GenerateCompletions {
    task_metadata! {
        name: "Install shell completions",
        phase: TaskPhase::Sync,
        domain: Domain::Shell,
        deps: [crate::tasks::repository::update::UpdateRepository],
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.system().platform().is_linux()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        process_operation(ctx, &ZshCompletionOperation)
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::platform::Os;
    use crate::tasks::Task;
    use crate::tasks::test_helpers::{ContextBuilder, empty_config, make_linux_context};
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
        use crate::tasks::TaskId;
        assert!(GenerateCompletions.dependencies().contains(&TaskId::Type(
            std::any::TypeId::of::<crate::tasks::repository::update::UpdateRepository>()
        )));
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
        drop(GenerateCompletions.run(&ctx).unwrap());
        let mtime1 = std::fs::metadata(completions_dir.join(ZSH_COMPLETION_FILENAME))
            .unwrap()
            .modified()
            .unwrap();

        // Second run should be a no-op (same content → same mtime).
        drop(GenerateCompletions.run(&ctx).unwrap());
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
