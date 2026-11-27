use super::*;

#[test]
fn omitted_command_is_update_with_the_same_options() {
    for args in [
        vec!["dotfiles"],
        vec![
            "dotfiles",
            "--dry-run",
            "--profile",
            "list",
            "--only",
            "apm",
        ],
        vec!["dotfiles", "--parent-run-id", "run-id", "--dry-run"],
    ] {
        let parsed = Cli::try_parse_from(args).unwrap();
        let Command::Update(_) = parsed.command else {
            panic!("expected update")
        };
    }
    for old in ["install", "uninstall", "tasks"] {
        assert_eq!(
            Cli::try_parse_from(["dotfiles", old]).unwrap_err().kind(),
            ErrorKind::InvalidSubcommand
        );
    }
    assert_eq!(
        Cli::try_parse_from(["dotfiles", "update", "--update"])
            .unwrap_err()
            .kind(),
        ErrorKind::UnknownArgument
    );
}

#[test]
fn version_aliases_and_verbose_remains_lowercase_v() {
    let version = display_output(&["dotfiles", "--version"], ErrorKind::DisplayVersion);
    let command = Cli::command();
    let expected = command.get_version().expect("CLI version");
    assert_eq!(version, format!("dotfiles {expected}\n"));

    for args in [
        &["dotfiles", "-V"][..],
        &["dotfiles", "update", "--version"][..],
    ] {
        assert!(display_output(args, ErrorKind::DisplayVersion).contains(expected));
    }

    let cli = Cli::parse_from(["dotfiles", "update", "-v"]);
    let Command::Update(opts) = cli.command else {
        panic!("expected update command");
    };
    assert!(opts.execution.verbose);
}

#[test]
fn update_accepts_scoped_options_and_new_names() {
    let cli = Cli::parse_from([
        "dotfiles",
        "update",
        "-p",
        "desktop",
        "-n",
        "--no-repo-update",
        "--fail-on-skip",
        "--only",
        "symlinks,git-hooks",
        "--with-deps",
    ]);
    let Command::Update(opts) = cli.command else {
        panic!("expected update command");
    };
    assert_eq!(opts.repository.profile.as_deref(), Some("desktop"));
    assert!(opts.dry_run);
    assert!(opts.no_repo_update);
    assert!(opts.execution.require_complete);
    assert_eq!(opts.tasks.filters.only, ["symlinks", "git-hooks"]);
    assert!(opts.tasks.with_deps);
}

#[test]
fn update_rejects_removed_option_names() {
    for old in [
        "-d",
        "--offline",
        "--require-complete",
        "--retry-failed",
        "--update-pins",
    ] {
        let error = Cli::try_parse_from(["dotfiles", "update", old])
            .expect_err("removed option should fail");
        assert_eq!(error.kind(), ErrorKind::UnknownArgument, "{old}");
    }
}

#[test]
fn command_options_are_not_accepted_by_unrelated_commands() {
    for args in [
        &["dotfiles", "log", "--dry-run"][..],
        &["dotfiles", "log", "--profile", "base"][..],
        &["dotfiles", "check", "--skip-attestation"][..],
        &["dotfiles", "check", "--with-deps"][..],
    ] {
        let error = Cli::try_parse_from(args.iter().copied())
            .expect_err("irrelevant option should fail during parsing");
        assert_eq!(error.kind(), ErrorKind::UnknownArgument, "{args:?}");
    }
}

#[test]
fn removed_check_command_is_rejected() {
    let error = Cli::try_parse_from(["dotfiles", "test"]).expect_err("test command was removed");
    assert_eq!(error.kind(), ErrorKind::InvalidSubcommand);
}

#[test]
fn check_accepts_task_filters_without_mutation_options() {
    let cli = Cli::parse_from([
        "dotfiles",
        "check",
        "--only",
        "config-diagnostics",
        "--skip",
        "apm-plugins",
    ]);
    let Command::Check(opts) = cli.command else {
        panic!("expected check command");
    };
    assert_eq!(opts.tasks.only, ["config-diagnostics"]);
    assert_eq!(opts.tasks.skip, ["apm-plugins"]);
}

