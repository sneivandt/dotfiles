//! CLI argument definitions and top-level argument parsing.

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Top-level CLI entry point for the dotfiles management engine.
#[derive(Parser, Debug)]
#[command(
    name = "dotfiles",
    about = "Manage dotfiles and system configuration",
    version = option_env!("DOTFILES_VERSION").unwrap_or(concat!("dev-", env!("CARGO_PKG_VERSION"))),
    disable_version_flag = true,
    propagate_version = true,
    override_usage = "dotfiles [COMMAND] [OPTIONS]",
    after_help = "\
Default command: update

Examples:
  dotfiles update
  dotfiles update --dry-run
  dotfiles update --only symlinks
  dotfiles check
  dotfiles list

Options may appear before or after a command.
The launcher may download the CLI before help or previews.",
    help_template = "\
{about-with-newline}
{usage-heading} {usage}

{all-args}{after-help}
"
)]
pub struct Cli {
    /// Internal linkage for restarted and elevated processes.
    #[arg(long, hide = true, global = true)]
    pub parent_run_id: Option<String>,
    /// Subcommand to execute.
    #[command(subcommand)]
    pub command: Command,

    /// Print version
    #[arg(short = 'V', long, global = true, action = clap::ArgAction::Version)]
    pub version: Option<bool>,
}

impl Cli {
    /// Parse arguments, defaulting to update when the command is omitted.
    pub(crate) fn parse() -> Self {
        Self::parse_from(std::env::args_os())
    }

    /// Parse an explicit argument sequence with the same default command.
    pub(crate) fn parse_from<I, T>(args: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        Self::try_parse_from(args).unwrap_or_else(|error| error.exit())
    }

    /// Parse arguments without exiting on invalid input.
    pub(crate) fn try_parse_from<I, T>(args: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let mut args: Vec<std::ffi::OsString> = args.into_iter().map(Into::into).collect();
        normalize_command_position(&mut args);
        <Self as Parser>::try_parse_from(args)
    }
}

/// Move an explicit command ahead of its options, skipping option values.
/// Derive value-taking options from Clap so command names used as values stay values.
pub(super) fn normalize_command_position(args: &mut Vec<std::ffi::OsString>) {
    use clap::CommandFactory as _;
    let command = Cli::command();
    let options: Vec<_> = command
        .get_arguments()
        .chain(
            command
                .get_subcommands()
                .flat_map(clap::Command::get_arguments),
        )
        .collect();
    let mut position = 1;
    while let Some(value) = args.get(position) {
        let Some(arg) = value.to_str() else { break };
        if arg == "--" {
            break;
        }
        if !arg.starts_with('-') {
            let explicit = args.remove(position);
            args.insert(1, explicit);
            return;
        }
        let takes_next = arg.strip_prefix("--").map_or_else(
            || {
                let mut shorts = arg.trim_start_matches('-').chars();
                while let Some(short) = shorts.next() {
                    if options.iter().any(|option| {
                        option.get_short() == Some(short) && option.get_action().takes_values()
                    }) {
                        return shorts.as_str().is_empty();
                    }
                }
                false
            },
            |long| {
                !long.contains('=')
                    && options.iter().any(|option| {
                        option.get_long() == Some(long) && option.get_action().takes_values()
                    })
            },
        );
        position = position.saturating_add(if takes_next { 2 } else { 1 });
    }
    let top_level_display = args
        .iter()
        .skip(1)
        .any(|arg| matches!(arg.to_str(), Some("--help" | "-h" | "--version" | "-V")))
        && args.len() == 2;
    if !top_level_display {
        args.insert(1.min(args.len()), "update".into());
    }
}

/// Available subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Apply configuration and update dependencies
    #[command(after_help = "Startup may check for CLI updates before running tasks.
Dry runs may query tools and save logs, caches, or selections.
Use list with an existing binary for read-only discovery.
JSON Lines output disables prompts.")]
    Update(UpdateCommandOpts),

    /// Remove managed links and integrations
    #[command(after_help = "\
Removes home links, Git hooks, the launcher, and overlay script state.
Preserves packages, services, registry values, and shell selection.

