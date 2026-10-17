//! Pure resource plan/diff construction.
//!
//! This module maps observed [`ResourceState`] values plus processing options
//! into typed plans.  The plans are side-effect free: they can be unit-tested,
//! rendered for dry-run output, and then handed to the apply layer for mutation.

use super::mode::ProcessOpts;
use crate::engine::{ResourceState, SkipKind};

/// Planned operation for installing or updating one resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ApplyOperation {
    /// Resource is already in the desired state.
    Noop,
    /// Resource should not be changed.
    Skip {
        /// Human-readable reason for skipping the resource.
        reason: String,
        /// Whether the skip is benign or represents unmet work.
        kind: SkipKind,
    },
    /// Resource should be applied.
    Apply {
        /// Human-facing verb for log and dry-run output.
        verb: &'static str,
        /// Existing value when replacing an incorrect resource.
        current: Option<String>,
        /// Whether apply errors should abort the enclosing task.
        bail_on_error: bool,
    },
}

impl ApplyOperation {
    /// Choose an apply operation from current state and processing options.
    ///
    /// This is the resource lifecycle state machine: it maps every combination
    /// of [`ResourceState`] × [`ProcessMode`](super::ProcessMode) to a concrete
    /// [`ApplyOperation`].
    #[must_use]
    pub(crate) fn from_state(state: &ResourceState, opts: &ProcessOpts) -> Self {
        let apply = |current| Self::Apply {
            verb: opts.verb,
            current,
            bail_on_error: opts.mode.bail_on_error(),
        };
        match state {
            ResourceState::Correct => Self::Noop,
            ResourceState::Invalid { reason } => Self::Skip {
                reason: reason.clone(),
                kind: SkipKind::UnmetWork,
            },
            ResourceState::Unknown { reason } => Self::Skip {
                reason: format!("state unknown: {reason}"),
                kind: SkipKind::UnmetWork,
            },
            ResourceState::Missing if opts.mode.fix_missing() => apply(None),
            ResourceState::Missing => Self::Skip {
                reason: "mode skips missing resources".into(),
                kind: SkipKind::Benign,
            },
            ResourceState::Incorrect { current } if opts.mode.fix_incorrect() => {
                apply(Some(current.clone()))
            }
            ResourceState::Incorrect { .. } => Self::Skip {
                reason: "mode skips incorrect resources".into(),
                kind: SkipKind::Benign,
            },
        }
    }

    /// Dry-run message for an apply operation, if the plan would mutate state.
    #[must_use]
    pub(crate) fn dry_run_message(&self, description: &str) -> Option<String> {
        match self {
            Self::Apply {
                verb,
                current: Some(current),
                ..
            } => Some(format!("{verb} {description} (currently {current})")),
            Self::Apply {
                verb,
                current: None,
                ..
            } => Some(format!("{verb} {description}")),
            Self::Noop | Self::Skip { .. } => None,
        }
    }
}

/// Planned operation for removing one resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RemoveOperation {
    /// Resource is absent, not managed by us, or otherwise does not need removal.
    Noop,
    /// Resource should not be removed.
    Skip {
        /// Human-readable reason for skipping removal.
        reason: String,
    },
    /// Resource should be removed.
    Remove {
        /// Human-facing verb for log and dry-run output.
        verb: &'static str,
    },
}

impl RemoveOperation {
    /// Choose a remove operation from current state.
    #[must_use]
    pub(crate) fn from_state(state: &ResourceState, verb: &'static str) -> Self {
        match state {
            ResourceState::Correct => Self::Remove { verb },
            ResourceState::Unknown { reason } => Self::Skip {
                reason: format!("state unknown ({reason})"),
            },
            ResourceState::Missing
            | ResourceState::Incorrect { .. }
            | ResourceState::Invalid { .. } => Self::Noop,
        }
    }

