//! Read-only task discovery.

use anyhow::{Result, bail};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};

use crate::app::cli::{DiscoveryFormat, ListOpts, TaskGraphCommand};
use crate::app::config::Config;
use crate::app::config::store::ConfigStore;
use crate::app::filter::{resolve_filters, selected_task_ids};
use crate::domains::ai::apm::ApmPackageMode;
use crate::domains::files::chmod::ApplyFilePermissions;
use crate::domains::repository::update::RepositoryUpdateSignal;
use crate::domains::system::{system_files::ConfigureSystemFiles, systemd_units::ConfigureSystemd};
use crate::engine::graph::ResolvedTaskGraph;
use crate::engine::{Task, TaskId, TaskVisibility};
use crate::infra::logging::{StyleChoice, TextStyle, stdout_style};
use crate::infra::platform::Platform;

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct TaskListing {
    pub(crate) selector: String,
    pub(crate) task: String,
    pub(crate) commands: Vec<TaskGraphCommand>,
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

impl TaskListing {
    fn include(&mut self, command: TaskGraphCommand) {
        if let Err(index) = self.commands.binary_search(&command) {
            self.commands.insert(index, command);
        }
    }
}

/// List visible task selectors, omitting clearly inapplicable tasks by default.
/// Discovery never executes tasks, probes runtime prerequisites, creates a run
/// log, acquires the run lock, or persists profile and overlay selections.
///
/// # Errors
///
/// Returns an error if configuration cannot be loaded, task metadata
/// conflicts, or output cannot be written.
pub(crate) fn run(opts: &ListOpts) -> Result<()> {
    let env = crate::infra::env::system();
    let platform = Platform::detect();
    let config = load_discovery_config(&opts.repository, env.as_ref(), platform)?;
    let overlay = config.overlay.clone();
    let store = ConfigStore::from_config(config);
    let stdout = std::io::stdout();
    let style = stdout_style();
    if let Some(command) = opts.graph {
        if opts.with_deps && !matches!(command, TaskGraphCommand::Update) {
            bail!("--with-deps is only available for update graphs");
        }
        let tasks = command_tasks(&store, overlay.as_deref(), command);
        let listings = collect_graph(&tasks, &opts.only, &opts.skip, opts.with_deps)?;
        write_graph(&listings, opts.format, style, &mut stdout.lock())
    } else {
        let mut listings = collect_listings(&store, overlay.as_deref())?;
        if !opts.all {
            let is_ci = env.var_os("CI").is_some();
            listings.retain(|listing| {
                potentially_applicable(&listing.selector, store.aggregate.get(), platform, is_ci)
            });
        }
        write_listings(&listings, opts.format, style, &mut stdout.lock())
    }
}

pub(crate) fn discover_all_listings(
    repository: &crate::app::cli::RepositoryOpts,
) -> Result<Vec<TaskListing>> {
    let env = crate::infra::env::system();
    let platform = Platform::detect();
    let root = super::runner::resolve_root_path(repository.root.as_deref())?;
    let overlay = crate::domains::overlay::resolution::resolve_from_args(
        repository.overlay.as_deref(),
        &root,
        env.as_ref(),
        true,
    )?
    .map(|(path, _)| path);
    let profile = crate::app::config::profiles::resolve_read_only(
        repository.profile.as_deref(),
        &root,
        platform,
        env.as_ref(),
    )?;
    let config = Config::load(&root, &profile, platform, overlay.as_deref())?;
    let resolved_overlay = config.overlay.clone();
    let store = ConfigStore::from_config(config);
    collect_listings(&store, resolved_overlay.as_deref())
}

pub(super) fn load_discovery_config(
    repository: &crate::app::cli::RepositoryOpts,
    env: &dyn crate::infra::env::Env,
    platform: Platform,
) -> Result<Config> {
    let root = super::runner::resolve_root_path(repository.root.as_deref())?;
    let overlay = crate::domains::overlay::resolution::resolve_read_only(
        repository.overlay.as_deref(),
        &root,
        env,
    )?;
    let profile = crate::app::config::profiles::resolve_read_only(
        repository.profile.as_deref(),
        &root,
        platform,
        env,
    )?;
    Config::load(&root, &profile, platform, overlay.as_deref())
}

/// Only reject tasks whose platform or immutable configuration proves there
/// is no work. Keep unknown tasks and filesystem/runtime-dependent cases.
fn potentially_applicable(
    selector: &str,
    config: &Config,
    platform: Platform,
    is_ci: bool,
) -> bool {
    match selector {
        "developer-mode" => platform.is_windows(),
        "registry" => platform.has_registry() && !config.registry.is_empty(),
        "file-permissions" => {
            ApplyFilePermissions::is_applicable(platform, !config.chmod.is_empty())
        }
        "packages" => config.packages.iter().any(|package| !package.is_aur),
        "aur-packages" => {
            platform.supports_aur() && config.packages.iter().any(|package| package.is_aur)
        }
        "paru" => platform.uses_pacman(),
        "custom-package-repository" | "custom-package-versions" => platform.supports_aur(),
        "system-files" => {
            ConfigureSystemFiles::is_applicable(platform, !config.system_files.is_empty(), is_ci)
        }
        "systemd" => ConfigureSystemd::is_applicable(platform, !config.units.is_empty(), is_ci),
        "login-shell" => platform.is_linux(),
        "vscode-extensions" => !config.vscode_extensions.is_empty(),
        "agent-settings" => !config.agent_settings.is_empty(),
        "symlinks" => !config.symlinks.is_empty(),
        "git" => !config.git_settings.is_empty(),
        "symlink-sources" => {
            !config.validation_symlinks.is_empty() || !config.validation_chmod.is_empty()
        }
        _ => true,
    }
}

fn command_tasks(
    store: &ConfigStore,
    overlay: Option<&std::path::Path>,
    command: TaskGraphCommand,
) -> Vec<Box<dyn Task>> {
    match command {
        TaskGraphCommand::Update => {
            let mode = ApmPackageMode::UpdatePins;
            let mut tasks = crate::app::catalog::update_tasks_for_run(
                store,
                &RepositoryUpdateSignal::new(),
                mode,
            );
            if let Some(root) = overlay {
                tasks.extend(crate::domains::overlay::scripts::overlay_script_tasks(
                    store.scripts.get(),
                    root,
                ));
            }
            tasks
        }
        TaskGraphCommand::Remove => {
            let mut tasks = crate::app::catalog::all_remove_tasks(store, false);
            if let Some(root) = overlay {
                tasks.extend(
                    crate::domains::overlay::scripts::overlay_script_removal_tasks(
                        store.scripts.get(),
                        root,
                    ),
                );
            }
            tasks
        }
        TaskGraphCommand::Check => super::check::validation_tasks(store.aggregate.clone()),
    }
}

#[expect(
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
    let requested = resolve_filters(&task_refs, only, "--only")?;
    let excluded = resolve_filters(&task_refs, skip, "--skip")?;
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
                selection: selection_reason(task, &selected, &requested, &excluded),
            }
        })
        .collect())
}

