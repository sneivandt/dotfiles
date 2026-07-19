//! CLI argument definitions and top-level argument parsing.
use clap::{Parser, Subcommand};

/// Top-level CLI entry point for the dotfiles management engine.
#[derive(Parser, Debug)]
#[command(
    name = "dotfiles",
    about = "Manage system configuration from this dotfiles repository",
    version = option_env!("DOTFILES_VERSION").unwrap_or(concat!("dev-", env!("CARGO_PKG_VERSION"))),
    disable_version_flag = true,
    disable_help_subcommand = true,
    after_help = "\
Examples:
  dotfiles install
  dotfiles install --dry-run
  dotfiles install --only symlinks
  dotfiles test",
    help_template = "\
{about-with-newline}
{usage-heading} {usage}

{all-args}{after-help}
"
)]
pub struct Cli {
    /// Subcommand to execute.
    #[command(subcommand)]
    pub command: Command,

    /// Show complete task and action details
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Print version
    #[arg(long, action = clap::ArgAction::Version)]
    pub version: Option<bool>,

    /// Options shared across all subcommands.
    #[command(flatten)]
    pub global: GlobalOpts,
}

/// Options shared across all subcommands.
#[derive(Parser, Debug, Clone)]
pub struct GlobalOpts {
    /// Use a specific profile
    #[arg(short, long, global = true, value_name = "PROFILE")]
    pub profile: Option<String>,

    /// Preview changes without applying them
    #[arg(short = 'd', long, global = true)]
    pub dry_run: bool,

    /// Use PATH as the dotfiles repository
    #[arg(long, global = true, value_name = "PATH")]
    pub root: Option<std::path::PathBuf>,

    /// Merge configuration from an overlay repository
    #[arg(long, global = true, value_name = "PATH")]
    pub overlay: Option<std::path::PathBuf>,

    /// Run tasks sequentially
    #[arg(long = "no-parallel", global = true, action = clap::ArgAction::SetFalse)]
    pub parallel: bool,
}

/// Available subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Apply dotfiles and system configuration
    Install(InstallOpts),
    /// Apply configuration and advance pinned dependencies
    Update(UpdateOpts),
    /// Remove managed integrations while preserving user files
    Uninstall(UninstallOpts),
    /// Validate configuration and run self-tests
    Test(TestOpts),
    /// Show the latest run log
    Log(LogOpts),
    /// Generate shell completions for the given shell
    #[command(hide = true)]
    Completions(CompletionsOpts),
}

/// Options for the `install` subcommand.
#[derive(Parser, Debug, Clone)]
pub struct InstallOpts {
    /// Skip matching tasks (comma-separated)
    #[arg(long, value_delimiter = ',', value_name = "TASKS")]
    pub skip: Vec<String>,

    /// Run only matching tasks (comma-separated)
    #[arg(long, value_delimiter = ',', value_name = "TASKS")]
    pub only: Vec<String>,
}

/// Options for the `update` subcommand.
///
/// `update` runs the same task graph as `install` and therefore accepts the
/// same task selectors.  It is an alias of [`InstallOpts`] so the two commands
/// share one option type and one filtering implementation.
pub type UpdateOpts = InstallOpts;

/// Options for the `uninstall` subcommand.
#[derive(Parser, Debug, Clone)]
pub struct UninstallOpts;

/// Options for the `test` subcommand.
#[derive(Parser, Debug, Clone)]
pub struct TestOpts;

/// Options for the `log` subcommand.
#[derive(Parser, Debug, Clone)]
pub struct LogOpts;

/// Options for the `completions` subcommand.
#[derive(Parser, Debug, Clone)]
pub struct CompletionsOpts {
    /// Target shell
    #[arg(value_enum)]
    pub shell: clap_complete::Shell,
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use clap::{CommandFactory, error::ErrorKind};

    fn display_output(args: &[&str], expected_kind: ErrorKind) -> String {
        let error = Cli::try_parse_from(args.iter().copied())
            .expect_err("display arguments should stop normal parsing");
        assert_eq!(
            error.kind(),
            expected_kind,
            "display argument should return the expected clap result"
        );
        error.to_string()
    }

    #[test]
    fn verify_cli() {
        Cli::command().debug_assert();
    }

    #[test]
    fn standard_help_flags_are_enabled() {
        for flag in ["-h", "--help"] {
            let help = display_output(&["dotfiles", flag], ErrorKind::DisplayHelp);
            assert!(
                help.starts_with("Manage system configuration from this dotfiles repository\n"),
                "{flag} should show top-level help without a version header"
            );
        }
    }

