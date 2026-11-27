use super::*;

fn complete(words: &[&str], index: usize) -> Vec<String> {
    let words = words.iter().map(OsString::from).collect::<Vec<_>>();
    let mut command = command_for_words(&words);
    clap_complete::engine::complete(&mut command, words, index, None)
        .expect("complete CLI arguments")
        .iter()
        .filter(|candidate| !candidate.is_hide_set())
        .map(|candidate| candidate.get_value().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn format_candidates_follow_the_command_before_and_after_options() {
    for (command, expected) in [
        ("update", vec!["human", "jsonl"]),
        ("remove", vec!["human", "jsonl"]),
        ("check", vec!["human", "jsonl"]),
        ("list", vec!["table", "plain", "json"]),
    ] {
        for (words, index) in [
            (vec!["dotfiles", command, "--format", ""], 3),
            (vec!["dotfiles", "--format", "", command], 2),
        ] {
            assert_eq!(complete(&words, index), expected, "{words:?}");
            for value in &expected {
                super::super::cli::Cli::try_parse_from(["dotfiles", "--format", value, command])
                    .expect("every suggested format must parse");
            }
        }
    }
    assert_eq!(
        complete(&["dotfiles", "--format", ""], 2),
        ["human", "jsonl"]
    );
    assert_eq!(
        complete(&["dotfiles", "--format=", "list"], 1),
        ["--format=table", "--format=plain", "--format=json"]
    );
    assert_eq!(
        complete(&["dotfiles", "--list", "--format", "", "log"], 3),
        ["table", "plain", "json"]
    );
}

fn assert_flags(words: &[&str], forbidden: &[&str], allowed: &str) {
    let candidates = complete(words, words.len() - 1);
    for flag in forbidden {
        assert!(
            !candidates.iter().any(|candidate| candidate == flag),
            "{words:?}: {flag}"
        );
    }
    assert!(
        candidates.iter().any(|candidate| candidate == allowed),
        "{words:?}: {allowed}"
    );
}

#[test]
fn completion_respects_conflicts_and_required_flags() {
    for (words, forbidden, allowed) in [
        (
            vec!["dotfiles", "update", "--quiet", "--"],
            vec!["--verbose", "--format"],
            "--dry-run",
        ),
        (
            vec!["dotfiles", "log", "--list", "--"],
            vec!["--raw", "--task", "--id", "--verbose"],
            "--format",
        ),
        (vec!["dotfiles", "log", "--"], vec!["--format"], "--list"),
        (
            vec!["dotfiles", "list", "--"],
            vec!["--only", "--skip", "--with-deps"],
            "--graph",
        ),
        (
            vec!["dotfiles", "list", "--graph", "check", "--"],
            vec!["--all"],
            "--only",
        ),
        (
            vec!["dotfiles", "update", "--"],
            vec!["--with-deps"],
            "--only",
        ),
        (
            vec!["dotfiles", "update", "--only", "symlinks", "--"],
            vec![],
            "--with-deps",
        ),
    ] {
        assert_flags(&words, &forbidden, allowed);
    }
    assert!(complete(&["dotfiles", "log", "--format", ""], 3).is_empty());
    assert!(complete(&["dotfiles", "update", "--quiet", "--format", ""], 4).is_empty());
}

#[test]
fn completion_handles_repeated_options_short_flags_and_positionals() {
    for (words, forbidden, allowed) in [
        (
            vec!["dotfiles", "log", "0", "--"],
            vec!["--list", "--id"],
            "--raw",
        ),
        (
            vec!["dotfiles", "update", "-q", "--"],
            vec!["--format", "--verbose", "--quiet"],
            "--dry-run",
        ),
        (
            vec!["dotfiles", "update", "--profile=base", "-q", "--"],
            vec!["--format", "--verbose"],
            "--dry-run",
        ),
        (
            vec!["dotfiles", "update", "--profile", "base", "--"],
            vec!["--profile"],
            "--only",
        ),
    ] {
        assert_flags(&words, &forbidden, allowed);
    }
}

#[test]
fn graph_dependency_completion_requires_an_update_graph() {
    for (words, forbidden, allowed) in [
        (
            vec![
                "dotfiles",
                "list",
                "--graph",
                "check",
                "--only",
                "apm-plugins",
                "--",
            ],
            vec!["--with-deps"],
            "--skip",
        ),
        (
            vec![
                "dotfiles",
                "list",
                "--graph=remove",
                "--only",
                "symlinks",
                "--",
            ],
            vec!["--with-deps"],
            "--skip",
        ),
        (
            vec![
                "dotfiles", "list", "--graph", "update", "--only", "symlinks", "--",
            ],
            vec![],
            "--with-deps",
        ),
    ] {
        assert_flags(&words, &forbidden, allowed);
    }
    assert_eq!(
        complete(
            &[
                "dotfiles",
                "list",
                "--only",
                "symlinks",
                "--with-deps",
                "--graph",
                ""
            ],
            6
        ),
        ["update"]
    );
    assert_eq!(
        complete(&["dotfiles", "list", "--graph", ""], 3),
        ["update", "remove", "check"]
    );
    assert_eq!(
        complete(
            &[
                "dotfiles",
                "list",
                "--graph",
                "update",
                "--only",
                "symlinks",
                "--with-deps",
                "--format",
                ""
            ],
            8
        ),
        ["table", "plain", "json"]
    );
    assert_eq!(
        complete(
            &[
                "dotfiles",
                "list",
                "--graph",
                "update",
                "--only",
                "symlinks",
                "--with-deps",
                "--profile",
                ""
            ],
            8
        ),
        ["base", "desktop"]
    );
}

#[test]
fn command_names_used_as_values_do_not_change_completion_context() {
    for name in [
        "log",
        "list",
        "explain",
        "check",
        "remove",
        "update",
        "completions",
        "help",
    ] {
        assert_eq!(
            complete(&["dotfiles", "--root", name, "--format", ""], 4),
            ["human", "jsonl"],
            "root named {name}"
        );
        let candidates = complete(&["dotfiles", "--root", name, "--quiet", "--"], 4);
        assert!(
            !candidates.contains(&"--format".to_owned()),
            "root named {name}"
        );
        assert!(
            !candidates.contains(&"--quiet".to_owned()),
            "repeated quiet, root named {name}"
        );
        assert!(
            candidates.contains(&"--dry-run".to_owned()),
            "root named {name}"
        );
    }
    let update_after_explain_root = complete(&["dotfiles", "update", "--root", "explain", "--"], 4);
    assert!(
        update_after_explain_root.contains(&"--dry-run".to_owned()),
        "expected update options after --root explain: {update_after_explain_root:?}"
    );
    assert_eq!(
        complete(&["dotfiles", "--root", "log", "list", "--format", ""], 5),
        ["table", "plain", "json"]
    );
    let candidates = complete(&["dotfiles", "log", "--command", "check", "--"], 4);
    assert!(candidates.contains(&"--list".to_owned()));
    assert!(!candidates.contains(&"--profile".to_owned()));
}

#[test]
fn graph_selector_membership_matches_the_selected_graph() {
    for command in ["update", "remove", "check"] {
        for graph in [vec!["--graph", command], vec![]] {
            let mut words = vec!["dotfiles", "list"];
            if graph.is_empty() {
                let attached = format!("--graph={command}");
                let attached_words =
                    ["dotfiles", "list", &attached, "--only", ""].map(OsString::from);
                assert_eq!(task_memberships(&attached_words), [command]);
            } else {
                words.extend(graph);
                words.extend(["--skip", ""]);
                assert_eq!(
                    task_memberships(&words.iter().map(OsString::from).collect::<Vec<_>>()),
                    [command]
                );
            }
        }
    }
}

#[test]
fn prefix_options_do_not_change_task_completion_membership() {
    for (words, expected) in [
        (
            vec!["dotfiles", "--profile", "base", "check", "--only", ""],
            &["check"][..],
        ),
        (
            vec!["dotfiles", "--root=update", "remove", "--only", ""],
            &["remove"][..],
        ),
        (
            vec!["dotfiles", "--profile", "check", "--only", ""],
            &["update"][..],
        ),
        (
            vec!["dotfiles", "-vpbase", "check", "--only", ""],
            &["check"][..],
        ),
    ] {
        let words = words.into_iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(task_memberships(&words), expected);
    }
}

#[test]
fn default_update_options_are_available_to_completion() {
    let mut command = command();
    command.build();
    for id in ["profile", "only", "dry_run", "no_repo_update"] {
        assert!(command.get_arguments().any(|arg| arg.get_id() == id));
    }
}

#[test]
fn profile_candidates_have_descriptions() {
    let profiles = profile_candidates();
    assert_eq!(
        profiles
            .iter()
            .map(CompletionCandidate::get_value)
            .collect::<Vec<_>>(),
        ["base", "desktop"]
    );
    assert!(profiles.iter().all(|profile| {
        profile
            .get_help()
            .is_some_and(|help| !help.to_string().is_empty())
    }));
}

#[test]
fn repository_options_are_forwarded_to_task_discovery() {
    let words = [
        "dotfiles",
        "update",
        "--profile=desktop",
        "--root",
        "/repo",
        "-pbase",
        "--overlay=/private",
        "--only",
        "sys",
    ]
    .map(OsString::from);

    assert_eq!(
        repository_args(&words),
        [
            "--profile",
            "desktop",
            "--root",
            "/repo",
            "--profile",
            "base",
            "--overlay",
            "/private"
        ]
        .map(OsString::from)
    );
}

#[test]
fn repository_options_preserve_value_boundaries() {
    for (words, expected) in [
        (vec![], vec![]),
        (vec!["--root"], vec![]),
        (vec!["-p", "base", "--root"], vec!["--profile", "base"]),
        (vec!["--root", "--overlay"], vec!["--root", "--overlay"]),
        (
            vec!["--overlay", "", "--root=/repo", "--root", "/other"],
            vec!["--overlay", "", "--root", "/repo", "--root", "/other"],
        ),
        (
            vec!["--only", "--profile=desktop", "--root=/repo"],
            vec!["--root", "/repo"],
        ),
        (
            vec!["--quiet", "--profile=desktop"],
            vec!["--profile", "desktop"],
        ),
        (
            vec!["--unknown-option", "--root=/repo"],
            vec!["--root", "/repo"],
        ),
        (
            vec!["--root=/repo", "--", "--profile=desktop"],
            vec!["--root", "/repo"],
        ),
    ] {
        let words = words.into_iter().map(OsString::from).collect::<Vec<_>>();
        let expected = expected.into_iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(repository_args(&words), expected, "{words:?}");
    }
}

#[test]
fn aliases_own_their_values_but_only_selected_options_count_as_present() {
    let selected = clap::Command::new("list").arg(
        clap::Arg::new("profile")
            .long("profile")
            .alias("preset")
            .short('p')
            .short_alias('P'),
    );
    let other = clap::Command::new("log").arg(clap::Arg::new("task").long("task"));
    let cli = clap::Command::new("dotfiles").subcommands([selected.clone(), other]);
    for spelling in [vec!["--preset", "log"], vec!["-P=log"]] {
        let mut input = vec![OsString::from("dotfiles")];
        input.extend(spelling.iter().map(OsString::from));
        input.push("list".into());
        super::super::cli::normalize_command_position(&mut input, &cli);
        assert_eq!(input[1], "list", "{spelling:?}");
        let raw = RawArgs::new(&spelling);
        assert_eq!(
            option_values(&raw, &cli).collect::<Vec<_>>(),
            [("profile", OsStr::new("log"))]
        );
    }
    // --task owns the next word in the union, but is unknown on `list`.
    // An unfinished --preset must still hide the selected command's profile.
    let raw = RawArgs::new(["--task", "--preset"]);
    assert_eq!(
        option_values(&raw, &cli).collect::<Vec<_>>(),
        [("task", OsStr::new("--preset"))]
    );
    assert_eq!(present_arguments(&selected, &raw), ["profile"]);
}

#[test]
fn clustered_profile_options_reach_task_discovery() {
    for flags in [
        vec!["-vpbase"],
        vec!["-vp", "base"],
        vec!["-vp=base"],
        vec!["--profile=base"],
    ] {
        let words = ["dotfiles", "check"]
            .into_iter()
            .chain(flags.iter().copied())
            .map(OsString::from)
            .collect::<Vec<_>>();
        let super::super::cli::Command::Check(source) =
            super::super::cli::Cli::try_parse_from(&words)
                .unwrap()
                .command
        else {
            panic!("expected check command");
        };
        let forwarded = ["dotfiles", "list"]
            .map(OsString::from)
            .into_iter()
            .chain(repository_args(&words));
        let super::super::cli::Command::List(discovery) =
            super::super::cli::Cli::try_parse_from(forwarded)
                .unwrap()
                .command
        else {
            panic!("expected discovery command");
        };
        assert_eq!(
            source.repository.profile.as_deref(),
            Some("base"),
            "{flags:?}"
        );
        assert_eq!(
            discovery.repository.profile, source.repository.profile,
            "{flags:?}"
        );
    }
}

#[test]
fn graph_values_respect_other_options_and_the_end_of_options() {
    use clap::CommandFactory as _;
    let command = super::super::cli::Cli::command();
    for words in [
        vec!["dotfiles", "list", "--root", "--graph=update"],
        vec!["dotfiles", "list", "--", "--graph=update"],
    ] {
        let words = words.into_iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(
            graph_value(&RawArgs::new(&words), &command),
            None,
            "{words:?}"
        );
        assert!(task_memberships(&words).is_empty(), "{words:?}");
    }
}

#[test]
fn non_unicode_repository_paths_preserve_graph_and_discovery_options() {
    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStringExt as _;
        OsString::from_vec(b"fixture=\xff=path".to_vec())
    };
    #[cfg(windows)]
    let path = {
        use std::os::windows::ffi::OsStringExt as _;
        OsString::from_wide(&[b'f'.into(), b'='.into(), 0xd800, b'='.into(), b'p'.into()])
    };
    for option in ["--root", "--overlay"] {
        let mut attached = OsString::from(format!("{option}="));
        attached.push(&path);
        for arguments in [vec![option.into(), path.clone()], vec![attached.clone()]] {
            for before_command in [false, true] {
                let mut words = vec![OsString::from("dotfiles")];
                if !before_command {
                    words.push("list".into());
                }
                words.extend(arguments.clone());
                if before_command {
                    words.push("list".into());
                }
                words.extend(["--graph".into(), "check".into(), "-pbase".into()]);
                assert_eq!(task_memberships(&words), ["check"]);
                assert_eq!(
                    repository_args(&words),
                    [
                        option.into(),
                        path.clone(),
                        "--profile".into(),
                        "base".into()
                    ],
                    "{words:?}"
                );
                let command = command_for_words(&words);
                let argument = command
                    .get_arguments()
                    .find(|argument| argument.get_long() == option.strip_prefix("--"))
                    .unwrap();
                assert!(
                    argument.is_hide_set(),
                    "{option} is already present: {words:?}"
                );
            }
        }
        for boundary in ["--", "--only"] {
            let words = [OsString::from(boundary), attached.clone()];
            assert!(
                repository_args(&words).is_empty(),
                "an attached path after {boundary} is not an option"
            );
        }
        let words = [OsString::from("--root"), attached.clone()];
        assert_eq!(
            repository_args(&words),
            [OsString::from("--root"), attached],
            "a following option-shaped value belongs to its preceding option"
        );
    }
}

#[test]
fn update_modes_complete_normal_and_update_only_tasks() {
    for words in [vec!["dotfiles", "update"], vec!["dotfiles", "--dry-run"]] {
        let words = words.into_iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(task_memberships(&words), ["update"]);
    }
}

#[test]
fn remove_completes_remove_tasks() {
    let words = ["dotfiles", "remove"]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    assert_eq!(task_memberships(&words), ["remove"]);
}

#[test]
fn standalone_and_unknown_commands_have_no_execution_task_membership() {
    for command in ["list", "log", "completions", "help", "unknown"] {
        let words = ["dotfiles", command].map(OsString::from);
        let expected: &[&str] = &[];
        assert_eq!(task_memberships(&words), expected, "{command}");
    }
}

#[test]
fn task_candidates_follow_command_membership() {
    use super::super::cli::TaskGraphCommand;

    let listings = || {
        vec![
            TaskListing {
                selector: "symlinks".into(),
                task: "Home symlinks".into(),
                commands: vec![TaskGraphCommand::Update, TaskGraphCommand::Remove],
            },
            TaskListing {
                selector: "apm-plugins".into(),
                task: "APM plugins".into(),
                commands: vec![TaskGraphCommand::Check],
            },
            TaskListing {
                selector: "pin-only".into(),
                task: "Pin updater".into(),
                commands: vec![TaskGraphCommand::Update],
            },
        ]
    };

    let update = task_candidates_from_listings(listings(), &["update"]);
    assert_eq!(update.len(), 2);
    assert_eq!(update[0].get_value(), "symlinks");
    assert_eq!(update[1].get_value(), "pin-only");

    let check = task_candidates_from_listings(listings(), &["check"]);
    assert_eq!(check.len(), 1);
    assert_eq!(check[0].get_value(), "apm-plugins");
}

#[test]
fn registration_uses_runtime_completion_and_keeps_the_dot_completer() {
    let zsh = crate::app::catalog::generate_zsh_completions();
    assert_eq!(zsh, "source <(DOTFILES_COMPLETE=zsh dotfiles)\n");
    assert_eq!(
        registration(clap_complete::Shell::Bash),
        "source <(DOTFILES_COMPLETE=bash dotfiles)\n"
    );
    assert_eq!(
        registration(clap_complete::Shell::Elvish),
        "eval (E:DOTFILES_COMPLETE=elvish dotfiles | slurp)\n"
    );
    assert_eq!(
        registration(clap_complete::Shell::Fish),
        "DOTFILES_COMPLETE=fish dotfiles | source\n"
    );

    let powershell = crate::app::catalog::generate_powershell_completions();
    assert!(powershell.contains("DOTFILES_COMPLETE = 'powershell'"));
    assert!(powershell.contains("-Native -CommandName 'dot'"));
    assert!(!powershell.contains("-ParameterName 'Arguments'"));
    assert!(powershell.contains("$cursorPosition - $commandAst.Extent.StartOffset"));
}

#[test]
fn empty_positional_word_is_not_counted_as_present() {
    use clap::CommandFactory as _;
    let log = super::super::cli::Cli::command()
        .find_subcommand("log")
        .unwrap()
        .clone();
    assert!(present_arguments(&log, &RawArgs::new([""])).is_empty());
    assert_eq!(present_arguments(&log, &RawArgs::new(["0"])), ["run"]);
}