fn selection_reason(
    task: &dyn Task,
    selected: &HashSet<TaskId>,
    requested: &HashSet<TaskId>,
    excluded: &HashSet<TaskId>,
) -> GraphSelection {
    if excluded.contains(&task.task_id()) {
        GraphSelection::Skipped
    } else if !selected.contains(&task.task_id()) {
        GraphSelection::Filtered
    } else if requested.is_empty() {
        GraphSelection::Default
    } else if requested.contains(&task.task_id()) {
        GraphSelection::Requested
    } else {
        GraphSelection::Dependency
    }
}

fn write_graph(
    listings: &[GraphListing],
    format: DiscoveryFormat,
    style: StyleChoice,
    out: &mut dyn std::io::Write,
) -> Result<()> {
    if format == DiscoveryFormat::Json {
        serde_json::to_writer_pretty(&mut *out, listings)?;
        writeln!(out)?;
        return Ok(());
    }
    if format == DiscoveryFormat::Table {
        writeln!(
            out,
            "{}{}",
            style.bold("Dependencies"),
            style.paint(TextStyle::Dim, " · predecessors first")
        )?;
        writeln!(
            out,
            "{}",
            style.paint(
                TextStyle::Dim,
                "  requires <- must succeed; after <- wait only"
            )
        )?;
        for listing in listings {
            let active = matches!(
                listing.selection,
                GraphSelection::Default | GraphSelection::Requested | GraphSelection::Dependency
            );
            let selector = style.paint(
                if active {
                    TextStyle::Cyan
                } else {
                    TextStyle::Dim
                },
                &listing.selector,
            );
            let selection = style.paint(
                selection_style(listing.selection),
                selection_label(listing.selection),
            );
            let internal = if listing.internal {
                style.paint(TextStyle::Dim, ", internal")
            } else {
                String::new()
            };
            let task = if active {
                listing.task.clone()
            } else {
                style.paint(TextStyle::Dim, &listing.task)
            };
            writeln!(out)?;
            writeln!(
                out,
                "{selector} {}{selection}{internal}{} {task}",
                style.paint(TextStyle::Dim, "["),
                style.paint(TextStyle::Dim, "] —"),
            )?;
            for predecessor in &listing.blocking {
                let target = if active {
                    predecessor.clone()
                } else {
                    style.paint(TextStyle::Dim, predecessor)
                };
                writeln!(
                    out,
                    "{}{target}",
                    style.paint(TextStyle::Dim, "  requires <- ")
                )?;
            }
            for predecessor in &listing.after {
                let target = if active {
                    predecessor.clone()
                } else {
                    style.paint(TextStyle::Dim, predecessor)
                };
                writeln!(
                    out,
                    "{}{target}",
                    style.paint(TextStyle::Dim, "  after    <- ")
                )?;
            }
        }
        return Ok(());
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
            selection_label(listing.selection)
        )?;
    }
    Ok(())
}

