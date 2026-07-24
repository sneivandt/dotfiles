//! Task filter matching helpers for `install --only` and `install --skip`.

use crate::engine::Task;
use crate::infra::logging::Output;

/// Warn when a filter does not match any known task.
pub(crate) fn warn_unmatched_filters(
    tasks: &[&dyn Task],
    filters: &[String],
    flag: &str,
    log: &dyn Output,
) {
    for filter in filters {
        let matched = tasks.iter().any(|task| task_matches_filter(*task, filter));
        if !matched {
            log.warn(&format!("{flag} '{filter}' did not match any task"));
        }
    }
}

/// Return whether a task passes both the inclusion and exclusion filters.
#[must_use]
pub(crate) fn task_passes_filters(task: &dyn Task, only: &[String], skip: &[String]) -> bool {
    let included = only.is_empty() || only.iter().any(|filter| task_matches_filter(task, filter));
    let excluded = skip.iter().any(|filter| task_matches_filter(task, filter));
    included && !excluded
}

/// Return whether any filter does not match a known task.
#[must_use]
pub(crate) fn has_unmatched_filter(tasks: &[&dyn Task], filters: &[String]) -> bool {
    filters
        .iter()
        .any(|filter| !tasks.iter().any(|task| task_matches_filter(*task, filter)))
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
    use crate::engine::{Context, TaskResult};
    use anyhow::Result;

    struct SampleTask;

    impl Task for SampleTask {
        fn name(&self) -> &'static str {
            "Home symlinks"
        }

        fn selector(&self) -> &'static str {
            "symlinks"
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
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
    fn task_passes_filters_combines_only_and_skip() {
        let only = vec!["symlinks".to_string()];
        let task = SampleTask;
        assert!(task_passes_filters(&task, &only, &[]));

        let skip = vec!["symlinks".to_string()];
        assert!(!task_passes_filters(&task, &only, &skip));
    }
}
