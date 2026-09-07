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
            ("✓", "CHANGE", TextStyle::Green)
        }
        TaskStatus::Changed | TaskStatus::Passed => ("✓", "PASSED", TextStyle::Green),
        TaskStatus::DryRun => ("~", "DRYRUN", TextStyle::Magenta),
        TaskStatus::Skipped => ("⊘", "SKIPPED", TextStyle::Yellow),
        TaskStatus::Blocked => ("⊘", "BLOCKED", TextStyle::Yellow),
        TaskStatus::Interrupted => ("⊘", "INTERRUPTED", TextStyle::Yellow),
        TaskStatus::Failed => ("✗", "FAILED", TextStyle::Red),
        TaskStatus::Ok => ("○", "OK", TextStyle::Dim),
        TaskStatus::NotApplicable => ("⁃", "N/A", TextStyle::Dim),
    };
    (if symbols { symbol } else { word }, style)
}
