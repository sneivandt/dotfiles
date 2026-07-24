//! Task discovery command.

use std::sync::Arc;

use anyhow::{Result, bail};

use crate::app::cli::GlobalOpts;
use crate::engine::{Task, TaskVisibility};
use crate::infra::logging::{Logger, Output};

#[derive(Debug, Default, PartialEq, Eq)]
struct TaskListing {
    selector: String,
    label: String,
    commands: Vec<TaskCommand>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskCommand {
    Install,
    Update,
    Uninstall,
    Test,
}

impl TaskCommand {
    const fn label(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Update => "update",
            Self::Uninstall => "uninstall",
            Self::Test => "test",
        }
    }
}

impl TaskListing {
    fn include(&mut self, command: TaskCommand) {
        if !self.commands.contains(&command) {
            self.commands.push(command);
        }
    }
}

/// List visible task selectors and the commands that use them.
///
/// # Errors
///
/// Returns an error if configuration cannot be loaded or two visible tasks
/// reuse one selector with different labels.
pub fn run(
    global: &GlobalOpts,
    log: &Arc<Logger>,
    token: &crate::engine::CancellationToken,
) -> Result<()> {
    let runner = super::CommandRunner::new(global, log, token)?;
    let mut listings = Vec::new();

    let install_tasks = runner.install_tasks();
    add_tasks(&mut listings, &install_tasks, |listing, task| {
        listing.include(TaskCommand::Update);
        if !task.update_only() {
            listing.include(TaskCommand::Install);
        }
    })?;

    let overlay_tasks = runner.overlay_script_tasks();
    add_tasks(&mut listings, &overlay_tasks, |listing, _| {
        listing.include(TaskCommand::Install);
        listing.include(TaskCommand::Update);
    })?;

    let uninstall_tasks = runner.uninstall_tasks();
    add_tasks(&mut listings, &uninstall_tasks, |listing, _| {
        listing.include(TaskCommand::Uninstall);
    })?;

    let test_tasks = super::test::validation_tasks(runner.config_handle());
    add_tasks(&mut listings, &test_tasks, |listing, _| {
        listing.include(TaskCommand::Test);
    })?;

    display_tasks(&listings, &**log);
    Ok(())
}

fn add_tasks(
    listings: &mut Vec<TaskListing>,
    tasks: &[Box<dyn Task>],
    membership: impl Fn(&mut TaskListing, &dyn Task),
) -> Result<()> {
    for task in tasks {
        if task.visibility() == TaskVisibility::Internal {
            continue;
        }

        if let Some(listing) = listings
            .iter_mut()
            .find(|listing| listing.selector == task.selector())
        {
            if listing.label != task.name() {
                bail!(
                    "task selector '{}' is shared by '{}' and '{}'",
                    task.selector(),
                    listing.label,
                    task.name()
                );
            }
            membership(listing, task.as_ref());
            continue;
        }

        let mut listing = TaskListing {
            selector: task.selector().to_string(),
            label: task.name().to_string(),
            ..TaskListing::default()
        };
        membership(&mut listing, task.as_ref());
        listings.push(listing);
    }
    Ok(())
}

fn display_tasks(listings: &[TaskListing], log: &dyn Output) {
    let selector_width = listings
        .iter()
        .map(|listing| listing.selector.len())
        .max()
        .unwrap_or(8)
        .max("SELECTOR".len());
    let label_width = listings
        .iter()
        .map(|listing| listing.label.len())
        .max()
        .unwrap_or(4)
        .max("TASK".len());

    log.always("");
    log.always(&format!(
        "{:<selector_width$}  {:<label_width$}  COMMANDS",
        "SELECTOR", "TASK"
    ));
    for listing in listings {
        log.always(&format!(
            "{:<selector_width$}  {:<label_width$}  {}",
            listing.selector,
            listing.label,
            command_membership(listing)
        ));
    }
}

fn command_membership(listing: &TaskListing) -> String {
    [
        TaskCommand::Install,
        TaskCommand::Update,
        TaskCommand::Uninstall,
        TaskCommand::Test,
    ]
    .into_iter()
    .filter(|command| listing.commands.contains(command))
    .map(TaskCommand::label)
    .collect::<Vec<_>>()
    .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Context, TaskResult};

    struct VisibleTask;

    impl Task for VisibleTask {
        fn name(&self) -> &'static str {
            "Visible task"
        }

        fn selector(&self) -> &'static str {
            "visible"
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }
    }

    struct InternalTask;

    impl Task for InternalTask {
        fn name(&self) -> &'static str {
            "Internal task"
        }

        fn selector(&self) -> &'static str {
            "internal"
        }

        fn visibility(&self) -> TaskVisibility {
            TaskVisibility::Internal
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }
    }

    struct ConflictingTask;

    impl Task for ConflictingTask {
        fn name(&self) -> &'static str {
            "Conflicting task"
        }

        fn selector(&self) -> &'static str {
            "visible"
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }
    }

    #[test]
    fn add_tasks_merges_command_membership_by_selector() {
        let tasks: Vec<Box<dyn Task>> = vec![Box::new(VisibleTask)];
        let mut listings = Vec::new();

        let install_result = add_tasks(&mut listings, &tasks, |listing, _| {
            listing.include(TaskCommand::Install);
        });
        assert!(install_result.is_ok(), "add install membership");

        let update_result = add_tasks(&mut listings, &tasks, |listing, _| {
            listing.include(TaskCommand::Update);
        });
        assert!(update_result.is_ok(), "add update membership");

        assert_eq!(listings.len(), 1);
        let membership = listings.first().map(command_membership);
        assert_eq!(membership.as_deref(), Some("install, update"));
    }

    #[test]
    fn add_tasks_excludes_internal_tasks() {
        let tasks: Vec<Box<dyn Task>> = vec![Box::new(InternalTask)];
        let mut listings = Vec::new();

        let result = add_tasks(&mut listings, &tasks, |listing, _| {
            listing.include(TaskCommand::Install);
        });

        assert!(result.is_ok(), "add install membership");
        assert!(listings.is_empty());
    }

    #[test]
    fn add_tasks_rejects_conflicting_labels_for_one_selector() {
        let mut listings = Vec::new();
        let initial_result = add_tasks(&mut listings, &[Box::new(VisibleTask)], |listing, _| {
            listing.include(TaskCommand::Install);
        });
        assert!(initial_result.is_ok(), "add visible task");

        let result = add_tasks(&mut listings, &[Box::new(ConflictingTask)], |listing, _| {
            listing.include(TaskCommand::Update);
        });
        let error_message = result.err().map(|error| error.to_string());
        assert!(
            matches!(
                error_message.as_deref(),
                Some(message) if message.contains("task selector 'visible' is shared")
            ),
            "unexpected error: {error_message:?}"
        );
    }
}
