//! Task: generate and install shell completions for the dotfiles CLI.
use anyhow::{Context as _, Result};

use crate::engine::{
    Context, Operation, OperationState, Task, TaskMeta, TaskResult, TaskStats, process_operation,
};

/// Filename of the generated zsh completion script.
const ZSH_COMPLETION_FILENAME: &str = "_dotfiles";

/// Relative path within the symlinks directory to the zsh completions folder.
const ZSH_COMPLETIONS_SUBDIR: &str = "config/zsh/completions";

/// Filename of the generated PowerShell completion script.
const POWERSHELL_COMPLETION_FILENAME: &str = "dotfiles-completions.ps1";

struct CompletionOperation<'a> {
    content: &'a str,
    destination: std::path::PathBuf,
    display_path: String,
}

#[derive(PartialEq, Eq)]
struct CompletionSnapshot {
    path: std::path::PathBuf,
    content: Option<String>,
}

impl CompletionOperation<'_> {
    fn report_write(&self, ctx: &Context, planned: bool) {
        ctx.log().action(
            "write",
            &self.display_path,
            planned,
            &format!("write {}", self.display_path),
        );
    }

    fn snapshot(&self) -> Result<CompletionSnapshot> {
        let path = crate::infra::fs::resolve_target_entry(&self.destination)?;
        let metadata =
            crate::infra::fs::symlink_metadata_optional(&path, "inspect completion destination")?;
        let content = if let Some(metadata) = metadata {
            anyhow::ensure!(
                !metadata.is_symlink(),
                "completion destination is a symlink: {}",
                path.display()
            );
            anyhow::ensure!(
                metadata.is_file(),
                "not a regular completion file: {}",
                path.display()
            );
            Some(
                std::fs::read_to_string(&path)
                    .with_context(|| format!("read completions: {}", path.display()))?,
            )
        } else {
            None
        };
        Ok(CompletionSnapshot { path, content })
    }
}

impl Operation for CompletionOperation<'_> {
    type Plan = CompletionSnapshot;

    fn current_state(&self, _ctx: &Context) -> Result<OperationState<Self::Plan>> {
        let before = self.snapshot()?;
        if before.content.as_deref() == Some(self.content) {
            return Ok(OperationState::Complete);
        }
        Ok(OperationState::needs_run(before))
    }

    fn preview(&self, ctx: &Context, _plan: &Self::Plan) -> Result<TaskResult> {
        self.report_write(ctx, true);
        Ok(TaskStats::changed().finish())
    }

    fn apply(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        crate::infra::fs::write_atomic_checked(&plan.path, self.content, || {
            anyhow::ensure!(
                self.snapshot()? == *plan,
                "completion destination {} changed during update; save any edits elsewhere before retrying",
                self.destination.display()
            );
            Ok(())
        })?;
        self.report_write(ctx, false);
        Ok(TaskStats::changed().finish())
    }
}

/// Generate and install the CLI completion script for the current platform.
///
/// The application injects the generators because it owns the CLI definition.
/// Generate the selected shell's content only when the task runs.
///
#[derive(Debug)]
pub(crate) struct GenerateCompletions {
    generate_zsh: fn() -> String,
    generate_powershell: fn() -> String,
}

impl GenerateCompletions {
    /// Create the task with the shell completion generators.
    #[must_use]
    pub(crate) const fn new(
        generate_zsh: fn() -> String,
        generate_powershell: fn() -> String,
    ) -> Self {
        Self {
            generate_zsh,
            generate_powershell,
        }
    }
}

