//! Presentation mapping for recorded task outcomes.

use crate::infra::logging::style::TextStyle;
use crate::infra::logging::types::TaskStatus;

use super::totals::SummaryMode;

pub(super) const fn presentation(
    status: TaskStatus,
    mode: SummaryMode,
    symbols: bool,
) -> (&'static str, TextStyle) {
    let (symbol, word, style) = match status {
        TaskStatus::Changed if matches!(mode, SummaryMode::Standard) => {
            ("✓", "CHANGED", TextStyle::Green)
        }
        TaskStatus::Changed | TaskStatus::Passed => ("✓", "PASSED", TextStyle::Green),
        TaskStatus::DryRun => ("~", "PLANNED", TextStyle::Magenta),
        TaskStatus::Skipped => ("⊘", "SKIPPED", TextStyle::Yellow),
        TaskStatus::Blocked => ("⊘", "BLOCKED", TextStyle::Yellow),
        TaskStatus::Interrupted => ("⊘", "INTERRUPTED", TextStyle::Yellow),
        TaskStatus::Failed => ("✗", "FAILED", TextStyle::Red),
        TaskStatus::Ok => ("○", "CURRENT", TextStyle::Dim),
        TaskStatus::NotApplicable => ("⁃", "N/A", TextStyle::Dim),
    };
    (if symbols { symbol } else { word }, style)
}

/// A symbol alone does not distinguish exceptional states or identify a plan.
pub(super) const fn annotation(status: TaskStatus) -> Option<(&'static str, TextStyle)> {
    match status {
        TaskStatus::DryRun => Some(("planned", TextStyle::Magenta)),
        TaskStatus::Skipped => Some(("skipped", TextStyle::Yellow)),
        TaskStatus::Blocked => Some(("blocked", TextStyle::Yellow)),
        TaskStatus::Interrupted => Some(("interrupted", TextStyle::Yellow)),
        TaskStatus::Changed
        | TaskStatus::Passed
        | TaskStatus::Failed
        | TaskStatus::Ok
        | TaskStatus::NotApplicable => None,
    }
}
