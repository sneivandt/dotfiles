use super::*;
use crate::domains::overlay::config::scripts::ScriptEntry;
use crate::engine::{Context, TaskMeta, TaskResult};
use crate::test_helpers::{empty_config, test_task};
use std::path::PathBuf;

test_task! {
    VisibleTask {
        name: "Visible task",
        selector: "visible",
    }
    InternalTask {
        name: "Internal task",
        selector: "internal",
        visibility: TaskVisibility::Internal,
    }
    DependentTask {
        name: "Dependent task",
        selector: "dependent",
        deps: [VisibleTask],
        ordering: [InternalTask],
    }
}

#[test]
fn discovery_filters_only_proven_inapplicability() {
    use crate::infra::platform::Os;

    let config = empty_config(PathBuf::from("/fixture"));
    for platform in [
        Platform::new(Os::Linux, true),
        Platform::new(Os::Linux, false),
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
            "git",
            "symlink-sources",
        ] {
            assert!(
                !potentially_applicable(selector, &config, platform, false),
                "{selector} on {platform}"
            );
        }
        for selector in [
            "agent-packages",
            "repository",
            "git-hooks",
            "launcher",
            "path",
            "shellcheck",
            "psscriptanalyzer",
            "apm-plugins",
            "config-files",
            "config-diagnostics",
            "script-example",
            "unknown",
        ] {
            assert!(
                potentially_applicable(selector, &config, platform, false),
                "uncertain or unconditional {selector} on {platform}"
            );
        }
        assert_eq!(
            potentially_applicable("developer-mode", &config, platform, false),
            platform.is_windows()
        );
        assert_eq!(
            potentially_applicable("login-shell", &config, platform, false),
            platform.is_linux()
        );
        assert_eq!(
            potentially_applicable("paru", &config, platform, false),
            platform.uses_pacman()
        );
        for selector in ["custom-package-repository", "custom-package-versions"] {
            assert_eq!(
                potentially_applicable(selector, &config, platform, false),
                platform.supports_aur(),
                "{selector} requires AUR support on {platform}"
            );
        }
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
    assert!(potentially_applicable("aur-packages", &config, arch, false));
    assert!(!potentially_applicable(
        "aur-packages",
        &config,
        windows,
        false
    ));
    assert!(!potentially_applicable("packages", &config, arch, false));
    config.packages.push(Package {
        name: "native-example".into(),
        is_aur: false,
    });
    assert!(potentially_applicable("packages", &config, arch, false));
    assert!(potentially_applicable("packages", &config, windows, false));
}

#[test]
fn documented_selectors_match_catalog() {
    let store = ConfigStore::from_config(empty_config(PathBuf::from("/fixture")));
    let actual: HashSet<_> = collect_listings(&store, None)
        .expect("collect static public tasks")
        .into_iter()
        .map(|listing| listing.selector)
        .collect();
    let documented: HashSet<_> =
        include_str!("../../../../../docs/content/reference/task-reference.md")
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
        "docs/content/reference/task-reference.md must list exactly the public catalog selectors"
    );
}

#[test]
fn task_membership_merges_by_selector() {
    let tasks: Vec<Box<dyn Task>> = vec![Box::new(VisibleTask)];
    let mut listings = BTreeMap::new();
    add_tasks(&mut listings, &tasks, TaskGraphCommand::Remove).expect("uninstall membership");
    add_tasks(&mut listings, &tasks, TaskGraphCommand::Update).expect("install membership");
    add_tasks(&mut listings, &tasks, TaskGraphCommand::Update).expect("repeated membership");

    assert_eq!(listings.len(), 1);
    assert_eq!(command_membership(&listings["visible"]), "update, remove");
}

#[test]
fn apm_lists_install_and_update_membership() {
    let store = ConfigStore::from_config(empty_config(PathBuf::from("/tmp")));
    let listings = collect_listings(&store, None).expect("collect task listings");
    let apm = listings
        .iter()
        .find(|listing| listing.selector == "agent-packages")
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
        check_remove: false,
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
    let mut listings = BTreeMap::new();
    add_tasks(&mut listings, &tasks, TaskGraphCommand::Update).expect("task discovery");
    assert!(listings.is_empty());
}

