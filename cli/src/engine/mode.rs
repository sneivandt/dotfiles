//! Processing strategy and action types for the resource lifecycle state machine.

use crate::resources::ResourceState;

/// Processing strategy that determines how each [`ResourceState`] variant is handled.
///
/// Each variant encodes a specific combination of behaviours — which states
/// are fixable and whether errors are fatal — so the intent is explicit
/// without reasoning about individual boolean flags.
///
/// # Examples
///
/// ```
/// use dotfiles_cli::tasks::ProcessMode;
///
/// let strict = ProcessMode::Strict;
/// assert!(strict.fix_incorrect() && strict.fix_missing() && strict.bail_on_error());
///
/// let lenient = ProcessMode::Lenient;
/// assert!(lenient.fix_incorrect() && lenient.fix_missing() && !lenient.bail_on_error());
///
/// let missing_only = ProcessMode::InstallMissing;
/// assert!(!missing_only.fix_incorrect() && missing_only.fix_missing());
///
/// let existing_only = ProcessMode::FixExisting;
/// assert!(existing_only.fix_incorrect() && !existing_only.fix_missing());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessMode {
    /// Fix both missing and incorrect resources, bailing on errors.
    ///
    /// Use for resources where every failure must be surfaced (e.g. symlinks,
    /// hooks, git config).
    Strict,
    /// Fix both missing and incorrect resources, warning on errors instead of bailing.
    ///
    /// Use for resources where individual failures should not abort the batch
    /// (e.g. packages via winget, registry entries, developer mode).
    Lenient,
    /// Install only missing resources, warning on errors.
    ///
    /// Suitable for resources that should not be overwritten when already
    /// present (e.g. VS Code extensions, systemd units, Copilot skills).
    InstallMissing,
    /// Fix only incorrect resources (skip missing), bailing on errors.
    ///
    /// Use for resources where missing state is expected and only existing
    /// items need correction (e.g. chmod on files that may not exist yet).
    FixExisting,
}

impl ProcessMode {
    /// Whether `Incorrect` resources should be fixed.
    #[must_use]
    pub const fn fix_incorrect(self) -> bool {
        matches!(self, Self::Strict | Self::Lenient | Self::FixExisting)
    }

    /// Whether `Missing` resources should be created.
    #[must_use]
    pub const fn fix_missing(self) -> bool {
        matches!(self, Self::Strict | Self::Lenient | Self::InstallMissing)
    }

    /// Whether errors from `apply()` should propagate (bail).
    ///
    /// When `false`, errors are logged as warnings and counted as skipped.
    #[must_use]
    pub const fn bail_on_error(self) -> bool {
        matches!(self, Self::Strict | Self::FixExisting)
    }

    /// Determine the action to take for a resource in the given state.
    ///
    /// This encodes the state machine transition logic, mapping every
    /// combination of [`ResourceState`] × [`ProcessMode`] to a concrete
    /// [`ResourceAction`].
    ///
    /// # Examples
    ///
    /// ```
    /// use dotfiles_cli::resources::ResourceState;
    /// use dotfiles_cli::tasks::{ProcessMode, ResourceAction};
    ///
    /// assert_eq!(
    ///     ProcessMode::Strict.action_for(&ResourceState::Missing),
    ///     ResourceAction::Apply,
    /// );
    /// assert_eq!(
    ///     ProcessMode::Strict.action_for(&ResourceState::Correct),
    ///     ResourceAction::Noop,
    /// );
    /// assert!(matches!(
    ///     ProcessMode::InstallMissing.action_for(
    ///         &ResourceState::Incorrect { current: "x".into() },
    ///     ),
    ///     ResourceAction::Skip(_),
    /// ));
    /// ```
    #[must_use]
    pub fn action_for(self, state: &ResourceState) -> ResourceAction {
        match state {
            ResourceState::Correct => ResourceAction::Noop,
            ResourceState::Invalid { reason } => ResourceAction::Skip(reason.clone()),
            ResourceState::Unknown { reason } => {
                ResourceAction::Skip(format!("state unknown: {reason}"))
            }
            ResourceState::Missing if self.fix_missing() => ResourceAction::Apply,
            ResourceState::Missing => ResourceAction::Skip("mode skips missing resources".into()),
            ResourceState::Incorrect { .. } if self.fix_incorrect() => ResourceAction::Apply,
            ResourceState::Incorrect { .. } => {
                ResourceAction::Skip("mode skips incorrect resources".into())
            }
        }
    }
}

