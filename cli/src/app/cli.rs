//! CLI argument definitions and top-level argument parsing.

use clap::{Args, Parser, Subcommand, ValueEnum};

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
  dotfiles check",
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

    /// Print version
    #[arg(long, action = clap::ArgAction::Version)]
    pub version: Option<bool>,
}

/// Available subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Apply dotfiles and system configuration
    Install(InstallCommandOpts),

    /// Compatibility alias for `install --update-pins`
    #[command(hide = true)]
    Update(InstallCommandOpts),

    /// Remove managed integrations while preserving user files
    #[command(after_help = "\
Removes managed home symlinks, repository Git hooks, and the installed launcher.
Packages, services, registry values, shell selection, and overlay script effects remain.")]
    Uninstall(UninstallCommandOpts),

    /// Validate configuration and run repository checks
    Check(CheckCommandOpts),

    /// Compatibility alias for `check`
    #[command(hide = true)]
    Test(CheckCommandOpts),

    /// List task selectors and command membership
    Tasks(TasksOpts),

    /// List configured role profiles
    Profiles(ProfilesOpts),

    /// Show a retained run log
    Log(LogOpts),

    /// Generate shell completions for the given shell
    #[command(hide = true)]
    Completions(CompletionsOpts),
}

/// Repository and profile options used by configuration-aware commands.
#[derive(Args, Debug, Clone, Default)]
pub struct RepositoryOpts {
    /// Use a specific profile
    #[arg(short, long, value_name = "PROFILE")]
    pub profile: Option<String>,

    /// Use PATH as the dotfiles repository
    #[arg(long, value_name = "PATH")]
    pub root: Option<std::path::PathBuf>,

    /// Merge configuration from an overlay repository
    #[arg(long, value_name = "PATH")]
    pub overlay: Option<std::path::PathBuf>,
}

/// Options shared by commands that execute a task graph.
#[derive(Args, Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "CLI switches are independent user choices rather than state-machine states"
)]
pub struct ExecutionOpts {
    /// Show additional diagnostic task output
    #[arg(short, long)]
    pub verbose: bool,

    /// Run tasks sequentially
    #[arg(long = "no-parallel", action = clap::ArgAction::SetFalse)]
    pub parallel: bool,

    /// Fail when applicable work is skipped
    #[arg(long = "fail-on-skip")]
    pub require_complete: bool,

    /// Disable prompts and fail when input is required
    #[arg(long)]
    pub non_interactive: bool,

    /// Use ASCII words instead of status symbols
    #[arg(long)]
    pub no_symbols: bool,
}

impl Default for ExecutionOpts {
    fn default() -> Self {
        Self {
            verbose: false,
            parallel: true,
            require_complete: false,
            non_interactive: false,
            no_symbols: false,
        }
    }
}

/// Options for the `install` command.
#[derive(Args, Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "CLI switches are independent user choices rather than state-machine states"
)]
pub struct InstallCommandOpts {
    /// Repository and profile selection.
    #[command(flatten)]
    pub repository: RepositoryOpts,

    /// Task execution policy.
    #[command(flatten)]
    pub execution: ExecutionOpts,

    /// Task selection.
    #[command(flatten)]
    pub tasks: InstallOpts,

    /// Preview changes without applying them
    #[arg(short = 'n', long)]
    pub dry_run: bool,

    /// Advance pinned dependencies after normal convergence
    #[arg(long)]
    pub update_pins: bool,

    /// Use the current checkout without synchronizing its repository
    #[arg(long)]
    pub no_repo_update: bool,

    /// Skip self-update build provenance verification
    #[arg(long)]
    pub skip_attestation: bool,

    /// Internal marker for a child process that performs elevated tasks.
    #[arg(long = "elevated-child", hide = true)]
    pub elevated_child: bool,
}

/// Options for the `check` command.
#[derive(Args, Debug, Clone)]
pub struct CheckCommandOpts {
    /// Repository and profile selection.
    #[command(flatten)]
    pub repository: RepositoryOpts,

    /// Task execution policy.
    #[command(flatten)]
    pub execution: ExecutionOpts,

    /// Check selection.
    #[command(flatten)]
    pub tasks: CheckOpts,
}

/// Options for the `uninstall` command.
#[derive(Args, Debug, Clone)]
pub struct UninstallCommandOpts {
    /// Repository and profile selection.
    #[command(flatten)]
    pub repository: RepositoryOpts,

