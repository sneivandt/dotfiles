//! Processing strategy and action types for the resource lifecycle state machine.

/// Processing strategy that determines how each [`ResourceState`] variant is handled.
///
/// Each variant encodes a specific combination of behaviours — which states
/// are fixable and whether errors are fatal — so the intent is explicit
/// without reasoning about individual boolean flags.
///
/// # Examples
///
/// ```
/// use dotfiles_cli::testing::tasks::ProcessMode;
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
    /// present (e.g. VS Code extensions, systemd units, agent plugins).
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
    /// When `false`, errors are logged as warnings and counted as non-fatal failures.
    #[must_use]
    pub const fn bail_on_error(self) -> bool {
        matches!(self, Self::Strict | Self::FixExisting)
    }
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
/// use dotfiles_cli::testing::tasks::ProcessOpts;
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
/// let opts = ProcessOpts::fix_existing("configure");
/// assert!(opts.mode.fix_incorrect() && !opts.mode.fix_missing() && opts.mode.bail_on_error());
/// ```
#[derive(Debug)]
pub struct ProcessOpts {
    /// Verb for log messages — keep to the canonical set ("install",
    /// "configure", "update", "enable", "link", "unlink", "remove").
    pub verb: &'static str,
    /// Processing strategy controlling which states are fixable and error behaviour.
    pub mode: ProcessMode,
    /// Force sequential processing regardless of `ctx.parallel`.
    ///
    /// Use for resources that share an exclusive file lock (e.g. git config),
    /// where parallel writes would race on the lock file.
    pub sequential: bool,
}

impl ProcessOpts {
    /// Fix both missing and incorrect resources, bailing on errors.
    ///
    /// This is the strict default — suitable for resources where every
    /// failure must be surfaced (e.g. symlinks, hooks, git config).
    #[must_use]
    pub const fn strict(verb: &'static str) -> Self {
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
    pub const fn lenient(verb: &'static str) -> Self {
        Self {
            verb,
            mode: ProcessMode::Lenient,
            sequential: false,
        }
    }

    /// Install only missing resources, warning on errors instead of bailing.
    ///
    /// Suitable for resources that should not be overwritten when already
    /// present (e.g. VS Code extensions, systemd units, Copilot plugins).
    #[must_use]
    pub const fn install_missing(verb: &'static str) -> Self {
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
    pub const fn fix_existing(verb: &'static str) -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_matrix_covers_flags() {
        let cases = [
            (ProcessMode::Strict, true, true, true),
            (ProcessMode::Lenient, true, true, false),
            (ProcessMode::InstallMissing, false, true, false),
            (ProcessMode::FixExisting, true, false, true),
        ];

        for (mode, fixes_incorrect, fixes_missing, bails) in cases {
            assert_eq!(mode.fix_incorrect(), fixes_incorrect, "mode {mode:?}");
            assert_eq!(mode.fix_missing(), fixes_missing, "mode {mode:?}");
            assert_eq!(mode.bail_on_error(), bails, "mode {mode:?}");
        }
    }

    #[test]
    fn option_constructors_preserve_mode_verb_and_sequential_policy() {
        let cases = [
            (ProcessOpts::strict("link"), ProcessMode::Strict, "link"),
            (
                ProcessOpts::lenient("install"),
                ProcessMode::Lenient,
                "install",
            ),
            (
                ProcessOpts::install_missing("enable"),
                ProcessMode::InstallMissing,
                "enable",
            ),
            (
                ProcessOpts::fix_existing("configure"),
                ProcessMode::FixExisting,
                "configure",
            ),
        ];

        for (opts, expected_mode, expected_verb) in cases {
            assert_eq!(opts.mode, expected_mode);
            assert_eq!(opts.verb, expected_verb);
            assert!(!opts.sequential);
        }

        let sequential = ProcessOpts::strict("link").sequential();
        assert_eq!(sequential.mode, ProcessMode::Strict);
        assert_eq!(sequential.verb, "link");
        assert!(sequential.sequential);
    }
}