#[test]
fn tasks_have_discovery_options() {
    let tasks = Cli::parse_from(["dotfiles", "list", "--profile", "base", "--format", "json"]);
    let Command::List(opts) = tasks.command else {
        panic!("expected tasks command");
    };
    assert_eq!(opts.repository.profile.as_deref(), Some("base"));
    assert_eq!(opts.format, DiscoveryFormat::Json);
    assert_eq!(opts.graph, None);
    assert!(!opts.all);
    let all = Cli::parse_from(["dotfiles", "list", "--all"]);
    assert!(matches!(all.command, Command::List(all_opts) if all_opts.all));
    assert!(Cli::try_parse_from(["dotfiles", "list", "--all", "--graph", "update"]).is_err());
}

#[test]
fn tasks_accept_graph_command() {
    let cli = Cli::parse_from([
        "dotfiles",
        "list",
        "--graph",
        "update",
        "--format",
        "json",
        "--only",
        "symlinks",
        "--with-deps",
    ]);
    let Command::List(opts) = cli.command else {
        panic!("expected tasks command");
    };
    assert_eq!(opts.graph, Some(TaskGraphCommand::Update));
    assert_eq!(opts.format, DiscoveryFormat::Json);
    assert_eq!(opts.only, ["symlinks"]);
    assert!(opts.with_deps);
}

#[test]
fn selection_options_enforce_their_required_companions() {
    for (args, required) in [
        (&["dotfiles", "update", "--with-deps"][..], "--only"),
        (&["dotfiles", "update", "--with-deps"][..], "--only"),
        (&["dotfiles", "list", "--only", "symlinks"][..], "--graph"),
        (&["dotfiles", "list", "--skip", "symlinks"][..], "--graph"),
        (
            &["dotfiles", "list", "--graph", "update", "--with-deps"][..],
            "--only",
        ),
    ] {
        let error = Cli::try_parse_from(args.iter().copied()).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument, "{args:?}");
        assert!(error.to_string().contains(required), "{args:?}: {error}");
    }
}

#[test]
fn repeated_and_comma_delimited_selectors_are_all_preserved() {
    for name in ["update", "check", "remove"] {
        let cli = Cli::parse_from([
            "dotfiles",
            name,
            "--only",
            "symlinks,git-hooks",
            "--only",
            "apm",
            "--skip",
            "packages,repository",
            "--skip",
            "chmod",
        ]);
        let (only, skip) = match cli.command {
            Command::Update(opts) => (opts.tasks.filters.only, opts.tasks.filters.skip),
            Command::Check(opts) => (opts.tasks.only, opts.tasks.skip),
            Command::Remove(opts) => (opts.tasks.filters.only, opts.tasks.filters.skip),
            Command::List(_)
            | Command::Explain(_)
            | Command::Log(_)
            | Command::SystemFileHelper(_)
            | Command::Completions(_) => {
                panic!("expected execution command")
            }
        };
        assert_eq!(only, ["symlinks", "git-hooks", "apm"], "{name}");
        assert_eq!(skip, ["packages", "repository", "chmod"], "{name}");
    }
}

#[test]
fn execution_commands_keep_dependency_expansion_scoped_to_update() {
    for name in ["update", "check", "remove"] {
        for selected in [false, true] {
            let mut args = vec!["dotfiles", name, "--with-deps"];
            if selected {
                args.extend(["--only", "symlinks"]);
            }
            let result = Cli::try_parse_from(args);
            match (name, selected) {
                ("update", true) => {
                    let Command::Update(opts) = result.unwrap().command else {
                        panic!("expected update");
                    };
                    assert!(opts.tasks.with_deps, "{name}");
                    assert_eq!(opts.tasks.filters.only, ["symlinks"], "{name}");
                }
                ("update", false) => {
                    let error = result.unwrap_err();
                    assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument, "{name}");
                    assert!(error.to_string().contains("--only"), "{name}: {error}");
                }
                _ => assert_eq!(
                    result.unwrap_err().kind(),
                    ErrorKind::UnknownArgument,
                    "{name}, selected={selected}"
                ),
            }
        }
    }
}

