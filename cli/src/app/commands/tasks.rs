//! Read-only task discovery.

use anyhow::{Result, bail};
use serde::Serialize;

use crate::app::cli::{DiscoveryFormat, TasksOpts};
use crate::app::config::Config;
use crate::app::config::store::ConfigStore;
use crate::engine::{Task, TaskVisibility};
use crate::infra::platform::Platform;

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
struct TaskListing {
    selector: String,
    task: String,
    commands: Vec<TaskCommand>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
enum TaskCommand {
    #[serde(rename = "install")]
    Install,
    #[serde(rename = "install --update-pins")]
    InstallUpdatePins,
    #[serde(rename = "uninstall")]
    Uninstall,
    #[serde(rename = "check")]
    Check,
}

impl TaskCommand {
    const fn label(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::InstallUpdatePins => "install --update-pins",
            Self::Uninstall => "uninstall",
            Self::Check => "check",
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

/// List visible task selectors without creating a run log, acquiring the run
/// lock, or persisting profile and overlay selections.
///
/// # Errors
///
/// Returns an error if configuration cannot be loaded, task metadata
/// conflicts, or output cannot be written.
pub fn run(opts: &TasksOpts) -> Result<()> {
    let root = super::runner::resolve_root_path(opts.repository.root.as_deref())?;
    let env = crate::infra::env::system();
    let platform = Platform::detect();
    let overlay = crate::domains::overlay::resolution::resolve_read_only(
        opts.repository.overlay.as_deref(),
        &root,
        env.as_ref(),
    )?;
    let profile = crate::app::config::profiles::resolve_read_only(
        opts.repository.profile.as_deref(),
        &root,
        platform,
        env.as_ref(),
    )?;
    let config = Config::load(&root, &profile, platform, overlay.as_deref())?;
    let store = ConfigStore::from_config(config);
    let listings = collect_listings(&store, overlay.as_deref())?;
    let stdout = std::io::stdout();
    write_listings(&listings, opts.format, &mut stdout.lock())
}

fn collect_listings(
    store: &ConfigStore,
    overlay: Option<&std::path::Path>,
) -> Result<Vec<TaskListing>> {
    let mut listings = Vec::new();

    let install_tasks = crate::app::catalog::all_install_tasks(store);
    add_tasks(&mut listings, &install_tasks, |listing, task| {
        if task.update_only() {
            listing.include(TaskCommand::InstallUpdatePins);
        } else {
            listing.include(TaskCommand::Install);
        }
    })?;

    let overlay_tasks = overlay.map_or_else(Vec::new, |root| {
        crate::domains::overlay::scripts::overlay_script_tasks(&store.scripts.read(), root)
    });
    add_tasks(&mut listings, &overlay_tasks, |listing, _| {
        listing.include(TaskCommand::Install);
    })?;

    let uninstall_tasks = crate::app::catalog::all_uninstall_tasks(store);
    add_tasks(&mut listings, &uninstall_tasks, |listing, _| {
        listing.include(TaskCommand::Uninstall);
    })?;

    let check_tasks = super::check::validation_tasks(store.aggregate.clone());
    add_tasks(&mut listings, &check_tasks, |listing, _| {
        listing.include(TaskCommand::Check);
    })?;

    Ok(listings)
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
            if listing.task != task.name() {
                bail!(
                    "task selector '{}' is shared by '{}' and '{}'",
                    task.selector(),
                    listing.task,
                    task.name()
                );
            }
            membership(listing, task.as_ref());
            continue;
        }

        let mut listing = TaskListing {
            selector: task.selector().to_string(),
            task: task.name().to_string(),
            ..TaskListing::default()
        };
        membership(&mut listing, task.as_ref());
        listings.push(listing);
    }
    Ok(())
}

fn write_listings(
    listings: &[TaskListing],
    format: DiscoveryFormat,
    out: &mut dyn std::io::Write,
) -> Result<()> {
    match format {
        DiscoveryFormat::Table => write_table(listings, out),
        DiscoveryFormat::Plain => {
            for listing in listings {
                writeln!(
                    out,
                    "{}\t{}\t{}",
                    listing.selector,
                    listing.task,
                    command_membership(listing)
                )?;
            }
            Ok(())
        }
        DiscoveryFormat::Json => {
            serde_json::to_writer_pretty(&mut *out, listings)?;
            writeln!(out)?;
            Ok(())
        }
    }
}

fn write_table(listings: &[TaskListing], out: &mut dyn std::io::Write) -> Result<()> {
    let selector_width = listings
        .iter()
        .map(|listing| listing.selector.len())
        .max()
        .unwrap_or(8)
        .max("SELECTOR".len());
    let task_width = listings
        .iter()
        .map(|listing| listing.task.len())
        .max()
        .unwrap_or(4)
        .max("TASK".len());
    writeln!(
        out,
        "{:<selector_width$}  {:<task_width$}  COMMANDS",
        "SELECTOR", "TASK"
    )?;
    for listing in listings {
        writeln!(
            out,
            "{:<selector_width$}  {:<task_width$}  {}",
            listing.selector,
            listing.task,
            command_membership(listing)
        )?;
    }
    Ok(())
}

fn command_membership(listing: &TaskListing) -> String {
    listing
        .commands
        .iter()
        .map(|command| command.label())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Context, TaskMeta, TaskResult};

    struct VisibleTask;

    impl Task for VisibleTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("Visible task").with_selector("visible")
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }
    }

    struct InternalTask;

    impl Task for InternalTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("Internal task")
                .with_selector("internal")
                .with_visibility(TaskVisibility::Internal)
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }
    }

    #[test]
    fn task_membership_merges_by_selector() {
        let tasks: Vec<Box<dyn Task>> = vec![Box::new(VisibleTask)];
        let mut listings = Vec::new();
        add_tasks(&mut listings, &tasks, |listing, _| {
            listing.include(TaskCommand::Install);
        })
        .expect("install membership");
        add_tasks(&mut listings, &tasks, |listing, _| {
            listing.include(TaskCommand::Uninstall);
        })
        .expect("uninstall membership");

        assert_eq!(listings.len(), 1);
        assert_eq!(command_membership(&listings[0]), "install, uninstall");
    }

    #[test]
    fn internal_tasks_are_hidden() {
        let tasks: Vec<Box<dyn Task>> = vec![Box::new(InternalTask)];
        let mut listings = Vec::new();
        add_tasks(&mut listings, &tasks, |listing, _| {
            listing.include(TaskCommand::Install);
        })
        .expect("task discovery");
        assert!(listings.is_empty());
    }

    #[test]
    fn output_formats_are_stable() {
        let listings = vec![TaskListing {
            selector: "visible".to_string(),
            task: "Visible task".to_string(),
            commands: vec![TaskCommand::InstallUpdatePins],
        }];

        let mut table = Vec::new();
        write_listings(&listings, DiscoveryFormat::Table, &mut table).expect("table output");
        assert!(String::from_utf8(table).unwrap().contains("SELECTOR"));

        let mut plain = Vec::new();
        write_listings(&listings, DiscoveryFormat::Plain, &mut plain).expect("plain output");
        assert_eq!(
            String::from_utf8(plain).unwrap(),
            "visible\tVisible task\tinstall --update-pins\n"
        );

        let mut json = Vec::new();
        write_listings(&listings, DiscoveryFormat::Json, &mut json).expect("JSON output");
        let value: serde_json::Value = serde_json::from_slice(&json).expect("valid JSON");
        assert_eq!(value[0]["selector"], "visible");
        assert_eq!(value[0]["commands"][0], "install --update-pins");
    }
}