    /// Task execution policy.
    #[command(flatten)]
    pub execution: ExecutionOpts,

    /// Preview changes without applying them
    #[arg(short = 'n', long)]
    pub dry_run: bool,

    /// Skip self-update build provenance verification
    #[arg(long)]
    pub skip_attestation: bool,

    /// Internal marker for a child process that performs elevated tasks.
    #[arg(long = "elevated-child", hide = true)]
    pub elevated_child: bool,
}

/// Output format for discovery commands.
#[derive(Debug, Clone, Copy, Default, ValueEnum, PartialEq, Eq)]
pub enum DiscoveryFormat {
    /// Aligned columns with headings.
    #[default]
    Table,
    /// Tab-separated rows without headings.
    Plain,
    /// A JSON array of objects.
    Json,
}

/// Options for the `tasks` command.
#[derive(Args, Debug, Clone)]
pub struct TasksOpts {
    /// Repository and profile selection.
    #[command(flatten)]
    pub repository: RepositoryOpts,

    /// Output format
    #[arg(long, value_enum, default_value_t)]
    pub format: DiscoveryFormat,
}

/// Options for the `profiles` command.
#[derive(Args, Debug, Clone)]
pub struct ProfilesOpts {
    /// Use PATH as the dotfiles repository
    #[arg(long, value_name = "PATH")]
    pub root: Option<std::path::PathBuf>,
}

/// Options passed to the task engine after command-specific parsing.
#[derive(Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "Each independent execution switch maps directly to a bool"
)]
pub struct GlobalOpts {
    /// Selected profile.
    pub profile: Option<String>,
    /// Whether this is a dry run.
    pub dry_run: bool,
    /// Explicit repository root.
    pub root: Option<std::path::PathBuf>,
    /// Explicit overlay repository.
    pub overlay: Option<std::path::PathBuf>,
    /// Whether independent tasks may run in parallel.
    pub parallel: bool,
    /// Whether repository synchronization is disabled.
    pub no_repo_update: bool,
    /// Whether applicable skips fail the command.
    pub require_complete: bool,
    /// Whether prompts are disabled.
    pub non_interactive: bool,
    /// Whether status symbols are disabled.
    pub no_symbols: bool,
    /// Whether self-update attestation verification is disabled.
    pub skip_attestation: bool,
    /// Whether this process is an elevated child.
    pub elevated_child: bool,
}

impl GlobalOpts {
    fn from_execution(repository: RepositoryOpts, execution: &ExecutionOpts) -> Self {
        Self {
            profile: repository.profile,
            dry_run: false,
            root: repository.root,
            overlay: repository.overlay,
            parallel: execution.parallel,
            no_repo_update: false,
            require_complete: execution.require_complete,
            non_interactive: execution.non_interactive,
            no_symbols: execution.no_symbols,
            skip_attestation: false,
            elevated_child: false,
        }
    }
}

impl InstallCommandOpts {
    /// Split parsed options into engine context, task filters, and output policy.
    #[must_use]
    pub fn into_engine_parts(
        self,
        force_update_pins: bool,
    ) -> (GlobalOpts, InstallOpts, bool, bool) {
        let verbose = self.execution.verbose;
        let update_pins = self.update_pins || force_update_pins;
        let mut global = GlobalOpts::from_execution(self.repository, &self.execution);
        global.dry_run = self.dry_run;
        global.no_repo_update = self.no_repo_update;
        global.skip_attestation = self.skip_attestation;
        global.elevated_child = self.elevated_child;
        (global, self.tasks, update_pins, verbose)
    }
}

impl CheckCommandOpts {
    /// Split parsed options into engine context, task filters, and output policy.
    #[must_use]
    pub fn into_engine_parts(self) -> (GlobalOpts, CheckOpts, bool) {
        let verbose = self.execution.verbose;
        let global = GlobalOpts::from_execution(self.repository, &self.execution);
        (global, self.tasks, verbose)
    }
}

impl UninstallCommandOpts {
    /// Split parsed options into engine context and output policy.
    #[must_use]
    pub fn into_engine_parts(self) -> (GlobalOpts, UninstallOpts, bool) {
        let verbose = self.execution.verbose;
        let mut global = GlobalOpts::from_execution(self.repository, &self.execution);
        global.dry_run = self.dry_run;
        global.skip_attestation = self.skip_attestation;
        global.elevated_child = self.elevated_child;
        (global, UninstallOpts, verbose)
    }
}

