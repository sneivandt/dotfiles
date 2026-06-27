//! Uninstall command implementation.
use anyhow::Result;
use std::sync::Arc;

use crate::cli::{GlobalOpts, UninstallOpts};
use crate::logging::Logger;

/// Run the uninstall command.
///
/// # Errors
///
/// Returns an error if profile resolution, configuration loading, or task execution fails.
pub fn run(
    global: &GlobalOpts,
    _opts: &UninstallOpts,
    log: &Arc<Logger>,
    token: &crate::engine::CancellationToken,
) -> Result<()> {
    super::prepare_self_update(global, log)?;

    let runner = super::CommandRunner::new(global, log, token)?;
    let tasks = crate::tasks::all_uninstall_tasks();
    runner.run(tasks.iter().map(Box::as_ref))
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use std::collections::HashSet;

    use crate::tasks::{self, TaskId};

    #[test]
    fn uninstall_tasks_contains_expected_count() {
        let tasks = tasks::all_uninstall_tasks();
        assert_eq!(tasks.len(), 3);
    }

    #[test]
    fn uninstall_tasks_contain_remove_symlinks() {
        let tasks = tasks::all_uninstall_tasks();
        let names: Vec<&str> = tasks.iter().map(|t| t.name()).collect();
        assert!(
            names.contains(&"Remove symlinks"),
            "expected 'Remove symlinks' in {names:?}"
        );
    }

    #[test]
    fn uninstall_tasks_contain_remove_git_hooks() {
        let tasks = tasks::all_uninstall_tasks();
        let names: Vec<&str> = tasks.iter().map(|t| t.name()).collect();
        assert!(
            names.contains(&"Remove Git hooks"),
            "expected 'Remove Git hooks' in {names:?}"
        );
    }

    #[test]
    fn uninstall_tasks_have_unique_names() {
        let tasks = tasks::all_uninstall_tasks();
        let names: Vec<&str> = tasks.iter().map(|t| t.name()).collect();
        let unique: HashSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), unique.len(), "duplicate task names: {names:?}");
    }

    #[test]
    fn uninstall_tasks_have_unique_type_ids() {
        let tasks = tasks::all_uninstall_tasks();
        let ids: Vec<TaskId> = tasks.iter().map(|t| t.task_id()).collect();
        let unique: HashSet<TaskId> = ids.iter().copied().collect();
        assert_eq!(ids.len(), unique.len(), "duplicate task TaskIds found");
    }

    #[test]
    fn uninstall_tasks_have_resolvable_dependencies() {
        let tasks = tasks::all_uninstall_tasks();
        let present: HashSet<TaskId> = tasks.iter().map(|t| t.task_id()).collect();
        for task in &tasks {
            for dep in task.dependencies() {
                assert!(
                    present.contains(dep),
                    "task '{}' depends on a TaskId not in the uninstall task list",
                    task.name()
                );
            }
        }
    }
}
