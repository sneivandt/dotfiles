//! Pure resource plan/diff construction.
//!
//! This module maps observed [`ResourceState`] values plus processing options
//! into typed plans.  The plans are side-effect free: they can be unit-tested,
//! rendered for dry-run output, and then handed to the apply layer for mutation.

use super::mode::{ProcessOpts, ResourceAction};
use crate::resources::ResourceState;

/// Planned operation for installing or updating one resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ApplyOperation<'a> {
    /// Resource is already in the desired state.
    Noop,
    /// Resource should not be changed.
    Skip {
        /// Human-readable reason for skipping the resource.
        reason: String,
        /// Whether the skip is a non-fatal resource failure.
        failed: bool,
    },
    /// Resource should be applied.
    Apply {
        /// Human-facing verb for log and dry-run output.
        verb: &'a str,
        /// Existing value when replacing an incorrect resource.
        current: Option<String>,
        /// Whether apply errors should abort the enclosing task.
        bail_on_error: bool,
    },
}

/// Side-effect-free plan for applying one resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApplyChange<'a> {
    description: String,
    operation: ApplyOperation<'a>,
}

impl<'a> ApplyChange<'a> {
    /// Build an apply plan from a resource description, current state, and processing options.
    #[must_use]
    pub(crate) fn from_state(
        description: String,
        state: &ResourceState,
        opts: &ProcessOpts<'a>,
    ) -> Self {
        let operation = match opts.mode.action_for(state) {
            ResourceAction::Noop => ApplyOperation::Noop,
            ResourceAction::Skip { reason, failed } => ApplyOperation::Skip { reason, failed },
            ResourceAction::Apply => ApplyOperation::Apply {
                verb: opts.verb,
                current: incorrect_current(state),
                bail_on_error: opts.mode.bail_on_error(),
            },
        };
        Self {
            description,
            operation,
        }
    }

    /// Human-readable resource description captured when the plan was built.
    #[must_use]
    pub(crate) fn description(&self) -> &str {
        &self.description
    }

    /// Planned operation.
    #[must_use]
    pub(crate) const fn operation(&self) -> &ApplyOperation<'a> {
        &self.operation
    }

    /// Dry-run message for an apply operation, if the plan would mutate state.
    #[must_use]
    pub(crate) fn dry_run_message(&self) -> Option<String> {
        match &self.operation {
            ApplyOperation::Apply {
                verb,
                current: Some(current),
                ..
            } => Some(format!(
                "  would {verb}: {} (currently {current})",
                self.description
            )),
            ApplyOperation::Apply {
                verb,
                current: None,
                ..
            } => Some(format!("  would {verb}: {}", self.description)),
            ApplyOperation::Noop | ApplyOperation::Skip { .. } => None,
        }
    }
}

/// Planned operation for removing one resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RemoveOperation<'a> {
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
        verb: &'a str,
    },
}

/// Side-effect-free plan for removing one resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoveChange<'a> {
    description: String,
    operation: RemoveOperation<'a>,
}

impl<'a> RemoveChange<'a> {
    /// Build a remove plan from a resource description and current state.
    #[must_use]
    pub(crate) fn from_state(description: String, state: &ResourceState, verb: &'a str) -> Self {
        let operation = match state {
            ResourceState::Correct => RemoveOperation::Remove { verb },
            ResourceState::Unknown { reason } => RemoveOperation::Skip {
                reason: format!("state unknown ({reason})"),
            },
            ResourceState::Missing
            | ResourceState::Incorrect { .. }
            | ResourceState::Invalid { .. } => RemoveOperation::Noop,
        };
        Self {
            description,
            operation,
        }
    }

    /// Human-readable resource description captured when the plan was built.
    #[must_use]
    pub(crate) fn description(&self) -> &str {
        &self.description
    }

    /// Planned operation.
    #[must_use]
    pub(crate) const fn operation(&self) -> &RemoveOperation<'a> {
        &self.operation
    }

    /// Dry-run message for a remove operation, if the plan would mutate state.
    #[must_use]
    pub(crate) fn dry_run_message(&self) -> Option<String> {
        match &self.operation {
            RemoveOperation::Remove { verb } => {
                Some(format!("  would {verb}: {}", self.description))
            }
            RemoveOperation::Noop | RemoveOperation::Skip { .. } => None,
        }
    }
}

