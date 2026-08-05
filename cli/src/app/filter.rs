//! Task filter matching helpers for `install --only` and `install --skip`.

use crate::engine::Task;
use crate::infra::logging::Output;
use crate::infra::logging::OutputExt as _;

/// Return filters that do not match any known task.
pub(crate) fn unmatched_filters<'a>(tasks: &[&dyn Task], filters: &'a [String]) -> Vec<&'a str> {
    filters
        .iter()
        .filter(|filter| !tasks.iter().any(|task| task_matches_filter(*task, filter)))
        .map(String::as_str)
        .collect()
}

/// Warn for each previously identified unmatched filter.
pub(crate) fn warn_unmatched_filters(filters: &[&str], flag: &str, log: &dyn Output) {
    for filter in filters {
        log.warn(format!("{flag} '{filter}' did not match any task"));
    }
}

/// Return whether a task passes both the inclusion and exclusion filters.
#[must_use]
pub(crate) fn task_passes_filters(task: &dyn Task, only: &[String], skip: &[String]) -> bool {
    let included = only.is_empty() || only.iter().any(|filter| task_matches_filter(task, filter));
    let excluded = skip.iter().any(|filter| task_matches_filter(task, filter));
    included && !excluded
}

/// Return whether a task matches a user-supplied selector.
///
/// Exact normalized display-name matching is retained for compatibility, but
/// selector IDs are the authoritative interface.
#[must_use]
pub fn task_matches_filter(task: &dyn Task, filter: &str) -> bool {
    let normalized_filter = normalize_task_filter(filter);
    if normalized_filter.is_empty() {
        return false;
    }

    normalized_filter == normalize_task_filter(task.selector())
        || normalized_filter == normalize_task_filter(task.name())
}

fn normalize_task_filter(value: &str) -> String {
    normalized_task_tokens(value).join("-")
}

fn normalized_task_tokens(value: &str) -> Vec<String> {
    value
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Context, TaskMeta, TaskResult};
    use crate::infra::logging::MsgKind;
    use anyhow::Result;
    use std::borrow::Cow;
    use std::sync::Mutex;

    struct SampleTask;

    impl Task for SampleTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("Home symlinks").with_selector("symlinks")
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }
    }

    struct OtherTask;

    impl Task for OtherTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("System packages").with_selector("packages")
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }
    }

    /// Collects warnings so filter diagnostics can be asserted directly.
    #[derive(Debug, Default)]
    struct RecordingOutput {
        warnings: Mutex<Vec<String>>,
    }

    impl RecordingOutput {
        fn warnings(&self) -> Vec<String> {
            self.warnings
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }
    }

    impl Output for RecordingOutput {
        fn emit(&self, kind: MsgKind, msg: Cow<'_, str>) {
            if kind == MsgKind::Warn {
                self.warnings
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(msg.into_owned());
            }
        }
    }

    #[test]
    fn task_matches_filter_uses_explicit_selector() {
        let task = SampleTask;
        assert!(task_matches_filter(&task, "symlinks"));
        assert!(task_matches_filter(&task, "Home symlinks"));
        assert!(!task_matches_filter(&task, "home"));
        assert!(!task_matches_filter(&task, "install-symlinks"));
    }

    #[test]
    fn selector_matching_normalizes_punctuation_and_case() {
        let task = SampleTask;
        assert!(task_matches_filter(&task, "HOME_SYMLINKS"));
    }

    #[test]
    fn blank_filters_never_match() {
        let task = SampleTask;
        for filter in ["", "   ", "--", "//"] {
            assert!(
                !task_matches_filter(&task, filter),
                "filter {filter:?} normalizes to nothing and must not match"
            );
        }
    }

    #[test]
    fn task_passes_filters_combines_only_and_skip() {
        let only = vec!["symlinks".to_string()];
        let task = SampleTask;
        assert!(task_passes_filters(&task, &only, &[]));

        let skip = vec!["symlinks".to_string()];
        assert!(!task_passes_filters(&task, &only, &skip));
    }

    #[test]
    fn empty_only_includes_every_task_not_skipped() {
        assert!(
            task_passes_filters(&SampleTask, &[], &[]),
            "no filters should keep every task"
        );
        assert!(
            task_passes_filters(&SampleTask, &[], &["packages".to_string()]),
            "skipping another task must not exclude this one"
        );
        assert!(
            !task_passes_filters(&SampleTask, &["packages".to_string()], &[]),
            "an --only filter naming another task must exclude this one"
        );
    }

    #[test]
    fn unmatched_filters_returns_only_unknown_selectors() {
        let tasks: [&dyn Task; 2] = [&SampleTask, &OtherTask];
        assert_eq!(
            unmatched_filters(&tasks, &["symlinks".to_string(), "packages".to_string()]),
            Vec::<&str>::new(),
            "known selectors should not be returned"
        );
        assert_eq!(
            unmatched_filters(&tasks, &["symlinks".to_string(), "typo".to_string()]),
            vec!["typo"],
            "only unknown selectors should be returned"
        );
        assert_eq!(
            unmatched_filters(&tasks, &[]),
            Vec::<&str>::new(),
            "an empty filter list has nothing to mismatch"
        );
    }

    #[test]
    fn warn_unmatched_filters_reports_each_unknown_filter_once() {
        let tasks: [&dyn Task; 2] = [&SampleTask, &OtherTask];
        let log = RecordingOutput::default();

        let filters = [
            "symlinks".to_string(),
            "typo".to_string(),
            "nope".to_string(),
        ];
        let unmatched = unmatched_filters(&tasks, &filters);
        warn_unmatched_filters(&unmatched, "--only", &log);

        assert_eq!(
            log.warnings(),
            vec![
                "--only 'typo' did not match any task".to_string(),
                "--only 'nope' did not match any task".to_string(),
            ],
            "only unmatched filters should warn, preserving user order"
        );
    }
}