#[test]
fn execution_selector_arguments_keep_ids_completion_and_group_membership() {
    let mut command = Cli::command();
    command.build();
    for name in ["update", "check", "remove"] {
        let command = command.find_subcommand(name).unwrap();
        for id in ["skip", "only"] {
            let argument = command
                .get_arguments()
                .find(|argument| argument.get_id() == id)
                .unwrap();
            assert_eq!(argument.get_long(), Some(id), "{name}: {id}");
            assert_eq!(argument.get_value_delimiter(), Some(','), "{name}: {id}");
            assert_eq!(
                argument.get_value_names().unwrap(),
                ["SELECTOR"],
                "{name}: {id}"
            );
            assert!(
                argument
                    .get::<clap_complete::ArgValueCandidates>()
                    .is_some(),
                "{name}: {id} must retain task completion"
            );
        }
        let (id, expected) = if matches!(name, "update") {
            ("UpdateOpts", vec!["skip", "only", "with_deps"])
        } else {
            ("CheckOpts", vec!["skip", "only"])
        };
        let group = command
            .get_groups()
            .find(|group| group.get_id() == id)
            .unwrap();
        assert_eq!(
            group.get_args().map(clap::Id::as_str).collect::<Vec<_>>(),
            expected,
            "{name}: {id}"
        );
    }
}

#[test]
fn options_before_commands_preserve_values_and_command_scope() {
    for args in [
        vec!["dotfiles", "--profile", "base", "list", "--format", "json"],
        vec!["dotfiles", "--profile=base", "--format", "json", "list"],
        vec!["dotfiles", "-pbase", "list", "--format=json"],
    ] {
        let Command::List(opts) = Cli::parse_from(args).command else {
            panic!("expected list");
        };
        assert_eq!(opts.repository.profile.as_deref(), Some("base"));
        assert_eq!(opts.format, DiscoveryFormat::Json);
    }
    for args in [
        vec!["dotfiles", "--verbose", "check"],
        vec!["dotfiles", "-vpbase", "check"],
    ] {
        let Command::Check(opts) = Cli::parse_from(args).command else {
            panic!("expected check");
        };
        assert!(opts.execution.verbose);
    }
    for args in [
        vec!["dotfiles", "--profile", "list"],
        vec!["dotfiles", "--root", "check", "--profile", "log"],
    ] {
        assert!(matches!(Cli::parse_from(args).command, Command::Update(_)));
    }
    for args in [
        vec!["dotfiles", "--profile", "base", "log"],
        vec!["dotfiles", "--dry-run", "check"],
        vec!["dotfiles", "--unknown", "list"],
    ] {
        assert_eq!(
            Cli::try_parse_from(args).unwrap_err().kind(),
            ErrorKind::UnknownArgument
        );
    }
    assert!(Cli::try_parse_from(["dotfiles", "--", "list"]).is_err());
}

#[test]
fn non_unicode_repository_options_preserve_explicit_commands() {
    use std::ffi::OsString;

    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStringExt as _;
        OsString::from_vec(b"fixture-\xff".to_vec())
    };
    #[cfg(windows)]
    let path = {
        use std::os::windows::ffi::OsStringExt as _;
        OsString::from_wide(&[b'f'.into(), 0xd800])
    };
    for option in ["--root", "--overlay"] {
        for attached in [false, true] {
            let mut args = vec![OsString::from("dotfiles")];
            if attached {
                let mut value = OsString::from(format!("{option}="));
                value.push(&path);
                args.push(value);
            } else {
                args.extend([option.into(), path.clone()]);
            }
            args.extend(["list".into(), "--profile=base".into()]);
            let parsed =
                Cli::try_parse_from(args).expect("OS paths must not change command selection");
            let Command::List(opts) = parsed.command else {
                panic!("a non-Unicode repository path must never select update");
            };
            let selected = if option == "--root" {
                opts.repository.root
            } else {
                opts.repository.overlay
            };
            assert_eq!(selected.unwrap().as_os_str(), path);
        }
    }
}

#[test]
fn completion_shells_parse() {
    for shell in ["bash", "zsh", "fish", "powershell"] {
        assert!(matches!(
            Cli::parse_from(["dotfiles", "completions", shell]).command,
            Command::Completions(_)
        ));
    }
}