    #[test]
    fn version_flag_is_long_only() {
        let version = display_output(&["dotfiles", "--version"], ErrorKind::DisplayVersion);
        let command = Cli::command();
        let expected_version = command.get_version().expect("CLI should define a version");
        assert_eq!(
            version,
            format!("dotfiles {expected_version}\n"),
            "--version should use clap's standard version output"
        );

        let error = Cli::try_parse_from(["dotfiles", "-V"])
            .expect_err("-V should not be accepted as a version flag");
        assert_eq!(
            error.kind(),
            ErrorKind::UnknownArgument,
            "-V should remain unavailable"
        );
    }

    #[test]
    fn help_and_version_subcommands_are_disabled() {
        for subcommand in ["help", "version"] {
            let error = Cli::try_parse_from(["dotfiles", subcommand])
                .expect_err("removed subcommands should be rejected");
            assert_eq!(
                error.kind(),
                ErrorKind::InvalidSubcommand,
                "{subcommand} should not be a user-visible subcommand"
            );
        }

        let command = Cli::command();
        assert!(
            command
                .get_subcommands()
                .all(|subcommand| !matches!(subcommand.get_name(), "help" | "version")),
            "help and version should not appear in the command model"
        );
    }

    #[test]
    fn top_level_help_uses_user_facing_copy_and_examples() {
        let help = display_output(&["dotfiles", "--help"], ErrorKind::DisplayHelp);

        for description in [
            "install    Apply dotfiles and system configuration",
            "update     Apply configuration and advance pinned dependencies",
            "uninstall  Remove managed integrations while preserving user files",
            "test       Validate configuration and run self-tests",
            "log        Show the latest run log",
            "-v, --verbose            Show complete task and action details",
            "-p, --profile <PROFILE>  Use a specific profile",
            "-d, --dry-run            Preview changes without applying them",
            "--root <PATH>        Use PATH as the dotfiles repository",
            "--overlay <PATH>     Merge configuration from an overlay repository",
            "--no-parallel        Run tasks sequentially",
            "-h, --help               Print help",
        ] {
            assert!(
                help.contains(description),
                "top-level help should contain: {description}"
            );
        }

        assert!(
            help.contains(
                "Examples:\n  dotfiles install\n  dotfiles install --dry-run\n  \
                 dotfiles install --only symlinks\n  dotfiles test"
            ),
            "top-level help should show the concise examples"
        );
        assert!(
            !help.contains("Cross-platform dotfiles management engine"),
            "implementation-oriented copy should be removed"
        );
    }

    #[test]
    fn install_and_update_help_use_concise_task_selectors() {
        for command in ["install", "update"] {
            let help = display_output(&["dotfiles", command, "--help"], ErrorKind::DisplayHelp);
            assert!(
                help.contains("--only <TASKS>       Run only matching tasks (comma-separated)"),
                "{command} help should describe --only in one line"
            );
            assert!(
                help.contains("--skip <TASKS>       Skip matching tasks (comma-separated)"),
                "{command} help should describe --skip in one line"
            );
            assert!(
                !help.contains("case-insensitive selector match"),
                "{command} help should omit selector implementation details"
            );
        }
    }

    #[test]
    fn parse_install_with_profile() {
        let cli = Cli::parse_from(["dotfiles", "--profile", "arch", "install"]);
        assert_eq!(cli.global.profile, Some("arch".to_string()));
        assert!(matches!(cli.command, Command::Install(_)));
    }

    #[test]
    fn parse_install_with_profile_short() {
        let cli = Cli::parse_from(["dotfiles", "-p", "arch", "install"]);
        assert_eq!(cli.global.profile, Some("arch".to_string()));
        assert!(matches!(cli.command, Command::Install(_)));
    }

    #[test]
    fn parse_install_dry_run() {
        let cli = Cli::parse_from(["dotfiles", "--dry-run", "install"]);
        assert!(cli.global.dry_run);
    }

    #[test]
    fn parse_install_dry_run_short() {
        let cli = Cli::parse_from(["dotfiles", "-d", "install"]);
        assert!(cli.global.dry_run);
    }

    #[test]
    fn parse_install_skip_tasks() {
        let cli = Cli::parse_from(["dotfiles", "install", "--skip", "packages,fonts"]);
        assert!(
            matches!(&cli.command, Command::Install(_)),
            "Expected Install command"
        );
        if let Command::Install(opts) = cli.command {
            assert_eq!(opts.skip, vec!["packages", "fonts"]);
        }
    }