Startup may check for CLI updates before running tasks.
Dry runs may query tools and save logs, caches, or selections.
JSON Lines output disables prompts.")]
    Remove(RemoveCommandOpts),

    /// Validate configuration and repository files
    #[command(
        after_help = "Checks can write run logs and save configuration selections.
Missing optional tools are skipped; --fail-on-skip makes them fatal.
JSON Lines output disables prompts."
    )]
    Check(CheckCommandOpts),

    /// Inspect task selectors and dependencies
    List(TasksOpts),

    /// Inspect previous runs and task diagnostics
    Log(LogOpts),

    /// Generate shell completions for the given shell
    #[command(hide = true)]
    Completions(CompletionsOpts),
}

/// Repository and profile options used by configuration-aware commands.
#[derive(Args, Debug, Clone, Default)]
pub struct RepositoryOpts {
    /// Select a configuration profile
    #[arg(
        short,
        long,
        value_name = "PROFILE",
        add = clap_complete::ArgValueCandidates::new(crate::app::completion::profile_candidates)
    )]
    pub profile: Option<String>,

    /// Read configuration from this repository
    #[arg(long, value_name = "PATH")]
    pub root: Option<std::path::PathBuf>,

    /// Merge configuration from this overlay
    #[arg(long, value_name = "PATH")]
    pub overlay: Option<std::path::PathBuf>,
}

/// Execution output protocol. JSON Lines supports streamed and restarted runs.
#[derive(Debug, Clone, Copy, Default, ValueEnum, PartialEq, Eq)]
pub enum ExecutionFormat {
    /// Human-readable task output.
    #[default]
    Human,
    /// Versioned run and task records, one JSON object per line.
    Jsonl,
}

/// Options shared by commands that execute a task graph.
#[derive(Args, Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "CLI switches are independent user choices rather than state-machine states"
)]
pub struct ExecutionOpts {
    /// Show additional task diagnostics
    #[arg(short, long, conflicts_with = "quiet")]
    pub verbose: bool,

    /// Show only warnings and failures
    #[arg(short = 'q', long, conflicts_with = "format")]
    pub quiet: bool,

    /// Choose human or jsonl output
    #[arg(
        long,
        value_enum,
        default_value_t,
        hide_possible_values = true,
        conflicts_with = "quiet"
    )]
    pub format: ExecutionFormat,

    /// Run selected tasks sequentially
    #[arg(long = "no-parallel", action = clap::ArgAction::SetFalse)]
    pub parallel: bool,

    /// Fail if applicable tasks are skipped
    #[arg(long = "fail-on-skip")]
    pub require_complete: bool,

    /// Disable prompts and fail if input is needed
    #[arg(long)]
    pub non_interactive: bool,

    /// Use text labels for task status
    #[arg(long)]
    pub no_symbols: bool,
}

impl Default for ExecutionOpts {
    fn default() -> Self {
        Self {
            verbose: false,
            quiet: false,
            format: ExecutionFormat::Human,
            parallel: true,
            require_complete: false,
            non_interactive: false,
            no_symbols: false,
        }
    }
}

/// Options for the `update` command.
#[derive(Args, Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "CLI switches are independent user choices rather than state-machine states"
)]
pub struct UpdateCommandOpts {
    /// Repository and profile selection.
    #[command(flatten, next_help_heading = "Repository")]
    pub repository: RepositoryOpts,

    /// Task execution policy.
    #[command(flatten, next_help_heading = "Execution and output")]
    pub execution: ExecutionOpts,

    /// Task selection.
    #[command(flatten, next_help_heading = "Task selection")]
    pub tasks: InstallOpts,

    /// Preview changes without applying them
    #[arg(short = 'n', long, help_heading = "Mutation policy")]
    pub dry_run: bool,

    /// Use the current checkout without syncing
    #[arg(long, help_heading = "Mutation policy")]
    pub no_repo_update: bool,

    /// Skip CLI update provenance verification
    #[arg(long, help_heading = "Mutation policy")]
    pub skip_attestation: bool,

    /// Internal marker for a child process that performs elevated tasks.
    #[arg(long = "elevated-child", hide = true)]
    pub elevated_child: bool,
}