/// Subcommands that drive the task engine.
#[derive(Debug)]
pub enum EngineCommand {
    /// Apply dotfiles and system configuration.
    Install {
        /// Shared task-engine options.
        global: GlobalOpts,
        /// Task selectors.
        opts: InstallOpts,
        /// Whether pinned dependencies should advance.
        update_pins: bool,
        /// Whether diagnostic output is enabled.
        verbose: bool,
    },
    /// Remove managed integrations.
    Uninstall {
        /// Shared task-engine options.
        global: GlobalOpts,
        /// Uninstall options.
        opts: UninstallOpts,
        /// Whether diagnostic output is enabled.
        verbose: bool,
    },
    /// Validate the repository configuration.
    Check {
        /// Shared task-engine options.
        global: GlobalOpts,
        /// Check selectors.
        opts: CheckOpts,
        /// Whether diagnostic output is enabled.
        verbose: bool,
    },
}

impl EngineCommand {
    /// Name used for run logs and progress output.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Install { .. } => "install",
            Self::Uninstall { .. } => "uninstall",
            Self::Check { .. } => "check",
        }
    }

    /// Shared execution options.
    #[must_use]
    pub const fn global(&self) -> &GlobalOpts {
        match self {
            Self::Install { global, .. }
            | Self::Uninstall { global, .. }
            | Self::Check { global, .. } => global,
        }
    }

    /// Whether diagnostic output is enabled.
    #[must_use]
    pub const fn verbose(&self) -> bool {
        match self {
            Self::Install { verbose, .. }
            | Self::Uninstall { verbose, .. }
            | Self::Check { verbose, .. } => *verbose,
        }
    }
}

/// Task filters shared by `install` and `check`.
#[derive(Args, Debug, Clone, Default)]
pub struct InstallOpts {
    /// Skip task selectors; repeat the option or separate values with commas
    #[arg(long, value_delimiter = ',', value_name = "SELECTOR")]
    pub skip: Vec<String>,

    /// Run only task selectors; repeat the option or separate values with commas
    #[arg(long, value_delimiter = ',', value_name = "SELECTOR")]
    pub only: Vec<String>,

    /// Include the dependency closure of tasks selected by `--only`
    #[arg(long, requires = "only")]
    pub with_deps: bool,
}

/// Options for the `check` task set.
pub type CheckOpts = InstallOpts;

/// Options for the `uninstall` task set.
#[derive(Debug, Clone, Copy)]
pub struct UninstallOpts;

/// Options for the `log` subcommand.
#[derive(Args, Debug, Clone)]
pub struct LogOpts {
    /// Run to show, newest first (0 is the latest run)
    #[arg(value_name = "RUN")]
    pub run: Option<usize>,

    /// List retained runs instead of showing one
    #[arg(short, long)]
    pub list: bool,

    /// Only consider runs of this command
    #[arg(short, long, value_name = "COMMAND")]
    pub command: Option<String>,

    /// Include diagnostic lines
    #[arg(short, long)]
    pub verbose: bool,
}

