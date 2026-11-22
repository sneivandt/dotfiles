use super::*;

#[test]
fn verify_cli() {
    Cli::command().debug_assert();
}

#[test]
fn top_level_help_is_small_and_uses_canonical_commands() {
    let help = display_output(&["dotfiles", "--help"], ErrorKind::DisplayHelp);

    let normalized = help.split_whitespace().collect::<Vec<_>>().join(" ");
    for text in [
        "update Sync the repository and apply configuration",
        "remove Remove managed links and integrations",
        "check Validate configuration and repository files",
        "list Inspect task selectors and dependencies",
        "explain Explain how a configured home symlink is managed",
        "log Inspect previous runs and task diagnostics",
        "help Print this message or the help of the given subcommand(s)",
        "dotfiles check",
        "A task is a named group of work",
    ] {
        assert!(
            normalized.contains(text),
            "top-level help should contain {text:?}"
        );
    }
    for hidden in ["test       ", "--profile"] {
        assert!(
            !help.contains(hidden),
            "top-level help should omit {hidden:?}"
        );
    }
}

#[test]
fn mutation_help_explains_package_and_file_preservation_scope() {
    let update = display_output(&["dotfiles", "update", "--help"], ErrorKind::DisplayHelp);
    assert!(
        update.contains("On Arch, installing one can also upgrade the system."),
        "{update}"
    );
    assert!(update.contains("Changes and safety:"), "{update}");
    let remove = display_output(&["dotfiles", "remove", "--help"], ErrorKind::DisplayHelp);
    assert!(
        remove.contains("independent copies of their source contents"),
        "{remove}"
    );
    assert!(
        remove.contains("Also copy configuration to missing home paths"),
        "{remove}"
    );
    assert!(!remove.contains("Materialize"), "{remove}");
}

#[test]
fn update_help_documents_equivalence_and_shared_options() {
    let help = display_output(&["dotfiles", "update", "--help"], ErrorKind::DisplayHelp);
    for text in [
        "Usage: dotfiles update",
        "--dry-run",
        "--only",
        "--with-deps",
        "--no-repo-update",
    ] {
        assert!(help.contains(text), "update help should contain {text:?}");
    }
    assert!(!help.contains("--update-pins"));
}

#[test]
fn execution_help_groups_options_and_explains_side_effects() {
    for name in ["update", "remove", "check"] {
        let help = display_output(&["dotfiles", name, "--help"], ErrorKind::DisplayHelp);
        let mut previous = 0;
        for heading in ["Repository:", "Execution and output:", "Task selection:"] {
            let position = help.find(heading).expect("help heading");
            assert!(position > previous, "{name}: {heading}: {help}");
            previous = position;
        }
        for option in [
            "--profile",
            "--root",
            "--overlay",
            "--verbose",
            "--quiet",
            "--format",
            "--only",
            "--skip",
        ] {
            assert!(help.contains(option), "{name}: {option}: {help}");
        }
        assert!(
            help.contains("logs") && help.contains("selections"),
            "{name}: {help}"
        );
        if name != "check" {
            assert!(
                help.contains("Startup may check for CLI updates before running tasks"),
                "{name}: {help}"
            );
            assert!(
                help.contains("Dry runs") || help.contains("dry runs"),
                "{name}: {help}"
            );
        }
    }
    let help = display_output(&["dotfiles", "--help"], ErrorKind::DisplayHelp);
    for text in ["dotfiles [OPTIONS] [COMMAND]", "Default command: update"] {
        assert!(help.contains(text), "{text}: {help}");
    }
}

#[test]
fn standalone_short_flags_and_scoped_help_keep_the_explicit_command() {
    let Command::Check(check_opts) = Cli::try_parse_from(["dotfiles", "-v", "check"])
        .expect("valid short flag before check")
        .command
    else {
        panic!("expected check");
    };
    assert!(check_opts.execution.verbose);
    let Command::Remove(remove_opts) = Cli::try_parse_from(["dotfiles", "-n", "remove"])
        .expect("valid short flag before remove")
        .command
    else {
        panic!("expected remove");
    };
    assert!(remove_opts.dry_run);
    let help = display_output(&["dotfiles", "--dry-run", "--help"], ErrorKind::DisplayHelp);
    assert!(help.contains("Usage: dotfiles update [OPTIONS]"), "{help}");
    assert!(help.contains("--dry-run"), "{help}");
}

#[test]
fn remove_help_states_what_remains() {
    let help = display_output(&["dotfiles", "remove", "--help"], ErrorKind::DisplayHelp);
    assert!(help.contains("Preserves packages, services, registry values"));
    assert!(help.contains("-n, --dry-run"));
    assert!(help.contains("--only <SELECTOR>"));
    assert!(!help.contains("--with-deps"));
    assert!(!help.contains("--no-repo-update"));
}

#[test]
fn help_subcommand_uses_conventional_clap_behavior() {
    let help = display_output(&["dotfiles", "help", "update"], ErrorKind::DisplayHelp);
    assert!(help.contains("Usage: dotfiles update [OPTIONS]"));
}

#[test]
fn system_file_helper_keeps_snapshot_and_paths_as_data() {
    let cli = Cli::parse_from([
        "dotfiles",
        "system-file-helper",
        "--target",
        "/etc/example ' $(false)",
        "--fragment",
        "/repo/system/example",
        "--fragment-hash=-snapshot",
        "--merge",
        "toml",
        "--stage",
        "/etc/private-stage",
    ]);
    let Command::SystemFileHelper(opts) = cli.command else {
        panic!("expected standalone system-file helper");
    };
    assert_eq!(opts.target, std::path::Path::new("/etc/example ' $(false)"));
    assert_eq!(opts.fragment_hash, "-snapshot");
    assert_eq!(opts.merge, "toml");
}