/// Options for the `check` command.
#[derive(Args, Debug, Clone)]
#[command(group(clap::ArgGroup::new("CheckOpts").args(["skip", "only"]).multiple(true)))]
pub struct CheckCommandOpts {
    /// Repository and profile selection.
    #[command(flatten, next_help_heading = "Repository")]
    pub repository: RepositoryOpts,

    /// Task execution policy.
    #[command(flatten, next_help_heading = "Execution and output")]
    pub execution: ExecutionOpts,

    /// Check selection.
    #[command(flatten, next_help_heading = "Task selection")]
    pub tasks: CheckOpts,
}

/// Options for the `remove` command.
#[derive(Args, Debug, Clone)]
#[command(group(clap::ArgGroup::new("CheckOpts").args(["skip", "only"]).multiple(true)))]
pub struct RemoveCommandOpts {
    /// Repository and profile selection.
    #[command(flatten, next_help_heading = "Repository")]
    pub repository: RepositoryOpts,

    /// Task execution policy.
    #[command(flatten, next_help_heading = "Execution and output")]
    pub execution: ExecutionOpts,

    /// Task selection.
    #[command(flatten, next_help_heading = "Task selection")]
    pub tasks: UninstallOpts,

    /// Preview changes without applying them
    #[arg(short = 'n', long, help_heading = "Mutation policy")]
    pub dry_run: bool,

    /// Skip CLI update provenance verification
    #[arg(long, help_heading = "Mutation policy")]
    pub skip_attestation: bool,

    /// Internal marker for a child process that performs elevated tasks.
    #[arg(long = "elevated-child", hide = true)]
    pub elevated_child: bool,
}

/// Output format for tabular listing commands.
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

/// Command whose task dependency graph is shown by `list --graph`.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskGraphCommand {
    /// Configuration and dependency updates.
    Update,
    /// Removal of managed integrations.
    Remove,
    /// Repository validation.
    Check,
}

impl TaskGraphCommand {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Update => "update",
            Self::Remove => "remove",
            Self::Check => "check",
        }
    }
}

/// Options for the `list` command.
#[derive(Args, Debug, Clone)]
pub struct TasksOpts {
    /// Repository and profile selection.
    #[command(flatten, next_help_heading = "Repository")]
    pub repository: RepositoryOpts,

    /// Choose table, plain, or json output
    #[arg(long, value_enum, default_value_t)]
    pub format: DiscoveryFormat,

    /// Include tasks for other platforms or profiles
    #[arg(long, conflicts_with = "graph")]
    pub all: bool,

    /// Show dependencies for a command
    #[arg(long, value_enum, value_name = "COMMAND")]
    pub graph: Option<TaskGraphCommand>,

    /// Mark selected tasks in the dependency graph
    #[arg(
        long,
        value_delimiter = ',',
        value_name = "SELECTOR",
        requires = "graph"
    )]
    pub only: Vec<String>,

    /// Mark excluded tasks in the dependency graph
    #[arg(
        long,
        value_delimiter = ',',
        value_name = "SELECTOR",
        requires = "graph"
    )]
    pub skip: Vec<String>,

    /// Include dependencies of selected tasks
    #[arg(long, requires = "only")]
    pub with_deps: bool,
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
    /// Console output policy.
    pub output_mode: crate::infra::logging::OutputMode,
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
            output_mode: if execution.quiet {
                crate::infra::logging::OutputMode::Quiet
            } else if execution.format == ExecutionFormat::Jsonl {
                crate::infra::logging::OutputMode::Jsonl
            } else {
                crate::infra::logging::OutputMode::Human
            },
            skip_attestation: false,
            elevated_child: false,
        }
    }
}