/// Action to take on a resource, as determined by [`ProcessMode::action_for`].
///
/// This enum encodes the output of the resource lifecycle state machine,
/// making the decision explicit and testable independently of the processing
/// loop.
///
/// # Examples
///
/// ```
/// use dotfiles_cli::tasks::ResourceAction;
///
/// let apply = ResourceAction::Apply;
/// let noop = ResourceAction::Noop;
/// let skip = ResourceAction::Skip("not applicable".into());
///
/// assert_eq!(apply, ResourceAction::Apply);
/// assert_ne!(apply, noop);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceAction {
    /// Apply the resource change (create or update).
    Apply,
    /// No action needed — resource is already in the desired state.
    Noop,
    /// Skip the resource for the given reason.
    Skip(String),
}

/// Configuration for the generic resource processing loop.
///
/// Pairs a [`ProcessMode`] with a human-readable verb for log messages.
///
/// Use the named constructors to express intent clearly:
///
/// # Examples
///
/// ```
/// use dotfiles_cli::tasks::ProcessOpts;
///
/// // Fix everything, bail on errors (strict):
/// let opts = ProcessOpts::strict("link");
/// assert!(opts.mode.fix_incorrect() && opts.mode.fix_missing() && opts.mode.bail_on_error());
///
/// // Fix everything, warn on errors (lenient):
/// let opts = ProcessOpts::lenient("install");
/// assert!(opts.mode.fix_incorrect() && opts.mode.fix_missing() && !opts.mode.bail_on_error());
///
/// // Install only missing resources (lenient):
/// let opts = ProcessOpts::install_missing("enable");
/// assert!(!opts.mode.fix_incorrect() && opts.mode.fix_missing() && !opts.mode.bail_on_error());
///
/// // Fix existing only, bail on errors:
/// let opts = ProcessOpts::fix_existing("apply permissions");
/// assert!(opts.mode.fix_incorrect() && !opts.mode.fix_missing() && opts.mode.bail_on_error());
/// ```
#[derive(Debug)]
pub struct ProcessOpts<'a> {
    /// Verb for log messages (e.g., "install", "link", "apply permissions").
    pub verb: &'a str,
    /// Processing strategy controlling which states are fixable and error behaviour.
    pub mode: ProcessMode,
    /// Force sequential processing regardless of `ctx.parallel`.
    ///
    /// Use for resources that share an exclusive file lock (e.g. git config),
    /// where parallel writes would race on the lock file.
    pub sequential: bool,
}

impl<'a> ProcessOpts<'a> {
    /// Fix both missing and incorrect resources, bailing on errors.
    ///
    /// This is the strict default — suitable for resources where every
    /// failure must be surfaced (e.g. symlinks, hooks, git config).
    #[must_use]
    pub const fn strict(verb: &'a str) -> Self {
        Self {
            verb,
            mode: ProcessMode::Strict,
            sequential: false,
        }
    }

    /// Fix both missing and incorrect resources, warning on errors.
    ///
    /// Suitable for resources where individual failures should not abort
    /// the batch (e.g. packages, registry entries).
    #[must_use]
    pub const fn lenient(verb: &'a str) -> Self {
        Self {
            verb,
            mode: ProcessMode::Lenient,
            sequential: false,
        }
    }

    /// Install only missing resources, warning on errors instead of bailing.
    ///
    /// Suitable for resources that should not be overwritten when already
    /// present (e.g. VS Code extensions, systemd units, Copilot skills).
    #[must_use]
    pub const fn install_missing(verb: &'a str) -> Self {
        Self {
            verb,
            mode: ProcessMode::InstallMissing,
            sequential: false,
        }
    }

    /// Fix only incorrect resources, bailing on errors.
    ///
    /// Skip missing resources — only fix existing items that have drifted.
    #[must_use]
    pub const fn fix_existing(verb: &'a str) -> Self {
        Self {
            verb,
            mode: ProcessMode::FixExisting,
            sequential: false,
        }
    }

    /// Force sequential processing regardless of the context parallel flag.
    ///
    /// Use for resources that share an exclusive file lock (e.g. git config),
    /// where parallel writes would race on the lock file.
    #[must_use]
    pub const fn sequential(mut self) -> Self {
        self.sequential = true;
        self
    }
}