impl Task for GenerateCompletions {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new("Shell completions").with_selector("completions")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let (content, destination) = if ctx.platform().is_windows() {
            (
                (self.generate_powershell)(),
                ctx.home()
                    .join(".config")
                    .join("powershell")
                    .join("profile.d")
                    .join(POWERSHELL_COMPLETION_FILENAME),
            )
        } else {
            (
                (self.generate_zsh)(),
                ctx.symlinks_dir()
                    .join(ZSH_COMPLETIONS_SUBDIR)
                    .join(ZSH_COMPLETION_FILENAME),
            )
        };
        let display_path = crate::infra::logging::abbreviate_path(
            &destination,
            Some(ctx.home()),
            Some(ctx.root()),
        );
        process_operation(
            ctx,
            &CompletionOperation {
                content: &content,
                destination,
                display_path,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Task;
    use crate::infra::platform::Os;
    use crate::test_helpers::{ContextBuilder, make_linux_context};
    use std::path::PathBuf;

    const SAMPLE_ZSH_COMPLETION: &str = "#compdef dotfiles\n# generated completion for dotfiles\n";
    const SAMPLE_POWERSHELL_COMPLETION: &str =
        "Register-ArgumentCompleter -Native -CommandName dotfiles\n";

    fn task() -> GenerateCompletions {
        GenerateCompletions::new(
            || SAMPLE_ZSH_COMPLETION.to_string(),
            || SAMPLE_POWERSHELL_COMPLETION.to_string(),
        )
    }

    // ------------------------------------------------------------------
    // should_run
    // ------------------------------------------------------------------

    #[test]
    fn should_run_true_on_windows() {
        let ctx = ContextBuilder::new(PathBuf::from("/repo"))
            .overlay(None)
            .os(Os::Windows)
            .build();
        assert!(task().should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_linux() {
        let ctx = make_linux_context(PathBuf::from("/repo"), None);
        assert!(task().should_run(&ctx));
    }

    // ------------------------------------------------------------------
    // run — real filesystem
    // ------------------------------------------------------------------

    fn completion_fixture(os: Os) -> (tempfile::TempDir, Context, PathBuf, &'static str) {
        let dir = tempfile::tempdir_in(".").unwrap();
        let root = dir.path().join("repo");
        let home = dir.path().join("home");
        let ctx = ContextBuilder::new(root.clone())
            .overlay(None)
            .os(os)
            .build()
            .with_home(home.clone());
        let (destination, content) = if os == Os::Windows {
            (
                home.join(".config")
                    .join("powershell")
                    .join("profile.d")
                    .join(POWERSHELL_COMPLETION_FILENAME),
                SAMPLE_POWERSHELL_COMPLETION,
            )
        } else {
            (
                root.join("symlinks")
                    .join(ZSH_COMPLETIONS_SUBDIR)
                    .join(ZSH_COMPLETION_FILENAME),
                SAMPLE_ZSH_COMPLETION,
            )
        };
        (dir, ctx, destination, content)
    }

    #[test]
    fn completions_create_repair_and_converge_without_rewriting_current_content() {
        for os in [Os::Linux, Os::Windows] {
            for existing_parent in [false, true] {
                let (_dir, ctx, destination, expected) = completion_fixture(os);
                if existing_parent {
                    std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
                }
                crate::test_helpers::assert_task_changed(&task().run(&ctx).unwrap());
                assert_eq!(
                    std::fs::read_to_string(&destination).unwrap(),
                    expected,
                    "{os:?}"
                );

                std::fs::write(&destination, "stale completions").unwrap();
                crate::test_helpers::assert_task_changed(&task().run(&ctx).unwrap());
                assert_eq!(
                    std::fs::read_to_string(&destination).unwrap(),
                    expected,
                    "{os:?}"
                );

                let past = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
                std::fs::File::options()
                    .write(true)
                    .open(&destination)
                    .unwrap()
                    .set_times(std::fs::FileTimes::new().set_modified(past))
                    .unwrap();
                let before = std::fs::metadata(&destination).unwrap().modified().unwrap();
                crate::test_helpers::assert_task_ok(&task().run(&ctx).unwrap());
                assert_eq!(
                    std::fs::metadata(&destination).unwrap().modified().unwrap(),
                    before,
                    "{os:?}: current content must not be rewritten"
                );
            }
        }
    }

    #[test]
    fn only_the_selected_shell_generator_runs() {
        fn unexpected() -> String {
            panic!("an unselected shell does not need a completion script")
        }

        for (os, task) in [
            (
                Os::Linux,
                GenerateCompletions::new(|| SAMPLE_ZSH_COMPLETION.to_owned(), unexpected),
            ),
            (
                Os::Windows,
                GenerateCompletions::new(unexpected, || SAMPLE_POWERSHELL_COMPLETION.to_owned()),
            ),
        ] {
            let (_dir, ctx, destination, expected) = completion_fixture(os);
            crate::test_helpers::assert_task_changed(&task.run(&ctx).unwrap());
            assert_eq!(std::fs::read_to_string(destination).unwrap(), expected);
        }
    }

    #[test]
    fn stale_completion_plan_preserves_later_edits_and_rechecks_on_retry() {
        for existing in [false, true] {
            let (_dir, ctx, destination, expected) = completion_fixture(Os::Linux);
            let parent = destination.parent().unwrap();
            std::fs::create_dir_all(parent).unwrap();
            let neighbor = parent.join("user-completion");
            std::fs::write(&neighbor, "untouched").unwrap();
            if existing {
                std::fs::write(&destination, "outdated").unwrap();
            }
            let op = CompletionOperation {
                content: expected,
                destination: destination.clone(),
                display_path: crate::infra::logging::abbreviate_path(
                    &destination,
                    Some(ctx.home()),
                    Some(ctx.root()),
                ),
            };
            let OperationState::NeedsRun { plan } = op.current_state(&ctx).unwrap() else {
                panic!("expected a completion update");
            };
            std::fs::write(&destination, "editor save").unwrap();
            let error = op.apply(&ctx, &plan).unwrap_err();
            assert!(error.to_string().contains("changed"), "{error:#}");
            assert_eq!(
                std::fs::read_to_string(&destination).unwrap(),
                "editor save"
            );
            assert_eq!(std::fs::read_dir(parent).unwrap().count(), 2);
            crate::test_helpers::assert_task_changed(&task().run(&ctx).unwrap());
            assert_eq!(std::fs::read_to_string(&destination).unwrap(), expected);
            assert_eq!(std::fs::read_to_string(&neighbor).unwrap(), "untouched");
        }
    }

    #[test]
    fn dry_run_preserves_missing_and_stale_completions() {
        for os in [Os::Linux, Os::Windows] {
            let (_dir, ctx, destination, _) = completion_fixture(os);
            let ctx = ctx.with_dry_run(true);
            crate::test_helpers::assert_task_changed(&task().run(&ctx).unwrap());
            assert!(
                !destination.parent().unwrap().exists(),
                "{os:?}: preview created directories"
            );

            std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
            std::fs::write(&destination, "stale completions").unwrap();
            crate::test_helpers::assert_task_changed(&task().run(&ctx).unwrap());
            assert_eq!(
                std::fs::read_to_string(&destination).unwrap(),
                "stale completions",
                "{os:?}"
            );
        }
    }

    #[test]
    fn completion_actions_identify_the_verb_subject_and_preview_mode() {
        use crate::test_helpers::CapturingOutput;
        use std::sync::Arc;

        for os in [Os::Linux, Os::Windows] {
            for dry_run in [false, true] {
                let (_dir, ctx, _destination, _) = completion_fixture(os);
                let expected_subject = if os == Os::Windows {
                    "~/.config/powershell/profile.d/dotfiles-completions.ps1"
                } else {
                    "symlinks/config/zsh/completions/_dotfiles"
                };
                let output = Arc::new(CapturingOutput::default());
                let ctx = ctx
                    .with_dry_run(dry_run)
                    .with_log(Arc::<CapturingOutput>::clone(&output));
                crate::test_helpers::assert_task_changed(&task().run(&ctx).unwrap());

                let actions = output.actions();
                assert_eq!(actions.len(), 1);
                let action = &actions[0];
                assert_eq!(action.verb, "write");
                assert_eq!(action.planned, dry_run);
                assert_eq!(action.subject, expected_subject);
                assert_eq!(action.message, format!("write {expected_subject}"));
            }
        }
    }

    #[test]
    fn completion_read_errors_preserve_existing_bytes_even_in_preview() {
        for os in [Os::Linux, Os::Windows] {
            let (_dir, ctx, destination, _) = completion_fixture(os);
            std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
            std::fs::write(&destination, [0xff, 0xfe]).unwrap();
            for dry_run in [true, false] {
                let error = task().run(&ctx.with_dry_run(dry_run)).unwrap_err();
                assert!(error.to_string().contains("read completions"), "{error:#}");
                assert_eq!(std::fs::read(&destination).unwrap(), [0xff, 0xfe]);
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn completion_symlinks_are_rejected_without_changing_the_referent() {
        let (_dir, ctx, destination, expected) = completion_fixture(Os::Linux);
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        let referent = std::path::absolute(destination.with_extension("original")).unwrap();
        for content in [None, Some("preserve"), Some(expected)] {
            if let Some(content) = content {
                std::fs::write(&referent, content).unwrap();
            }
            std::os::unix::fs::symlink(&referent, &destination).unwrap();
            let error = task().run(&ctx).unwrap_err();
            assert!(error.to_string().contains("is a symlink"), "{error:#}");
            assert_eq!(std::fs::read_link(&destination).unwrap(), referent);
            if let Some(content) = content {
                assert_eq!(std::fs::read_to_string(&referent).unwrap(), content);
            } else {
                assert!(!referent.exists());
            }
            std::fs::remove_file(&destination).unwrap();
        }
    }

    #[test]
    fn blocked_completion_destination_fails_without_removing_existing_files() {
        for os in [Os::Linux, Os::Windows] {
            let (_dir, ctx, destination, _) = completion_fixture(os);
            std::fs::create_dir_all(&destination).unwrap();
            let unmanaged = destination.join("keep.txt");
            std::fs::write(&unmanaged, "preserve").unwrap();

            assert!(
                task().run(&ctx).is_err(),
                "{os:?}: directory destination must fail"
            );
            assert_eq!(
                std::fs::read_to_string(unmanaged).unwrap(),
                "preserve",
                "{os:?}"
            );
        }
    }
}