impl UpdateCommandOpts {
    /// Split parsed options into engine context, task filters, and output policy.
    #[must_use]
    pub fn into_engine_parts(self) -> (GlobalOpts, InstallOpts, bool, bool) {
        let verbose = self.execution.verbose;
        let update_pins = true;
        let global = GlobalOpts {
            dry_run: self.dry_run,
            no_repo_update: self.no_repo_update,
            skip_attestation: self.skip_attestation,
            elevated_child: self.elevated_child,
            ..GlobalOpts::from_execution(self.repository, &self.execution)
        };
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

impl RemoveCommandOpts {
    /// Split parsed options into engine context and output policy.
    #[must_use]
    pub fn into_engine_parts(self) -> (GlobalOpts, UninstallOpts, bool) {
        let verbose = self.execution.verbose;
        let global = GlobalOpts {
            dry_run: self.dry_run,
            skip_attestation: self.skip_attestation,
            elevated_child: self.elevated_child,
            ..GlobalOpts::from_execution(self.repository, &self.execution)
        };
        (global, self.tasks, verbose)
    }
}

/// Subcommands that drive the task engine.
#[derive(Debug)]
pub enum EngineCommand {
    /// Apply dotfiles and system configuration.
    Update {
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
    Remove {
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
            Self::Update { .. } => "update",
            Self::Remove { .. } => "remove",
            Self::Check { .. } => "check",
        }
    }

    /// Shared execution options.
    #[must_use]
    pub const fn global(&self) -> &GlobalOpts {
        match self {
            Self::Update { global, .. }
            | Self::Remove { global, .. }
            | Self::Check { global, .. } => global,
        }
    }

    /// Whether diagnostic output is enabled.
    #[must_use]
    pub const fn verbose(&self) -> bool {
        match self {
            Self::Update { verbose, .. }
            | Self::Remove { verbose, .. }
            | Self::Check { verbose, .. } => *verbose,
        }
    }
}

/// Task selection for `update`.
#[derive(Args, Debug, Clone, Default)]
#[group(args = ["skip", "only", "with_deps"])]
pub struct InstallOpts {
    /// Shared task selectors.
    #[command(flatten)]
    pub filters: TaskFilters,

    /// Include dependencies of selected tasks
    #[arg(long, requires = "only")]
    pub with_deps: bool,
}

/// Task filters shared by commands that execute a task graph.
#[derive(Args, Debug, Clone, Default)]
// Command-specific containers retain the existing argument-group identities.
#[group(skip)]
pub struct TaskFilters {
    /// Skip selected tasks (repeat or use commas)
    #[arg(
        long,
        value_delimiter = ',',
        value_name = "SELECTOR",
        add = clap_complete::ArgValueCandidates::new(crate::app::completion::task_candidates)
    )]
    pub skip: Vec<String>,

    /// Run selected tasks (repeat or use commas)
    #[arg(
        long,
        value_delimiter = ',',
        value_name = "SELECTOR",
        add = clap_complete::ArgValueCandidates::new(crate::app::completion::task_candidates)
    )]
    pub only: Vec<String>,
}

/// Options for the `check` task set.
pub type CheckOpts = TaskFilters;

/// Options for the `uninstall` task set.
pub type UninstallOpts = TaskFilters;

/// Canonical command names accepted by `log --command`.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum LogCommand {
    /// Installation runs.
    Install,
    /// Configuration and dependency update runs.
    Update,
    /// Managed integration removal runs.
    Remove,
    /// Historical uninstallation runs.
    Uninstall,
    /// Validation runs, including legacy `test` runs.
    Check,
}

impl LogCommand {
    /// Whether a stored command belongs to this canonical command family.
    #[must_use]
    pub fn matches(self, stored: &str) -> bool {
        match self {
            Self::Install => stored == "install",
            Self::Update => stored == "update",
            Self::Remove => stored == "remove",
            Self::Uninstall => stored == "uninstall",
            Self::Check => matches!(stored, "check" | "test"),
        }
    }
}

/// Options for the `log` subcommand.
#[derive(Args, Debug, Clone)]
pub struct LogOpts {
    /// Select a run by index (0 is the latest)
    #[arg(value_name = "RUN", conflicts_with = "list")]
    pub run: Option<usize>,