    #[test]
    fn parse_install_only_tasks() {
        let cli = Cli::parse_from(["dotfiles", "install", "--only", "symlinks"]);
        assert!(
            matches!(&cli.command, Command::Install(_)),
            "Expected Install command"
        );
        if let Command::Install(opts) = cli.command {
            assert_eq!(opts.only, vec!["symlinks"]);
        }
    }

    #[test]
    fn parse_verbose() {
        let cli = Cli::parse_from(["dotfiles", "-v", "install"]);
        assert!(cli.verbose);
    }

    #[test]
    fn parse_uninstall() {
        let cli = Cli::parse_from(["dotfiles", "uninstall"]);
        assert!(matches!(cli.command, Command::Uninstall(_)));
    }

    #[test]
    fn parse_update() {
        let cli = Cli::parse_from(["dotfiles", "update"]);
        assert!(matches!(cli.command, Command::Update(_)));
    }

    #[test]
    fn parse_update_with_profile() {
        let cli = Cli::parse_from(["dotfiles", "--profile", "desktop", "update"]);
        assert_eq!(cli.global.profile, Some("desktop".to_string()));
        assert!(matches!(cli.command, Command::Update(_)));
    }

    #[test]
    fn parse_update_dry_run() {
        let cli = Cli::parse_from(["dotfiles", "-d", "update"]);
        assert!(cli.global.dry_run);
        assert!(matches!(cli.command, Command::Update(_)));
    }

    #[test]
    fn parse_update_skip_tasks() {
        let cli = Cli::parse_from(["dotfiles", "update", "--skip", "packages"]);
        assert!(
            matches!(&cli.command, Command::Update(_)),
            "Expected Update command"
        );
        if let Command::Update(opts) = cli.command {
            assert_eq!(opts.skip, vec!["packages"]);
        }
    }

    #[test]
    fn parse_update_only_tasks() {
        let cli = Cli::parse_from(["dotfiles", "update", "--only", "apm"]);
        assert!(
            matches!(&cli.command, Command::Update(_)),
            "Expected Update command"
        );
        if let Command::Update(opts) = cli.command {
            assert_eq!(opts.only, vec!["apm"]);
        }
    }

    #[test]
    fn parse_test() {
        let cli = Cli::parse_from(["dotfiles", "test"]);
        assert!(matches!(cli.command, Command::Test(_)));
    }

    #[test]
    fn parse_log() {
        let cli = Cli::parse_from(["dotfiles", "log"]);
        assert!(matches!(cli.command, Command::Log(_)));
    }

    #[test]
    fn parse_log_verbose() {
        let cli = Cli::parse_from(["dotfiles", "log", "--verbose"]);
        assert!(cli.verbose);
        assert!(matches!(cli.command, Command::Log(_)));
    }

    #[test]
    fn parse_root_override() {
        let cli = Cli::parse_from(["dotfiles", "--root", "/tmp/dotfiles", "install"]);
        assert_eq!(
            cli.global.root,
            Some(std::path::PathBuf::from("/tmp/dotfiles"))
        );
    }

    #[test]
    fn parallel_is_enabled_by_default() {
        let cli = Cli::parse_from(["dotfiles", "install"]);
        assert!(cli.global.parallel, "parallel should be true by default");
    }

    #[test]
    fn no_parallel_disables_parallel() {
        let cli = Cli::parse_from(["dotfiles", "--no-parallel", "install"]);
        assert!(
            !cli.global.parallel,
            "--no-parallel should set parallel to false"
        );
    }

    #[test]
    fn parse_completions_bash() {
        let cli = Cli::parse_from(["dotfiles", "completions", "bash"]);
        assert!(matches!(cli.command, Command::Completions(_)));
        if let Command::Completions(opts) = cli.command {
            assert_eq!(opts.shell, clap_complete::Shell::Bash);
        }
    }

    #[test]
    fn parse_completions_zsh() {
        let cli = Cli::parse_from(["dotfiles", "completions", "zsh"]);
        if let Command::Completions(opts) = cli.command {
            assert_eq!(opts.shell, clap_complete::Shell::Zsh);
        }
    }

    #[test]
    fn parse_completions_fish() {
        let cli = Cli::parse_from(["dotfiles", "completions", "fish"]);
        if let Command::Completions(opts) = cli.command {
            assert_eq!(opts.shell, clap_complete::Shell::Fish);
        }
    }
}