    /// Dry-run message for a remove operation, if the plan would mutate state.
    #[must_use]
    pub(crate) fn dry_run_message(&self, description: &str) -> Option<String> {
        match self {
            Self::Remove { verb } => Some(format!("{verb} {description}")),
            Self::Noop | Self::Skip { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{ProcessMode, ProcessOpts};

    #[test]
    fn state_machine_matrix_covers_every_mode_and_state() {
        let cases = [
            (ProcessMode::Strict, true, true),
            (ProcessMode::Lenient, true, true),
            (ProcessMode::InstallMissing, false, true),
            (ProcessMode::FixExisting, true, false),
        ];

        for (mode, fixes_incorrect, fixes_missing) in cases {
            let opts = ProcessOpts {
                verb: "install",
                mode,
                sequential: false,
            };
            let plan_for = |state: &ResourceState| ApplyOperation::from_state(state, &opts);

            assert_eq!(
                plan_for(&ResourceState::Correct),
                ApplyOperation::Noop,
                "mode {mode:?}, state Correct",
            );
            assert_eq!(
                plan_for(&ResourceState::Invalid {
                    reason: "invalid target".to_string(),
                }),
                ApplyOperation::Skip {
                    reason: "invalid target".to_string(),
                    kind: SkipKind::UnmetWork,
                },
                "mode {mode:?}, state Invalid",
            );
            assert_eq!(
                plan_for(&ResourceState::Unknown {
                    reason: "tool missing".to_string(),
                }),
                ApplyOperation::Skip {
                    reason: "state unknown: tool missing".to_string(),
                    kind: SkipKind::UnmetWork,
                },
                "mode {mode:?}, state Unknown",
            );
            assert_eq!(
                plan_for(&ResourceState::Missing),
                if fixes_missing {
                    ApplyOperation::Apply {
                        verb: "install",
                        current: None,
                        bail_on_error: mode.bail_on_error(),
                    }
                } else {
                    ApplyOperation::Skip {
                        reason: "mode skips missing resources".to_string(),
                        kind: SkipKind::Benign,
                    }
                },
                "mode {mode:?}, state Missing",
            );
            assert_eq!(
                plan_for(&ResourceState::Incorrect {
                    current: "old".to_string(),
                }),
                if fixes_incorrect {
                    ApplyOperation::Apply {
                        verb: "install",
                        current: Some("old".to_string()),
                        bail_on_error: mode.bail_on_error(),
                    }
                } else {
                    ApplyOperation::Skip {
                        reason: "mode skips incorrect resources".to_string(),
                        kind: SkipKind::Benign,
                    }
                },
                "mode {mode:?}, state Incorrect",
            );
        }
    }

    #[test]
    fn apply_plan_noops_for_correct_state() {
        let plan =
            ApplyOperation::from_state(&ResourceState::Correct, &ProcessOpts::strict("install"));

        assert_eq!(plan, ApplyOperation::Noop);
        assert!(plan.dry_run_message("thing").is_none());
    }

    #[test]
    fn apply_plan_captures_missing_apply_verb_and_bail_mode() {
        let plan =
            ApplyOperation::from_state(&ResourceState::Missing, &ProcessOpts::strict("install"));

        assert_eq!(
            plan,
            ApplyOperation::Apply {
                verb: "install",
                current: None,
                bail_on_error: true,
            }
        );
        assert_eq!(
            plan.dry_run_message("thing").unwrap(),
            "install thing".to_string()
        );
    }

    #[test]
    fn apply_plan_captures_incorrect_current_value() {
        let plan = ApplyOperation::from_state(
            &ResourceState::Incorrect {
                current: "old".to_string(),
            },
            &ProcessOpts::lenient("replace"),
        );

        assert_eq!(
            plan,
            ApplyOperation::Apply {
                verb: "replace",
                current: Some("old".to_string()),
                bail_on_error: false,
            }
        );
        assert_eq!(
            plan.dry_run_message("thing").unwrap(),
            "replace thing (currently old)".to_string()
        );
    }

    #[test]
    fn remove_plan_removes_only_correct_resources() {
        let plan = RemoveOperation::from_state(&ResourceState::Correct, "unlink");

        assert_eq!(plan, RemoveOperation::Remove { verb: "unlink" });
        assert_eq!(
            plan.dry_run_message("thing").unwrap(),
            "unlink thing".to_string()
        );
    }

    #[test]
    fn remove_plan_noops_for_unmanaged_resources() {
        let plan = RemoveOperation::from_state(&ResourceState::Missing, "unlink");

        assert_eq!(plan, RemoveOperation::Noop);
    }

    #[test]
    fn remove_plan_skips_unknown_resources() {
        let plan = RemoveOperation::from_state(
            &ResourceState::Unknown {
                reason: "tool missing".to_string(),
            },
            "unlink",
        );

        assert_eq!(
            plan,
            RemoveOperation::Skip {
                reason: "state unknown (tool missing)".to_string()
            }
        );
    }
}
