//! Presentation mapping for recorded task outcomes.

use crate::infra::logging::style::TextStyle;
use crate::infra::logging::types::TaskStatus;

pub(super) const fn symbol(status: TaskStatus) -> char {
    match status {
        TaskStatus::Changed | TaskStatus::Passed => '✓',
        TaskStatus::DryRun => '~',
        TaskStatus::Skipped => '⊘',
        TaskStatus::Failed => '✗',
        TaskStatus::Ok => '○',
        TaskStatus::NotApplicable => '⁃',
    }
}

pub(super) const fn word(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Changed => "CHANGE",
        TaskStatus::DryRun => "DRYRUN",
        TaskStatus::Passed => "PASSED",
        TaskStatus::Skipped => "IGNORE",
        TaskStatus::Failed => "FAILED",
        TaskStatus::Ok => "OK",
        TaskStatus::NotApplicable => "N/A",
    }
}

pub(super) const fn text_style(status: TaskStatus) -> TextStyle {
    match status {
        TaskStatus::Changed | TaskStatus::Passed => TextStyle::Green,
        TaskStatus::Ok | TaskStatus::NotApplicable => TextStyle::Dim,
        TaskStatus::Skipped => TextStyle::Yellow,
        TaskStatus::DryRun => TextStyle::Magenta,
        TaskStatus::Failed => TextStyle::Red,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_symbols_are_single_characters() {
        for (status, expected) in [
            (TaskStatus::Changed, '✓'),
            (TaskStatus::DryRun, '~'),
            (TaskStatus::Passed, '✓'),
            (TaskStatus::Skipped, '⊘'),
            (TaskStatus::Failed, '✗'),
            (TaskStatus::Ok, '○'),
            (TaskStatus::NotApplicable, '⁃'),
        ] {
            assert_eq!(symbol(status), expected);
            assert_eq!(symbol(status).to_string().chars().count(), 1);
        }
    }
}
