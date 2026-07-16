//! Uninstall command implementation.
use anyhow::Result;
use std::sync::Arc;

use crate::app::cli::{GlobalOpts, UninstallOpts};
use crate::infra::logging::Logger;

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
    let tasks = runner.uninstall_tasks();
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

    use crate::app::config::store::ConfigStore;
    use crate::engine::TaskId;
    use crate::test_helpers::empty_config;

    fn store() -> ConfigStore {
        ConfigStore::from_config(empty_config(std::path::PathBuf::from("/tmp")))
    }

    #[test]
    fn uninstall_tasks_contains_expected_count() {
        let tasks = crate::app::catalog::all_uninstall_tasks(&store());
        assert_eq!(tasks.len(), 3);
    }

    #[test]
    fn uninstall_tasks_contain_materialize_symlinks() {
        let tasks = crate::app::catalog::all_uninstall_tasks(&store());
        let names: Vec<&str> = tasks.iter().map(|t| t.name()).collect();
        assert!(
            names.contains(&"Materialize symlinks"),
            "expected 'Materialize symlinks' in {names:?}"
        );
    }

    #[test]
    fn uninstall_tasks_contain_remove_git_hooks() {
        let tasks = crate::app::catalog::all_uninstall_tasks(&store());
        let names: Vec<&str> = tasks.iter().map(|t| t.name()).collect();
        assert!(
            names.contains(&"Remove Git hooks"),
            "expected 'Remove Git hooks' in {names:?}"
        );
    }

    #[test]
    fn uninstall_tasks_have_unique_names() {
        let tasks = crate::app::catalog::all_uninstall_tasks(&store());
        let names: Vec<&str> = tasks.iter().map(|t| t.name()).collect();
        let unique: HashSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), unique.len(), "duplicate task names: {names:?}");
    }

    #[test]
    fn uninstall_tasks_have_unique_type_ids() {
        let tasks = crate::app::catalog::all_uninstall_tasks(&store());
        let ids: Vec<TaskId> = tasks.iter().map(|t| t.task_id()).collect();
        let unique: HashSet<TaskId> = ids.iter().copied().collect();
        assert_eq!(ids.len(), unique.len(), "duplicate task TaskIds found");
    }

    #[test]
    fn uninstall_tasks_have_resolvable_dependencies() {
        let tasks = crate::app::catalog::all_uninstall_tasks(&store());
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