/// Options for the `completions` subcommand.
#[derive(Args, Debug, Clone)]
pub struct CompletionsOpts {
    /// Target shell
    #[arg(value_enum)]
    pub shell: clap_complete::Shell,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, error::ErrorKind};

    fn display_output(args: &[&str], expected_kind: ErrorKind) -> String {
        let error = Cli::try_parse_from(args.iter().copied())
            .expect_err("display arguments should stop normal parsing");
        assert_eq!(error.kind(), expected_kind);
        error.to_string()
    }

    #[test]
    fn verify_cli() {
        Cli::command().debug_assert();
    }

    #[test]
    fn version_is_long_only_and_verbose_remains_lowercase_v() {
        let version = display_output(&["dotfiles", "--version"], ErrorKind::DisplayVersion);
        let command = Cli::command();
        let expected = command.get_version().expect("CLI version");
        assert_eq!(version, format!("dotfiles {expected}\n"));

        for unsupported in ["-V", "--verbose"] {
            let error = Cli::try_parse_from(["dotfiles", unsupported])
                .expect_err("top-level option should be unavailable");
            assert_eq!(error.kind(), ErrorKind::UnknownArgument);
        }

        let cli = Cli::parse_from(["dotfiles", "install", "-v"]);
        let Command::Install(opts) = cli.command else {
            panic!("expected install command");
        };
        assert!(opts.execution.verbose);
    }

    #[test]
    fn top_level_help_is_small_and_uses_canonical_commands() {
        let help = display_output(&["dotfiles", "--help"], ErrorKind::DisplayHelp);

        for text in [
            "install    Apply dotfiles and system configuration",
            "uninstall  Remove managed integrations while preserving user files",
            "check      Validate configuration and run repository checks",
            "tasks      List task selectors and command membership",
            "profiles   List configured role profiles",
            "log        Show a retained run log",
            "dotfiles check",
        ] {
            assert!(
                help.contains(text),
                "top-level help should contain {text:?}"
            );
        }
        for hidden in ["update     ", "test       ", "--profile"] {
            assert!(
                !help.contains(hidden),
                "top-level help should omit {hidden:?}"
            );
        }
    }

    #[test]
    fn install_accepts_scoped_options_and_new_names() {
        let cli = Cli::parse_from([
            "dotfiles",
            "install",
            "-p",
            "desktop",
            "-n",
            "--update-pins",
            "--no-repo-update",
            "--fail-on-skip",
            "--only",
            "symlinks,git-hooks",
            "--with-deps",
        ]);
        let Command::Install(opts) = cli.command else {
            panic!("expected install command");
        };
        assert_eq!(opts.repository.profile.as_deref(), Some("desktop"));
        assert!(opts.dry_run);
        assert!(opts.update_pins);
        assert!(opts.no_repo_update);
        assert!(opts.execution.require_complete);
        assert_eq!(opts.tasks.only, ["symlinks", "git-hooks"]);
        assert!(opts.tasks.with_deps);
    }

    #[test]
    fn install_rejects_removed_option_names() {
        for old in ["-d", "--offline", "--require-complete", "--retry-failed"] {
            let error = Cli::try_parse_from(["dotfiles", "install", old])
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
            &["dotfiles", "uninstall", "--only", "symlinks"][..],
        ] {
            let error = Cli::try_parse_from(args.iter().copied())
                .expect_err("irrelevant option should fail during parsing");
            assert_eq!(error.kind(), ErrorKind::UnknownArgument, "{args:?}");
        }
    }

    #[test]
    fn compatibility_commands_parse_but_stay_hidden() {
        assert!(matches!(
            Cli::parse_from(["dotfiles", "update"]).command,
            Command::Update(_)
        ));
        assert!(matches!(
            Cli::parse_from(["dotfiles", "test"]).command,
            Command::Test(_)
        ));
    }

    #[test]
    fn check_accepts_task_filters_without_mutation_options() {
        let cli = Cli::parse_from([
            "dotfiles",
            "check",
            "--only",
            "config-warnings",
            "--skip",
            "shellcheck",
        ]);
        let Command::Check(opts) = cli.command else {
            panic!("expected check command");
        };
        assert_eq!(opts.tasks.only, ["config-warnings"]);
        assert_eq!(opts.tasks.skip, ["shellcheck"]);
    }

    #[test]
    fn tasks_and_profiles_have_discovery_options() {
        let tasks = Cli::parse_from(["dotfiles", "tasks", "--profile", "base", "--format", "json"]);
        let Command::Tasks(opts) = tasks.command else {
            panic!("expected tasks command");
        };
        assert_eq!(opts.repository.profile.as_deref(), Some("base"));
        assert_eq!(opts.format, DiscoveryFormat::Json);

        let profiles = Cli::parse_from(["dotfiles", "profiles", "--root", "/repo"]);
        assert!(matches!(profiles.command, Command::Profiles(_)));
    }

    #[test]
    fn uninstall_help_states_what_remains() {
        let help = display_output(&["dotfiles", "uninstall", "--help"], ErrorKind::DisplayHelp);
        assert!(help.contains("Packages, services, registry values"));
        assert!(help.contains("-n, --dry-run"));
        assert!(!help.contains("--no-repo-update"));
    }

    #[test]
    fn log_only_accepts_log_output_options() {
        let cli = Cli::parse_from(["dotfiles", "log", "2", "-c", "install", "-v"]);
        let Command::Log(opts) = cli.command else {
            panic!("expected log command");
        };
        assert_eq!(opts.run, Some(2));
        assert_eq!(opts.command.as_deref(), Some("install"));
        assert!(opts.verbose);
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
}