#[test]
fn output_formats_are_stable() {
    let listings = vec![
        TaskListing {
            selector: "git".to_string(),
            task: "Git config".to_string(),
            commands: vec![TaskGraphCommand::Update],
        },
        TaskListing {
            selector: "visible".to_string(),
            task: "Visible task".to_string(),
            commands: vec![TaskGraphCommand::Update],
        },
    ];

    let mut table = Vec::new();
    write_listings(
        &listings,
        DiscoveryFormat::Table,
        StyleChoice::plain(),
        &mut table,
    )
    .expect("table output");
    assert_eq!(
        String::from_utf8(table).unwrap(),
        "Update tasks\nSelector  Task\ngit       Git config\nvisible   Visible task\n"
    );

    let mut colored_table = Vec::new();
    write_listings(
        &listings,
        DiscoveryFormat::Table,
        StyleChoice::colored(),
        &mut colored_table,
    )
    .expect("colored table output");
    assert_eq!(
        String::from_utf8(colored_table).unwrap(),
        "\x1b[1mUpdate tasks\x1b[0m\n\x1b[2mSelector  Task\x1b[0m\n\x1b[36mgit\x1b[0m       Git config\n\x1b[36mvisible\x1b[0m   Visible task\n"
    );

    let mut plain = Vec::new();
    write_listings(
        &listings,
        DiscoveryFormat::Plain,
        StyleChoice::plain(),
        &mut plain,
    )
    .expect("plain output");
    assert_eq!(
        String::from_utf8(plain).unwrap(),
        "git\tGit config\tupdate\nvisible\tVisible task\tupdate\n"
    );

    let mut json = Vec::new();
    write_listings(
        &listings,
        DiscoveryFormat::Json,
        StyleChoice::plain(),
        &mut json,
    )
    .expect("JSON output");
    let value: serde_json::Value = serde_json::from_slice(&json).expect("valid JSON");
    assert_eq!(
        value,
        serde_json::json!([
            {
                "selector": "git",
                "task": "Git config",
                "commands": ["update"],
            },
            {
                "selector": "visible",
                "task": "Visible task",
                "commands": ["update"],
            },
        ])
    );
}

#[test]
fn table_groups_shared_tasks_under_each_command_alphabetically() {
    let store = ConfigStore::from_config(empty_config(PathBuf::from("/fixture")));
    let listings = collect_listings(&store, None).unwrap();
    let mut output = Vec::new();
    write_table(&listings, StyleChoice::plain(), &mut output).unwrap();
    let output = String::from_utf8(output).unwrap();
    let mut sections = output.split("\n\n");
    for (command, heading) in [
        (TaskGraphCommand::Update, "Update tasks"),
        (TaskGraphCommand::Remove, "Remove tasks"),
        (TaskGraphCommand::Check, "Check tasks"),
    ] {
        let section = sections.next().unwrap();
        let mut lines = section.lines();
        assert_eq!(lines.next(), Some(heading));
        assert!(lines.next().unwrap().starts_with("Selector"));
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
    let graph = collect_graph(&tasks, &["dependent".into()], &["visible".into()], false).unwrap();
    let mut output = Vec::new();
    write_graph(
        &graph,
        DiscoveryFormat::Table,
        StyleChoice::plain(),
        &mut output,
    )
    .unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("Dependencies · predecessors first"));
    assert!(output.contains("requires <- must succeed; after <- wait only"));
    assert!(output.contains("dependent [requested]"));
    assert!(output.contains("internal [filtered, internal]"));
    assert!(output.contains("visible [skipped]"));
    assert!(output.contains("\n  requires <- visible\n  after    <- internal\n"));
    assert!(
        !output.contains('\t'),
        "human output must not depend on tab stops"
    );

    let mut colored = Vec::new();
    write_graph(
        &graph,
        DiscoveryFormat::Table,
        StyleChoice::colored(),
        &mut colored,
    )
    .unwrap();
    let colored = String::from_utf8(colored).unwrap();
    assert!(colored.contains("\x1b[36mdependent\x1b[0m"));
    assert!(colored.contains("\x1b[32mrequested\x1b[0m"));
    assert!(colored.contains("\x1b[33mskipped\x1b[0m"));
    assert!(colored.contains("\x1b[2minternal\x1b[0m"));

    let mut plain = Vec::new();
    write_graph(
        &graph,
        DiscoveryFormat::Plain,
        StyleChoice::plain(),
        &mut plain,
    )
    .unwrap();
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

    let mut listings = BTreeMap::new();
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
    assert_eq!(listings["visible"].commands, [TaskGraphCommand::Update]);
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
    let graph = collect_graph(&tasks, &["dependent".into()], &[], true).expect("valid task graph");
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
    write_graph(
        &graph,
        DiscoveryFormat::Json,
        StyleChoice::plain(),
        &mut output,
    )
    .expect("graph JSON");
    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    assert_eq!(json[2]["blocking"][0], "visible");
}
