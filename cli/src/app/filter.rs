//! Task filter matching helpers for `install --only` and `install --skip`.

use crate::infra::logging::Output;

use crate::engine::Task;

const TASK_FILTER_STOP_WORDS: &[&str] = &[
    "install",
    "configure",
    "enable",
    "apply",
    "update",
    "run",
    "validate",
];

/// Warn when a filter does not match any known task.
pub(crate) fn warn_unmatched_filters(
    tasks: &[&dyn Task],
    filters: &[String],
    flag: &str,
    log: &dyn Output,
) {
    for filter in filters {
        let matched = tasks
            .iter()
            .any(|task| task_matches_filter(task.name(), filter));
        if !matched {
            log.warn(&format!("{flag} '{filter}' did not match any task"));
        }
    }
}

/// Return whether a task passes both the inclusion and exclusion filters.
#[must_use]
pub(crate) fn task_passes_filters(task_name: &str, only: &[String], skip: &[String]) -> bool {
    let included = only.is_empty()
        || only
            .iter()
            .any(|filter| task_matches_filter(task_name, filter));
    let excluded = skip
        .iter()
        .any(|filter| task_matches_filter(task_name, filter));
    included && !excluded
}

/// Return whether any filter does not match a known task.
#[must_use]
pub(crate) fn has_unmatched_filter(tasks: &[&dyn Task], filters: &[String]) -> bool {
    filters.iter().any(|filter| {
        !tasks
            .iter()
            .any(|task| task_matches_filter(task.name(), filter))
    })
}

/// Return whether a task name matches a user-supplied selector.
#[must_use]
pub fn task_matches_filter(task_name: &str, filter: &str) -> bool {
    let normalized_filter = normalize_task_filter(filter);
    if normalized_filter.is_empty() {
        return false;
    }

    let canonical_selector = canonical_task_selector(task_name);

    normalized_filter == normalize_task_filter(task_name)
        || normalized_filter == canonical_selector
        || canonical_selector
            .split('-')
            .next()
            .is_some_and(|token| token == normalized_filter)
}

fn canonical_task_selector(task_name: &str) -> String {
    let tokens = normalized_task_tokens(task_name);
    let trimmed: Vec<_> = tokens
        .iter()
        .skip_while(|token| TASK_FILTER_STOP_WORDS.contains(&token.as_str()))
        .cloned()
        .collect();

    if trimmed.is_empty() {
        tokens.join("-")
    } else {
        trimmed.join("-")
    }
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

    #[test]
    fn task_matches_filter_uses_canonical_selector() {
        assert!(task_matches_filter("Install symlinks", "symlinks"));
        assert!(task_matches_filter("Update repository", "repository"));
        assert!(task_matches_filter("Reload configuration", "reload"));
        assert!(task_matches_filter(
            "Update repository",
            "update-repository"
        ));
        assert!(!task_matches_filter("Update repository", "update"));
    }

    #[test]
    fn canonical_task_selector_drops_leading_action_words() {
        assert_eq!(
            canonical_task_selector("Install AUR packages"),
            "aur-packages"
        );
        assert_eq!(canonical_task_selector("Configure Git"), "git");
        assert_eq!(canonical_task_selector("Update binary"), "binary");
        assert_eq!(
            canonical_task_selector("Reload configuration"),
            "reload-configuration"
        );
    }

    #[test]
    fn task_passes_filters_combines_only_and_skip() {
        let only = vec!["symlinks".to_string()];
        let skip = vec!["git".to_string()];

        assert!(task_passes_filters("Install symlinks", &only, &skip));
        assert!(!task_passes_filters("Configure Git", &only, &skip));
    }
}
