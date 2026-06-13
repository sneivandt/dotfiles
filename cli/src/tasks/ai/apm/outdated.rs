//! Parsing of APM `outdated` / `update` command output into typed decisions.

use super::APM_UP_TO_DATE_MARKER;

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
    fn outdated_output_has_updates_detects_up_to_date_marker() {
        assert!(
            !outdated_output_has_updates("[*] All dependencies are up-to-date.\n", ""),
            "the up-to-date marker must report no stale dependencies"
        );
        assert!(
            outdated_output_has_updates("github/foo  1.0.0  2.0.0  major\n", ""),
            "output without the up-to-date marker must report stale dependencies"
        );
    }
}