    /// Select a run by its exact identifier
    #[arg(
        long,
        value_name = "ID",
        conflicts_with_all = ["run", "list", "command"]
    )]
    pub id: Option<String>,

    /// Filter by task selector or stored identity
    #[arg(long, value_name = "SELECTOR", conflicts_with = "list")]
    pub task: Option<String>,

    /// Show original records and diagnostics
    #[arg(long, conflicts_with = "list")]
    pub raw: bool,

    /// List previous runs and their outcomes
    #[arg(short, long)]
    pub list: bool,

    /// Choose table, plain, or json list output
    #[arg(long, value_enum, requires = "list")]
    pub format: Option<DiscoveryFormat>,

    /// Filter runs by their command
    #[arg(short, long, value_name = "COMMAND", value_enum)]
    pub command: Option<LogCommand>,

    /// Show additional task diagnostics
    #[arg(short, long, conflicts_with_all = ["list", "raw"])]
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
    fn verify_cli() {
        Cli::command().debug_assert();
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
            panic!("expected install command");
        };
        assert!(opts.execution.verbose);
    }

    #[test]
    fn top_level_help_is_small_and_uses_canonical_commands() {
        let help = display_output(&["dotfiles", "--help"], ErrorKind::DisplayHelp);

        for text in [
            "update  Apply configuration and update dependencies",
            "remove  Remove managed links and integrations",
            "check   Validate configuration and repository files",
            "list    Inspect task selectors and dependencies",
            "log     Inspect previous runs and task diagnostics",
            "help    Print this message or the help of the given subcommand(s)",
            "dotfiles check",
        ] {
            assert!(
                help.contains(text),
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
    fn install_accepts_scoped_options_and_new_names() {
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
            panic!("expected install command");
        };
        assert_eq!(opts.repository.profile.as_deref(), Some("desktop"));
        assert!(opts.dry_run);
        assert!(opts.no_repo_update);
        assert!(opts.execution.require_complete);
        assert_eq!(opts.tasks.filters.only, ["symlinks", "git-hooks"]);
        assert!(opts.tasks.with_deps);
    }

    #[test]
    fn install_rejects_removed_option_names() {
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
    fn removed_test_command_is_rejected() {
        let error =
            Cli::try_parse_from(["dotfiles", "test"]).expect_err("test command was removed");
        assert_eq!(error.kind(), ErrorKind::InvalidSubcommand);
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
                Command::Remove(opts) => (opts.tasks.only, opts.tasks.skip),
                Command::List(_) | Command::Log(_) | Command::Completions(_) => {
                    panic!("expected execution command")
                }
            };
            assert_eq!(only, ["symlinks", "git-hooks", "apm"], "{name}");
            assert_eq!(skip, ["packages", "repository", "chmod"], "{name}");
        }
    }

    #[test]
    fn execution_commands_keep_dependency_expansion_scoped_to_install_and_update() {
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
                            panic!("expected install or update");
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
                ("InstallOpts", vec!["skip", "only", "with_deps"])
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
        for text in [
            "dotfiles [COMMAND] [OPTIONS]",
            "Default command: update",
            "download the CLI",
        ] {
            assert!(help.contains(text), "{text}: {help}");
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
                    Command::List(_) | Command::Log(_) | Command::Completions(_) => {
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
    fn uninstall_help_states_what_remains() {
        let help = display_output(&["dotfiles", "remove", "--help"], ErrorKind::DisplayHelp);
        assert!(help.contains("Preserves packages, services, registry values"));
        assert!(help.contains("-n, --dry-run"));
        assert!(help.contains("--only <SELECTOR>"));
        assert!(!help.contains("--with-deps"));
        assert!(!help.contains("--no-repo-update"));
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

        let Command::Log(opts) = Cli::parse_from(["dotfiles", "log", "-c", "update"]).command
        else {
            panic!("expected log command");
        };
        assert_eq!(opts.command, Some(LogCommand::Update));
    }

    #[test]
    fn help_subcommand_uses_conventional_clap_behavior() {
        let help = display_output(&["dotfiles", "help", "update"], ErrorKind::DisplayHelp);
        assert!(help.contains("Usage: dotfiles update [OPTIONS]"));
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
                    let (global, _, update_pins, verbose) = opts.into_engine_parts();
                    assert_eq!(update_pins, name == "update");
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
                Command::List(_) | Command::Log(_) | Command::Completions(_) => {
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
}
