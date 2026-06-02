//! Parsing of APM `outdated` / `update` command output into typed decisions.

use super::{APM_UP_TO_DATE_MARKER, APM_UPDATE_NO_CHANGES_MARKER};

/// Result of checking the lockfile for stale dependencies.
pub(super) enum ApmOutdatedCheck {
    /// The check completed and reported whether dependencies are outdated.
    Outdated(bool),
    /// The check could not run because credentials are unavailable.
    Skipped(String),
}

/// Outcome of refreshing locked user-scope dependencies.
pub(super) enum ApmUpdateOutcome {
    /// `apm deps update` advanced at least one locked ref.
    Changed,
    /// `apm deps update` ran but every dependency was already current.
    Unchanged,
    /// The update could not run because credentials are unavailable.
    Skipped(String),
}

/// Return whether `apm outdated` reported any stale dependency.
pub(super) fn outdated_output_has_updates(stdout: &str, stderr: &str) -> bool {
    let output = format!("{stdout}\n{stderr}").to_lowercase();
    !output.contains(APM_UP_TO_DATE_MARKER)
}

/// Return whether `apm deps update` actually advanced any locked ref.
///
/// When every dependency is already current APM prints
/// `[*] All packages already at latest refs.`, so the absence of that marker
/// means at least one ref moved.  Dependencies pinned to git branch or commit
/// refs report an `unknown` status from `apm outdated`, which forces an update
/// attempt on every run; gating the change line on this marker keeps the
/// console quiet unless an update truly happened.
pub(super) fn update_output_made_changes(stdout: &str, stderr: &str) -> bool {
    let output = format!("{stdout}\n{stderr}").to_lowercase();
    !output.contains(APM_UPDATE_NO_CHANGES_MARKER)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;

    #[test]
    fn update_output_made_changes_detects_no_change_marker() {
        assert!(
            !update_output_made_changes("[*] All packages already at latest refs.\n", ""),
            "the no-change marker must suppress the update message"
        );
        assert!(
            update_output_made_changes("[*] Updated 2 APM dependencies in 0.8s.\n", ""),
            "a real update must report a change"
        );
    }
}
