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
    commands: Vec<TaskGraphCommand>,
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
        if opts.with_deps && !matches!(command, TaskGraphCommand::Update) {
            bail!("--with-deps is only available for update graphs");
        }
        let tasks = command_tasks(&store, overlay.as_deref(), command);
        let listings = collect_graph(&tasks, &opts.only, &opts.skip, opts.with_deps)?;
        write_graph(&listings, opts.format, &mut stdout.lock())
    } else {
        let mut listings = collect_listings(&store, overlay.as_deref())?;
        if !opts.all {
            listings.retain(|listing| {
                potentially_applicable(&listing.selector, store.aggregate.get(), platform)
            });
        }
        write_listings(&listings, opts.format, &mut stdout.lock())
    }
}

/// Only reject tasks whose platform or immutable configuration proves there
/// is no work. Keep unknown tasks and filesystem/runtime-dependent cases.
fn potentially_applicable(selector: &str, config: &Config, platform: Platform) -> bool {
    match selector {
        "developer-mode" => platform.is_windows(),
        "registry" => platform.has_registry() && !config.registry.is_empty(),
        "file-permissions" => platform.supports_chmod() && !config.chmod.is_empty(),
        "packages" => config.packages.iter().any(|package| !package.is_aur),
        "aur-packages" => {
            platform.supports_aur() && config.packages.iter().any(|package| package.is_aur)
        }
        "paru" => platform.uses_pacman(),
        "system-files" => platform.is_linux() && !config.system_files.is_empty(),
        "systemd" => platform.supports_systemd() && !config.units.is_empty(),
        "shell" => platform.is_linux(),
        "vscode-extensions" => !config.vscode_extensions.is_empty(),
        "agent-settings" => !config.agent_settings.is_empty(),
        "symlinks" => !config.symlinks.is_empty(),
        // Windows also removes an unmanaged core.autocrlf setting.
        "git" => platform.is_windows() || !config.git_settings.is_empty(),
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
            let mut tasks = crate::app::catalog::install_tasks_for_run(
                store,
                &RepositoryUpdateSignal::new(),
                mode,
            );
            tasks.retain(|task| {
                super::install::includes_task(task.as_ref(), command == TaskGraphCommand::Update)
            });
            if let Some(root) = overlay {
                tasks.extend(crate::domains::overlay::scripts::overlay_script_tasks(
                    store.scripts.get(),
                    root,
                ));
            }
            tasks
        }
        TaskGraphCommand::Remove => {
            let mut tasks = crate::app::catalog::all_uninstall_tasks(store);
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
        writeln!(out, "Dependencies (predecessors first)")?;
        writeln!(out, "  requires <- must succeed; after <- wait only")?;
        for listing in listings {
            writeln!(out)?;
            writeln!(
                out,
                "{} [{}{}] — {}",
                listing.selector,
                selection_label(listing.selection),
                if listing.internal { ", internal" } else { "" },
                listing.task,
            )?;
            for predecessor in &listing.blocking {
                writeln!(out, "  requires <- {predecessor}")?;
            }
            for predecessor in &listing.after {
                writeln!(out, "  after    <- {predecessor}")?;
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

fn collect_listings(
    store: &ConfigStore,
    overlay: Option<&std::path::Path>,
) -> Result<Vec<TaskListing>> {
    let mut listings = Vec::new();

    // Merge command membership before sorting the unique public selectors.
    for command in [
        TaskGraphCommand::Update,
        TaskGraphCommand::Remove,
        TaskGraphCommand::Check,
    ] {
        let tasks = command_tasks(store, overlay, command);
        add_tasks(&mut listings, &tasks, command)?;
    }

    listings.sort_by(|left, right| left.selector.cmp(&right.selector));
    Ok(listings)
}

fn add_tasks(
    listings: &mut Vec<TaskListing>,
    tasks: &[Box<dyn Task>],
    command: TaskGraphCommand,
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
            listing.include(command);
            continue;
        }

        let mut listing = TaskListing {
            selector: task.selector().to_string(),
            task: task.name().to_string(),
            ..TaskListing::default()
        };
        listing.include(command);
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
        writeln!(out, "{}", command.label())?;
        writeln!(out, "{:<selector_width$}  TASK", "SELECTOR")?;
        for listing in rows {
            writeln!(
                out,
                "{:<selector_width$}  {}",
                listing.selector, listing.task
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
mod tests {
    use super::*;
    use crate::domains::overlay::config::scripts::ScriptEntry;
    use crate::engine::{Context, TaskId, TaskMeta, TaskResult};
    use crate::test_helpers::empty_config;
    use std::path::PathBuf;

    struct VisibleTask;

    #[test]
    fn discovery_filters_only_proven_inapplicability() {
        use crate::infra::platform::Os;

        let config = empty_config(PathBuf::from("/fixture"));
        for platform in [
            Platform::new(Os::Linux, true),
            Platform::new(Os::Windows, false),
        ] {
            for selector in [
                "registry",
                "file-permissions",
                "packages",
                "aur-packages",
                "system-files",
                "systemd",
                "vscode-extensions",
                "agent-settings",
                "symlinks",
                "symlink-sources",
            ] {
                assert!(
                    !potentially_applicable(selector, &config, platform),
                    "{selector} on {platform}"
                );
            }
            for selector in [
                "apm",
                "repository",
                "git-hooks",
                "launcher",
                "path",
                "shellcheck",
                "psscriptanalyzer",
                "apm-plugins",
                "config-files",
                "config-warnings",
                "script-example",
                "unknown",
            ] {
                assert!(
                    potentially_applicable(selector, &config, platform),
                    "uncertain or unconditional {selector} on {platform}"
                );
            }
            assert_eq!(
                potentially_applicable("developer-mode", &config, platform),
                platform.is_windows()
            );
            assert_eq!(
                potentially_applicable("shell", &config, platform),
                platform.is_linux()
            );
            assert_eq!(
                potentially_applicable("paru", &config, platform),
                platform.uses_pacman()
            );
            assert_eq!(
                potentially_applicable("git", &config, platform),
                platform.is_windows(),
                "Windows git cleanup applies with empty configuration"
            );
        }
    }

    #[test]
    fn configured_packages_remain_visible_without_tool_probes() {
        use crate::domains::packages::config::packages::Package;
        use crate::infra::platform::Os;

        let mut config = empty_config(PathBuf::from("/fixture"));
        config.packages = vec![Package {
            name: "example".into(),
            is_aur: true,
        }];
        let arch = Platform::new(Os::Linux, true);
        let windows = Platform::new(Os::Windows, false);
        assert!(potentially_applicable("aur-packages", &config, arch));
        assert!(!potentially_applicable("aur-packages", &config, windows));
        assert!(!potentially_applicable("packages", &config, arch));
        config.packages.push(Package {
            name: "native-example".into(),
            is_aur: false,
        });
        assert!(potentially_applicable("packages", &config, arch));
        assert!(potentially_applicable("packages", &config, windows));
    }

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
    fn documented_selectors_match_catalog() {
        let store = ConfigStore::from_config(empty_config(PathBuf::from("/fixture")));
        let actual: HashSet<_> = collect_listings(&store, None)
            .expect("collect static public tasks")
            .into_iter()
            .map(|listing| listing.selector)
            .collect();
        let documented: HashSet<_> = include_str!("../../../../docs/TASKS.md")
            .lines()
            .filter_map(|line| {
                line.strip_prefix("| `")?
                    .split_once("` |")
                    .map(|(selector, _)| selector)
                    .filter(|selector| {
                        selector.bytes().all(|byte| {
                            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                        })
                    })
                    .map(str::to_owned)
            })
            .collect();
        assert_eq!(
            documented, actual,
            "docs/TASKS.md must list exactly the public catalog selectors"
        );
    }

    #[test]
    fn task_membership_merges_by_selector() {
        let tasks: Vec<Box<dyn Task>> = vec![Box::new(VisibleTask)];
        let mut listings = Vec::new();
        add_tasks(&mut listings, &tasks, TaskGraphCommand::Remove).expect("uninstall membership");
        add_tasks(&mut listings, &tasks, TaskGraphCommand::Update).expect("install membership");
        add_tasks(&mut listings, &tasks, TaskGraphCommand::Update).expect("repeated membership");

        assert_eq!(listings.len(), 1);
        assert_eq!(command_membership(&listings[0]), "update, remove");
    }

    #[test]
    fn apm_lists_install_and_update_membership() {
        let store = ConfigStore::from_config(empty_config(PathBuf::from("/tmp")));
        let listings = collect_listings(&store, None).expect("collect task listings");
        let apm = listings
            .iter()
            .find(|listing| listing.selector == "apm")
            .expect("APM task listing");

        assert_eq!(command_membership(apm), "update");
        let symlinks = listings
            .iter()
            .find(|listing| listing.selector == "symlinks")
            .expect("symlink task listing");
        assert_eq!(command_membership(symlinks), "update, remove");
    }

    #[test]
    fn overlay_script_is_listed_for_uninstall_and_its_graph() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.scripts = vec![ScriptEntry {
            name: "Private tools".to_string(),
            path: "scripts/tools.sh".to_string(),
            description: None,
        }];
        let store = ConfigStore::from_config(config);
        let overlay = std::path::Path::new("/tmp/overlay");
        let listings = collect_listings(&store, Some(overlay)).expect("task discovery");
        let script = listings
            .iter()
            .find(|listing| listing.selector == "script-private-tools")
            .expect("overlay script listing");
        assert_eq!(command_membership(script), "update, remove");

        assert!(
            listings
                .windows(2)
                .all(|pair| pair[0].selector < pair[1].selector),
            "public selectors must be unique and alphabetical, including overlay scripts"
        );
        for command in [
            TaskGraphCommand::Update,
            TaskGraphCommand::Remove,
            TaskGraphCommand::Check,
        ] {
            let graph = command_tasks(&store, Some(overlay), command);
            let expected = graph
                .iter()
                .filter(|task| task.visibility().is_visible())
                .map(|task| task.selector())
                .collect::<HashSet<_>>();
            let listed = listings
                .iter()
                .filter(|listing| listing.commands.contains(&command))
                .map(|listing| listing.selector.as_str())
                .collect::<HashSet<_>>();
            assert_eq!(
                listed, expected,
                "{command:?} membership must match its graph"
            );
        }

        let graph = command_tasks(&store, Some(overlay), TaskGraphCommand::Remove);
        assert!(
            graph
                .iter()
                .any(|task| task.selector() == "script-private-tools")
        );
    }

    #[test]
    fn internal_tasks_are_hidden() {
        let tasks: Vec<Box<dyn Task>> = vec![Box::new(InternalTask)];
        let mut listings = Vec::new();
        add_tasks(&mut listings, &tasks, TaskGraphCommand::Update).expect("task discovery");
        assert!(listings.is_empty());
    }

    #[test]
    fn output_formats_are_stable() {
        let listings = vec![TaskListing {
            selector: "visible".to_string(),
            task: "Visible task".to_string(),
            commands: vec![TaskGraphCommand::Update],
        }];

        let mut table = Vec::new();
        write_listings(&listings, DiscoveryFormat::Table, &mut table).expect("table output");
        assert_eq!(
            String::from_utf8(table).unwrap(),
            "update\nSELECTOR  TASK\nvisible   Visible task\n"
        );

        let mut plain = Vec::new();
        write_listings(&listings, DiscoveryFormat::Plain, &mut plain).expect("plain output");
        assert_eq!(
            String::from_utf8(plain).unwrap(),
            "visible\tVisible task\tupdate\n"
        );

        let mut json = Vec::new();
        write_listings(&listings, DiscoveryFormat::Json, &mut json).expect("JSON output");
        let value: serde_json::Value = serde_json::from_slice(&json).expect("valid JSON");
        assert_eq!(
            value,
            serde_json::json!([{
                "selector": "visible",
                "task": "Visible task",
                "commands": ["update"],
            }])
        );
    }

    #[test]
    fn table_groups_shared_tasks_under_each_command_alphabetically() {
        let store = ConfigStore::from_config(empty_config(PathBuf::from("/fixture")));
        let listings = collect_listings(&store, None).unwrap();
        let mut output = Vec::new();
        write_table(&listings, &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        let mut sections = output.split("\n\n");
        for command in [
            TaskGraphCommand::Update,
            TaskGraphCommand::Remove,
            TaskGraphCommand::Check,
        ] {
            let section = sections.next().unwrap();
            let mut lines = section.lines();
            assert_eq!(lines.next(), Some(command.label()));
            assert!(lines.next().unwrap().starts_with("SELECTOR"));
            let selectors = lines
                .map(|line| line.split_whitespace().next().unwrap())
                .collect::<Vec<_>>();
            assert!(selectors.windows(2).all(|pair| pair[0] < pair[1]));
            let expected = listings
                .iter()
                .filter(|listing| listing.commands.contains(&command))
                .map(|listing| listing.selector.as_str())
                .collect::<Vec<_>>();
            assert_eq!(
                selectors,
                expected,
                "all members of {} must appear",
                command.label()
            );
        }
        assert!(sections.next().is_none());
        assert_eq!(
            output
                .lines()
                .filter(|line| line.starts_with("symlinks "))
                .count(),
            2
        );
    }

    #[test]
    fn graph_table_explains_edges_and_keeps_selection_and_internal_nodes_visible() {
        let tasks: Vec<Box<dyn Task>> = vec![
            Box::new(DependentTask),
            Box::new(InternalTask),
            Box::new(VisibleTask),
        ];
        let graph =
            collect_graph(&tasks, &["dependent".into()], &["visible".into()], false).unwrap();
        let mut output = Vec::new();
        write_graph(&graph, DiscoveryFormat::Table, &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("requires <- must succeed; after <- wait only"));
        assert!(output.contains("dependent [requested]"));
        assert!(output.contains("internal [filtered, internal]"));
        assert!(output.contains("visible [skipped]"));
        assert!(output.contains("\n  requires <- visible\n  after    <- internal\n"));
        assert!(
            !output.contains('\t'),
            "human output must not depend on tab stops"
        );

        let mut plain = Vec::new();
        write_graph(&graph, DiscoveryFormat::Plain, &mut plain).unwrap();
        let plain = String::from_utf8(plain).unwrap();
        assert!(plain.lines().all(|line| line.split('\t').count() == 6));
        assert!(plain.contains("visible\tinternal\tfalse\trequested"));
    }

    #[test]
    fn reused_selector_with_a_different_label_is_rejected() {
        struct ConflictingTask;
        impl Task for ConflictingTask {
            fn meta(&self) -> TaskMeta<'_> {
                TaskMeta::new("Different task").with_selector("visible")
            }

            fn run(&self, _ctx: &Context) -> Result<TaskResult> {
                panic!("discovery must not execute tasks");
            }
        }

        let mut listings = Vec::new();
        add_tasks(
            &mut listings,
            &[Box::new(VisibleTask)],
            TaskGraphCommand::Update,
        )
        .unwrap();
        let error = add_tasks(
            &mut listings,
            &[Box::new(ConflictingTask)],
            TaskGraphCommand::Check,
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "task selector 'visible' is shared by 'Visible task' and 'Different task'"
        );
        assert_eq!(listings.len(), 1);
        assert_eq!(listings[0].commands, [TaskGraphCommand::Update]);
    }

    #[test]
    fn graph_selection_distinguishes_skips_from_filters_and_defaults() {
        let tasks: Vec<Box<dyn Task>> = vec![
            Box::new(DependentTask),
            Box::new(InternalTask),
            Box::new(VisibleTask),
        ];
        for (only, skip, expected) in [
            (
                vec![],
                vec![],
                [
                    ("dependent", GraphSelection::Default),
                    ("internal", GraphSelection::Default),
                    ("visible", GraphSelection::Default),
                ],
            ),
            (
                vec!["dependent".into()],
                vec!["visible".into()],
                [
                    ("dependent", GraphSelection::Requested),
                    ("internal", GraphSelection::Filtered),
                    ("visible", GraphSelection::Skipped),
                ],
            ),
        ] {
            let graph = collect_graph(&tasks, &only, &skip, false).unwrap();
            assert_eq!(graph.len(), expected.len());
            for (selector, selection) in expected {
                assert_eq!(
                    graph
                        .iter()
                        .find(|entry| entry.selector == selector)
                        .unwrap()
                        .selection,
                    selection,
                    "{selector}: only={only:?}, skip={skip:?}"
                );
            }
        }
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