fn incorrect_current(state: &ResourceState) -> Option<String> {
    match state {
        ResourceState::Incorrect { current } => Some(current.clone()),
        ResourceState::Missing
        | ResourceState::Correct
        | ResourceState::Invalid { .. }
        | ResourceState::Unknown { .. } => None,
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::engine::{ProcessMode, ProcessOpts};

    #[test]
    fn apply_plan_noops_for_correct_state() {
        let plan = ApplyChange::from_state(
            "thing".to_string(),
            &ResourceState::Correct,
            &ProcessOpts::strict("install"),
        );

        assert_eq!(plan.operation(), &ApplyOperation::Noop);
        assert!(plan.dry_run_message().is_none());
    }

    #[test]
    fn apply_plan_skips_invalid_state_with_reason() {
        let plan = ApplyChange::from_state(
            "thing".to_string(),
            &ResourceState::Invalid {
                reason: "bad target".to_string(),
            },
            &ProcessOpts::strict("install"),
        );

        assert_eq!(
            plan.operation(),
            &ApplyOperation::Skip {
                reason: "bad target".to_string(),
                failed: true,
            }
        );
    }

    #[test]
    fn apply_plan_captures_missing_apply_verb_and_bail_mode() {
        let plan = ApplyChange::from_state(
            "thing".to_string(),
            &ResourceState::Missing,
            &ProcessOpts::strict("install"),
        );

        assert_eq!(
            plan.operation(),
            &ApplyOperation::Apply {
                verb: "install",
                current: None,
                bail_on_error: true,
            }
        );
        assert_eq!(
            plan.dry_run_message().unwrap(),
            "  would install: thing".to_string()
        );
    }

    #[test]
    fn apply_plan_captures_incorrect_current_value() {
        let plan = ApplyChange::from_state(
            "thing".to_string(),
            &ResourceState::Incorrect {
                current: "old".to_string(),
            },
            &ProcessOpts::lenient("replace"),
        );

        assert_eq!(
            plan.operation(),
            &ApplyOperation::Apply {
                verb: "replace",
                current: Some("old".to_string()),
                bail_on_error: false,
            }
        );
        assert_eq!(
            plan.dry_run_message().unwrap(),
            "  would replace: thing (currently old)".to_string()
        );
    }

    #[test]
    fn apply_plan_respects_install_missing_mode() {
        let opts = ProcessOpts {
            verb: "install",
            mode: ProcessMode::InstallMissing,
            sequential: false,
        };
        let plan = ApplyChange::from_state(
            "thing".to_string(),
            &ResourceState::Incorrect {
                current: "old".to_string(),
            },
            &opts,
        );

        assert!(matches!(
            plan.operation(),
            ApplyOperation::Skip { reason, failed: false } if reason.contains("incorrect")
        ));
    }

    #[test]
    fn remove_plan_removes_only_correct_resources() {
        let plan = RemoveChange::from_state("thing".to_string(), &ResourceState::Correct, "unlink");

        assert_eq!(
            plan.operation(),
            &RemoveOperation::Remove { verb: "unlink" }
        );
        assert_eq!(
            plan.dry_run_message().unwrap(),
            "  would unlink: thing".to_string()
        );
    }

    #[test]
    fn remove_plan_noops_for_unmanaged_resources() {
        let plan = RemoveChange::from_state("thing".to_string(), &ResourceState::Missing, "unlink");

        assert_eq!(plan.operation(), &RemoveOperation::Noop);
    }

    #[test]
    fn remove_plan_skips_unknown_resources() {
        let plan = RemoveChange::from_state(
            "thing".to_string(),
            &ResourceState::Unknown {
                reason: "tool missing".to_string(),
            },
            "unlink",
        );

        assert_eq!(
            plan.operation(),
            &RemoveOperation::Skip {
                reason: "state unknown (tool missing)".to_string()
            }
        );
    }
}
