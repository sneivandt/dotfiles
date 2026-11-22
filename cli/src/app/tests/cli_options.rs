use super::*;

#[test]
fn historical_log_filters_remain_available() {
    for (name, stored) in [
        ("install", "install"),
        ("uninstall", "uninstall"),
        ("remove", "remove"),
        ("update", "update"),
    ] {
        let parsed = Cli::parse_from(["dotfiles", "log", "--command", name]);
        let Command::Log(opts) = parsed.command else {
            panic!("expected log")
        };
        assert!(opts.command.unwrap().matches(stored));
    }
}

#[test]
fn output_modes_validate_conflicts_and_convert_to_runtime_policy() {
    for name in ["update", "remove", "check"] {
        for (flag, value, expected) in [
            ("--quiet", None, crate::infra::logging::OutputMode::Quiet),
            (
                "--format",
                Some("jsonl"),
                crate::infra::logging::OutputMode::Jsonl,
            ),
        ] {
            let mut args = vec!["dotfiles", name, flag];
            args.extend(value);
            let global = match Cli::parse_from(args).command {
                Command::Update(opts) => opts.into_engine_parts().0,
                Command::Remove(opts) => opts.into_engine_parts().0,
                Command::Check(opts) => opts.into_engine_parts().0,
                Command::List(_)
                | Command::Explain(_)
                | Command::Log(_)
                | Command::SystemFileHelper(_)
                | Command::Completions(_) => {
                    panic!("expected engine command")
                }
            };
            assert_eq!(global.output_mode, expected);
        }
        for flags in [
            vec!["--quiet", "--verbose"],
            vec!["--quiet", "--format", "jsonl"],
        ] {
            let mut args = vec!["dotfiles", name];
            args.extend(flags);
            assert_eq!(
                Cli::try_parse_from(args).unwrap_err().kind(),
                ErrorKind::ArgumentConflict
            );
        }
    }
}

#[test]
fn log_only_accepts_log_output_options() {
    let cli = Cli::parse_from(["dotfiles", "log", "2", "-c", "update", "-v"]);
    let Command::Log(opts) = cli.command else {
        panic!("expected log command");
    };
    assert_eq!(opts.run, Some(2));
    assert_eq!(opts.command, Some(LogCommand::Update));
    assert!(opts.verbose);
}

#[test]
fn log_selection_modes_and_output_options_reject_ignored_combinations() {
    for (args, expected) in [
        (
            &["dotfiles", "log", "0", "--list"][..],
            ErrorKind::ArgumentConflict,
        ),
        (
            &["dotfiles", "log", "--id", "run-id", "--command", "update"][..],
            ErrorKind::ArgumentConflict,
        ),
        (
            &["dotfiles", "log", "--id", "run-id", "0"][..],
            ErrorKind::ArgumentConflict,
        ),
        (
            &["dotfiles", "log", "--id", "run-id", "--list"][..],
            ErrorKind::ArgumentConflict,
        ),
        (
            &["dotfiles", "log", "--list", "--task", "symlinks"][..],
            ErrorKind::ArgumentConflict,
        ),
        (
            &["dotfiles", "log", "--list", "--raw"][..],
            ErrorKind::ArgumentConflict,
        ),
        (
            &["dotfiles", "log", "--list", "--verbose"][..],
            ErrorKind::ArgumentConflict,
        ),
        (
            &["dotfiles", "log", "--raw", "--verbose"][..],
            ErrorKind::ArgumentConflict,
        ),
        (
            &["dotfiles", "log", "--format", "json"][..],
            ErrorKind::MissingRequiredArgument,
        ),
        (
            &["dotfiles", "log", "--command", "unknown"][..],
            ErrorKind::InvalidValue,
        ),
        (
            &["dotfiles", "log", "--command", "test"][..],
            ErrorKind::InvalidValue,
        ),
    ] {
        let error = Cli::try_parse_from(args.iter().copied())
            .expect_err("ignored or invalid log option combinations should fail");
        assert_eq!(error.kind(), expected, "{args:?}");
    }

    let Command::Log(opts) = Cli::parse_from(["dotfiles", "log", "-c", "update"]).command else {
        panic!("expected log command");
    };
    assert_eq!(opts.command, Some(LogCommand::Update));
}

#[test]
fn engine_option_conversion_preserves_command_and_output_flags() {
    for name in ["update", "remove", "check"] {
        let mut args = vec![
            "dotfiles",
            name,
            "--profile",
            "base",
            "--root",
            "/repo",
            "--overlay",
            "/overlay",
            "--verbose",
            "--no-parallel",
            "--fail-on-skip",
            "--non-interactive",
            "--no-symbols",
        ];
        let mutating = matches!(name, "update" | "remove");
        if mutating {
            args.extend(["--dry-run", "--skip-attestation", "--elevated-child"]);
        }
        if matches!(name, "update") {
            args.push("--no-repo-update");
        }
        let (global, verbose) = match Cli::parse_from(args).command {
            Command::Update(opts) => {
                let (global, _, verbose) = opts.into_engine_parts();
                assert!(global.no_repo_update);
                (global, verbose)
            }
            Command::Remove(opts) => {
                let (global, _, verbose) = opts.into_engine_parts();
                assert!(!global.no_repo_update);
                (global, verbose)
            }
            Command::Check(opts) => {
                let (global, _, verbose) = opts.into_engine_parts();
                assert!(!global.no_repo_update);
                (global, verbose)
            }
            Command::List(_)
            | Command::Explain(_)
            | Command::Log(_)
            | Command::SystemFileHelper(_)
            | Command::Completions(_) => {
                panic!("expected engine command")
            }
        };
        let runtime = crate::app::commands::RuntimePolicy::new(
            &global,
            verbose,
            crate::infra::env::MapEnv::new().into_handle(),
            true,
            true,
        );
        assert_eq!(global.profile.as_deref(), Some("base"));
        assert_eq!(global.root.as_deref(), Some(std::path::Path::new("/repo")));
        assert_eq!(
            global.overlay.as_deref(),
            Some(std::path::Path::new("/overlay"))
        );
        assert!(runtime.verbose);
        assert!(runtime.global.no_symbols);
        assert!(!runtime.execution.parallel);
        assert!(runtime.execution.require_complete);
        assert!(runtime.execution.non_interactive);
        assert_eq!(runtime.execution.dry_run, mutating);
        assert_eq!(global.skip_attestation, mutating);
        assert_eq!(runtime.execution.elevated_child, mutating);
    }
}
