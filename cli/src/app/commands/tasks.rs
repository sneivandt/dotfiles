//! Read-only task discovery.

use anyhow::{Result, bail};
use serde::Serialize;
use std::collections::HashSet;

use crate::app::cli::{DiscoveryFormat, TaskGraphCommand, TasksOpts};
use crate::app::config::Config;
use crate::app::config::store::ConfigStore;
use crate::app::filter::{selected_task_ids, task_matches_filter};
use crate::domains::ai::apm::ApmPackageMode;
use crate::domains::repository::update::RepositoryUpdateSignal;
use crate::engine::graph::ResolvedTaskGraph;
use crate::engine::{Task, TaskId, TaskVisibility};
use crate::infra::platform::Platform;

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
struct TaskListing {
    selector: String,
    task: String,
    commands: Vec<TaskCommand>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
struct GraphListing {
    selector: String,
    task: String,
    internal: bool,
    blocking: Vec<String>,
    after: Vec<String>,
    selection: GraphSelection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum GraphSelection {
    Default,
    Requested,
    Dependency,
    Filtered,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
enum TaskCommand {
    #[serde(rename = "install")]
    Install,
    #[serde(rename = "update")]
    Update,
    #[serde(rename = "uninstall")]
    Uninstall,
    #[serde(rename = "check")]
    Check,
}

impl TaskCommand {
    const fn label(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Update => "update",
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

/// List available visible task selectors without predicting runtime
/// applicability, creating a run log, acquiring the run lock, or persisting
/// profile and overlay selections.
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
    let stdout = std::io::stdout();
    if let Some(command) = opts.graph {
        if opts.with_deps
            && !matches!(
                command,
                TaskGraphCommand::Install | TaskGraphCommand::Update
            )
        {
            bail!("--with-deps is only available for install and update graphs");
        }
        let tasks = graph_tasks(&store, overlay.as_deref(), command);
        let listings = collect_graph(&tasks, &opts.only, &opts.skip, opts.with_deps)?;
        write_graph(&listings, opts.format, &mut stdout.lock())
    } else {
        let listings = collect_listings(&store, overlay.as_deref())?;
        write_listings(&listings, opts.format, &mut stdout.lock())
    }
}

fn graph_tasks(
    store: &ConfigStore,
    overlay: Option<&std::path::Path>,
    command: TaskGraphCommand,
) -> Vec<Box<dyn Task>> {
    match command {
        TaskGraphCommand::Install | TaskGraphCommand::Update => {
            let mode = if command == TaskGraphCommand::Update {
                ApmPackageMode::UpdatePins
            } else {
                ApmPackageMode::Install
            };
            let mut tasks = crate::app::catalog::install_tasks_for_run(
                store,
                &RepositoryUpdateSignal::new(),
                mode,
            );
            if command == TaskGraphCommand::Install {
                tasks.retain(|task| !task.update_only());
            }
            if let Some(root) = overlay {
                tasks.extend(crate::domains::overlay::scripts::overlay_script_tasks(
                    &store.scripts.read(),
                    root,
                ));
            }
            tasks
        }
        TaskGraphCommand::Uninstall => crate::app::catalog::all_uninstall_tasks(store),
        TaskGraphCommand::Check => super::check::validation_tasks(store.aggregate.clone()),
    }
}

#[allow(
    clippy::indexing_slicing,
    reason = "resolved graph indices refer to the task slice used to construct the graph"
)]
fn collect_graph(
    tasks: &[Box<dyn Task>],
    only: &[String],
    skip: &[String],
    with_deps: bool,
) -> Result<Vec<GraphListing>> {
    let task_refs = tasks.iter().map(Box::as_ref).collect::<Vec<_>>();
    let graph = ResolvedTaskGraph::resolve(&task_refs)?;
    let selected = selected_task_ids(&task_refs, only, skip, with_deps)?;
    Ok(graph
        .execution_order()
        .map(|index| {
            let task = task_refs[index];
            let mut blocking = Vec::new();
            let mut after = Vec::new();
            for &dependency in graph.dependencies(index) {
                let selector = task_refs[dependency].selector().to_string();
                if graph.blocks_on_failure(index, dependency) {
                    blocking.push(selector);
                } else {
                    after.push(selector);
                }
            }
            GraphListing {
                selector: task.selector().to_string(),
                task: task.name().to_string(),
                internal: task.visibility() == TaskVisibility::Internal,
                blocking,
                after,
                selection: selection_reason(task, &selected, only, skip),
            }
        })
        .collect())
}

fn selection_reason(
    task: &dyn Task,
    selected: &HashSet<TaskId>,
    only: &[String],
    skip: &[String],
) -> GraphSelection {
    if skip.iter().any(|filter| task_matches_filter(task, filter)) {
        GraphSelection::Skipped
    } else if !selected.contains(&task.task_id()) {
        GraphSelection::Filtered
    } else if only.is_empty() {
        GraphSelection::Default
    } else if only.iter().any(|filter| task_matches_filter(task, filter)) {
        GraphSelection::Requested
    } else {
        GraphSelection::Dependency
    }
}

fn write_graph(
    listings: &[GraphListing],
    format: DiscoveryFormat,
    out: &mut dyn std::io::Write,
) -> Result<()> {
    if format == DiscoveryFormat::Json {
        serde_json::to_writer_pretty(&mut *out, listings)?;
        writeln!(out)?;
        return Ok(());
    }
    if format == DiscoveryFormat::Table {
        writeln!(out, "SELECTOR\tTASK\tBLOCKING\tAFTER\tINTERNAL\tSELECTION")?;
    }
    for listing in listings {
        writeln!(
            out,
            "{}\t{}\t{}\t{}\t{}\t{}",
            listing.selector,
            listing.task,
            listing.blocking.join(","),
            listing.after.join(","),
            listing.internal,
            match listing.selection {
                GraphSelection::Default => "default",
                GraphSelection::Requested => "requested",
                GraphSelection::Dependency => "dependency",
                GraphSelection::Filtered => "filtered",
                GraphSelection::Skipped => "skipped",
            }
        )?;
    }
    Ok(())
}

fn collect_listings(
    store: &ConfigStore,
    overlay: Option<&std::path::Path>,
) -> Result<Vec<TaskListing>> {
    let mut listings = Vec::new();

    let install_tasks = crate::app::catalog::all_install_tasks(store);
    add_tasks(&mut listings, &install_tasks, |listing, task| {
        if !task.update_only() {
            listing.include(TaskCommand::Install);
        }
        listing.include(TaskCommand::Update);
    })?;

    let update_only_tasks = crate::app::catalog::update_only_install_tasks(store);
    add_tasks(&mut listings, &update_only_tasks, |listing, _| {
        listing.include(TaskCommand::Update);
    })?;

    let overlay_tasks = overlay.map_or_else(Vec::new, |root| {
        crate::domains::overlay::scripts::overlay_script_tasks(&store.scripts.read(), root)
    });
    add_tasks(&mut listings, &overlay_tasks, |listing, _| {
        listing.include(TaskCommand::Install);
        listing.include(TaskCommand::Update);
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
    use crate::engine::{Context, TaskId, TaskMeta, TaskResult};
    use crate::test_helpers::empty_config;
    use std::path::PathBuf;

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

    struct DependentTask;

    impl Task for DependentTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("Dependent task").with_selector("dependent")
        }

        fn dependencies(&self) -> &[TaskId] {
            const DEPS: &[TaskId] = &[TaskId::Type(std::any::TypeId::of::<VisibleTask>())];
            DEPS
        }

        fn ordering_dependencies(&self) -> &[TaskId] {
            const DEPS: &[TaskId] = &[TaskId::Type(std::any::TypeId::of::<InternalTask>())];
            DEPS
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
    fn apm_lists_install_and_update_membership() {
        let store = ConfigStore::from_config(empty_config(PathBuf::from("/tmp")));
        let listings = collect_listings(&store, None).expect("collect task listings");
        let apm = listings
            .iter()
            .find(|listing| listing.selector == "apm")
            .expect("APM task listing");

        assert_eq!(command_membership(apm), "install, update");
        let symlinks = listings
            .iter()
            .find(|listing| listing.selector == "symlinks")
            .expect("symlink task listing");
        assert_eq!(command_membership(symlinks), "install, update, uninstall");
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
            commands: vec![TaskCommand::Update],
        }];

        let mut table = Vec::new();
        write_listings(&listings, DiscoveryFormat::Table, &mut table).expect("table output");
        assert!(String::from_utf8(table).unwrap().contains("SELECTOR"));

        let mut plain = Vec::new();
        write_listings(&listings, DiscoveryFormat::Plain, &mut plain).expect("plain output");
        assert_eq!(
            String::from_utf8(plain).unwrap(),
            "visible\tVisible task\tupdate\n"
        );

        let mut json = Vec::new();
        write_listings(&listings, DiscoveryFormat::Json, &mut json).expect("JSON output");
        let value: serde_json::Value = serde_json::from_slice(&json).expect("valid JSON");
        assert_eq!(value[0]["selector"], "visible");
        assert_eq!(value[0]["commands"][0], "update");
    }

    #[test]
    fn graph_shows_blocking_and_ordering_edges_in_dependency_order() {
        let tasks: Vec<Box<dyn Task>> = vec![
            Box::new(DependentTask),
            Box::new(InternalTask),
            Box::new(VisibleTask),
        ];
        let graph =
            collect_graph(&tasks, &["dependent".into()], &[], true).expect("valid task graph");
        let dependent = graph.last().expect("dependent follows its predecessors");
        assert_eq!(dependent.selector, "dependent");
        assert_eq!(dependent.blocking, ["visible"]);
        assert_eq!(dependent.after, ["internal"]);
        assert_eq!(dependent.selection, GraphSelection::Requested);
        assert!(
            graph
                .iter()
                .any(|task| task.selector == "internal" && task.internal)
        );
        assert!(graph.iter().all(|task| {
            task.selector == "dependent" || task.selection == GraphSelection::Dependency
        }));

        let mut output = Vec::new();
        write_graph(&graph, DiscoveryFormat::Json, &mut output).expect("graph JSON");
        let json: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
        assert_eq!(json[2]["blocking"][0], "visible");
    }
}