const fn selection_label(selection: GraphSelection) -> &'static str {
    match selection {
        GraphSelection::Default => "default",
        GraphSelection::Requested => "requested",
        GraphSelection::Dependency => "dependency",
        GraphSelection::Filtered => "filtered",
        GraphSelection::Skipped => "skipped",
    }
}

const fn selection_style(selection: GraphSelection) -> TextStyle {
    match selection {
        GraphSelection::Requested => TextStyle::Green,
        GraphSelection::Dependency => TextStyle::Cyan,
        GraphSelection::Skipped => TextStyle::Yellow,
        GraphSelection::Default | GraphSelection::Filtered => TextStyle::Dim,
    }
}

fn collect_listings(
    store: &ConfigStore,
    overlay: Option<&std::path::Path>,
) -> Result<Vec<TaskListing>> {
    let mut listings = BTreeMap::new();

    // Keying by selector merges command membership and keeps public output sorted.
    for command in [
        TaskGraphCommand::Update,
        TaskGraphCommand::Remove,
        TaskGraphCommand::Check,
    ] {
        let tasks = command_tasks(store, overlay, command);
        add_tasks(&mut listings, &tasks, command)?;
    }

    Ok(listings.into_values().collect())
}

fn add_tasks(
    listings: &mut BTreeMap<String, TaskListing>,
    tasks: &[Box<dyn Task>],
    command: TaskGraphCommand,
) -> Result<()> {
    for task in tasks {
        if task.visibility() == TaskVisibility::Internal {
            continue;
        }
        let listing = listings
            .entry(task.selector().to_string())
            .or_insert_with(|| TaskListing {
                selector: task.selector().to_string(),
                task: task.name().to_string(),
                ..TaskListing::default()
            });
        if listing.task != task.name() {
            bail!(
                "task selector '{}' is shared by '{}' and '{}'",
                task.selector(),
                listing.task,
                task.name()
            );
        }
        listing.include(command);
    }
    Ok(())
}

fn write_listings(
    listings: &[TaskListing],
    format: DiscoveryFormat,
    style: StyleChoice,
    out: &mut dyn std::io::Write,
) -> Result<()> {
    match format {
        DiscoveryFormat::Table => write_table(listings, style, out),
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

fn write_table(
    listings: &[TaskListing],
    style: StyleChoice,
    out: &mut dyn std::io::Write,
) -> Result<()> {
    let selector_width = listings
        .iter()
        .map(|listing| listing.selector.len())
        .max()
        .unwrap_or(8)
        .max("Selector".len());
    let mut first = true;
    for command in [
        TaskGraphCommand::Update,
        TaskGraphCommand::Remove,
        TaskGraphCommand::Check,
    ] {
        let rows = listings
            .iter()
            .filter(|listing| listing.commands.contains(&command))
            .collect::<Vec<_>>();
        if rows.is_empty() {
            continue;
        }
        if !first {
            writeln!(out)?;
        }
        first = false;
        let heading = match command {
            TaskGraphCommand::Update => "Update tasks",
            TaskGraphCommand::Remove => "Remove tasks",
            TaskGraphCommand::Check => "Check tasks",
        };
        writeln!(out, "{}", style.bold(heading))?;
        writeln!(
            out,
            "{}",
            style.paint(
                TextStyle::Dim,
                &format!("{:<selector_width$}  Task", "Selector")
            )
        )?;
        for listing in rows {
            let pad = selector_width - listing.selector.len();
            writeln!(
                out,
                "{}{:pad$}  {}",
                style.paint(TextStyle::Cyan, &listing.selector),
                "",
                listing.task
            )?;
        }
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
#[path = "tests/list.rs"]
mod tests;
