//! CLI argument definitions and top-level argument parsing.

use clap::{Args, CommandFactory as _, FromArgMatches as _, Parser, Subcommand, ValueEnum};

/// Top-level CLI entry point for the dotfiles management engine.
#[derive(Parser, Debug)]
#[command(
    name = "dotfiles",
    about = "Manage dotfiles and system configuration",
    version = option_env!("DOTFILES_VERSION").unwrap_or(concat!("dev-", env!("CARGO_PKG_VERSION"))),
    disable_version_flag = true,
    propagate_version = true,
    override_usage = "dotfiles [OPTIONS] [COMMAND]",
    after_help = "\
Default command: update

A task is a named group of work, such as installing packages or creating home symlinks.
Use 'dotfiles list' to find task selectors for --only and --skip.

Examples:
  dotfiles update
  dotfiles update --dry-run
  dotfiles update --only symlinks
  dotfiles check
  dotfiles list

Documentation: https://dotfiles.stuartneivandt.com/",
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
        let mut command = Self::command();
        normalize_command_position(&mut args, &command);
        let mut matches = command.try_get_matches_from_mut(args)?;
        Self::from_arg_matches_mut(&mut matches).map_err(|error| error.format(&mut command))
    }
}

/// Move an explicit command ahead of its options, skipping option values.
/// Derive value-taking options from Clap so command names used as values stay values.
pub(super) fn normalize_command_position(
    args: &mut Vec<std::ffi::OsString>,
    command: &clap::Command,
) {
    use super::cli_words::{Word, value_options, words};
    let raw = clap_lex::RawArgs::new(args.iter().skip(1));
    if let Some(position) = words(&raw, value_options(command)).find_map(|word| match word {
        Word::Positional(index, _) => Some(index + 1),
        Word::Option(_, _) => None,
    }) {
        let explicit = args.remove(position);
        args.insert(1, explicit);
        return;
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
    /// Sync the repository and apply configuration
    #[command(
        after_help = "Installs missing system packages. On Arch, installing one can also upgrade the system.
Startup may check for CLI updates before running tasks.
Dry runs may query tools and save logs, caches, or selections.
Use list with an existing binary for read-only discovery.
JSON Lines output disables prompts."
    )]
    Update(UpdateCommandOpts),

    /// Remove managed links and integrations
    #[command(after_help = "\
Replaces managed home symlinks with independent copies of their source contents.
Removes Git hooks, the launcher, and overlay script state.
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
    List(ListOpts),

    /// Explain how a configured home symlink is managed
    Explain(ExplainOpts),

    /// Inspect previous runs and task diagnostics
    Log(LogOpts),

    /// Generate shell completions for the given shell
    #[command(hide = true)]
    Completions(CompletionsOpts),

    /// Prepare a system file inside the scoped privileged installer.
    #[command(hide = true)]
    SystemFileHelper(SystemFileHelperOpts),
}

/// Internal arguments for the scoped system-file installer.
#[derive(Args, Debug)]
pub struct SystemFileHelperOpts {
    /// Verified destination entry.
    #[arg(long)]
    pub target: std::path::PathBuf,
    /// Fragment path read by the unelevated caller.
    #[arg(long)]
    pub fragment: std::path::PathBuf,
    /// SHA-256 fingerprint of the caller's fragment snapshot.
    #[arg(long)]
    pub fragment_hash: String,
    /// Configured merge strategy.
    #[arg(long)]
    pub merge: String,
    /// Private directory created beside the destination.
    #[arg(long)]
    pub stage: std::path::PathBuf,
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
pub struct UpdateCommandOpts {
    /// Repository and profile selection.
    #[command(flatten, next_help_heading = "Repository")]
    pub repository: RepositoryOpts,

    /// Task execution policy.
    #[command(flatten, next_help_heading = "Execution and output")]
    pub execution: ExecutionOpts,

    /// Task selection.
    #[command(flatten, next_help_heading = "Task selection")]
    pub tasks: UpdateOpts,

    /// Preview changes without applying them
    #[arg(short = 'n', long, help_heading = "Changes and safety")]
    pub dry_run: bool,

    /// Use the current checkout without syncing
    #[arg(long, help_heading = "Changes and safety")]
    pub no_repo_update: bool,

    /// Skip CLI update provenance verification
    #[arg(long, help_heading = "Changes and safety")]
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
    pub tasks: RemoveOpts,

    /// Preview changes without applying them
    #[arg(short = 'n', long, help_heading = "Changes and safety")]
    pub dry_run: bool,

    /// Skip CLI update provenance verification
    #[arg(long, help_heading = "Changes and safety")]
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

/// Options for inspecting one managed home target.
#[derive(Args, Debug, Clone)]
pub struct ExplainOpts {
    /// Repository and profile selection.
    #[command(flatten)]
    pub repository: RepositoryOpts,

    /// Absolute or home-relative target path.
    pub path: std::path::PathBuf,
}

/// Options for the `list` command.
#[derive(Args, Debug, Clone)]
pub struct ListOpts {
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
        requires = "graph",
        add = clap_complete::ArgValueCandidates::new(crate::app::completion::task_candidates)
    )]
    pub only: Vec<String>,

    /// Mark excluded tasks in the dependency graph
    #[arg(
        long,
        value_delimiter = ',',
        value_name = "SELECTOR",
        requires = "graph",
        add = clap_complete::ArgValueCandidates::new(crate::app::completion::task_candidates)
    )]
    pub skip: Vec<String>,

    /// Include dependencies of selected tasks
    #[arg(long, requires = "only")]
    pub with_deps: bool,
}

/// Options passed to the task engine after command-specific parsing.
#[derive(Debug, Clone)]
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
    pub fn into_engine_parts(self) -> (GlobalOpts, UpdateOpts, bool) {
        let verbose = self.execution.verbose;
        let global = GlobalOpts {
            dry_run: self.dry_run,
            no_repo_update: self.no_repo_update,
            skip_attestation: self.skip_attestation,
            elevated_child: self.elevated_child,
            ..GlobalOpts::from_execution(self.repository, &self.execution)
        };
        (global, self.tasks, verbose)
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
    pub fn into_engine_parts(self) -> (GlobalOpts, RemoveOpts, bool) {
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
        opts: UpdateOpts,
        /// Whether diagnostic output is enabled.
        verbose: bool,
    },
    /// Remove managed integrations.
    Remove {
        /// Shared task-engine options.
        global: GlobalOpts,
        /// Remove options.
        opts: RemoveOpts,
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
pub struct UpdateOpts {
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

/// Options for the `remove` task set.
#[derive(Args, Debug, Clone)]
pub struct RemoveOpts {
    /// Select removal tasks.
    #[command(flatten)]
    pub filters: TaskFilters,

    /// Also copy configuration to missing home paths
    #[arg(long)]
    pub restore_missing: bool,
}

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
#[path = "tests/cli.rs"]
mod tests;
