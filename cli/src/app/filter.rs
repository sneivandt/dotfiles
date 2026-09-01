//! Task filter matching helpers for command `--only` and `--skip` options.

use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};

use crate::app::task_dependencies::{DependencyEdges, extend_dependency_closure};
use crate::engine::{Task, TaskId};
use crate::infra::logging::OutputExt as _;
use crate::infra::logging::{Logger, Output};

/// Validate task selectors, then return the tasks that survive them.
pub(crate) fn apply_task_filters<'a>(
    all_tasks: &'a [Box<dyn Task>],
    additional_known_tasks: &[Box<dyn Task>],
    only: &[String],
    skip: &[String],
    with_dependencies: bool,
    log: &Logger,
) -> Result<Vec<&'a dyn Task>> {
    let known_task_refs: Vec<&dyn Task> = all_tasks
        .iter()
        .chain(additional_known_tasks)
        .map(Box::as_ref)
        .collect();
    let unmatched_only = unmatched_filters(&known_task_refs, only);
    let unmatched_skip = unmatched_filters(&known_task_refs, skip);
    reject_unmatched_filters(&known_task_refs, &unmatched_only, "--only")?;
    reject_unmatched_filters(&known_task_refs, &unmatched_skip, "--skip")?;

    let mut selected = all_tasks
        .iter()
        .chain(additional_known_tasks)
        .filter(|task| {
            only.is_empty()
                || only
                    .iter()
                    .any(|filter| task_matches_filter(task.as_ref(), filter))
        })
        .map(|task| task.task_id())
        .collect::<HashSet<_>>();
    if with_dependencies {
        extend_dependency_closure(&known_task_refs, &mut selected, DependencyEdges::All);
    }
    let filtered: Vec<&dyn Task> = all_tasks
        .iter()
        .filter(|task| {
            selected.contains(&task.task_id())
                && !skip
                    .iter()
                    .any(|filter| task_matches_filter(task.as_ref(), filter))
        })
        .map(Box::as_ref)
        .collect();
    let omitted_dependencies = omitted_dependencies(all_tasks, &filtered);

    if !log.is_verbose() && !omitted_dependencies.is_empty() {
        log.separate_from_startup();
    }
    warn_omitted_dependencies(&omitted_dependencies, log);

    if !only.is_empty() || !skip.is_empty() {
        let names: Vec<&str> = filtered.iter().map(|task| task.name()).collect();
        log.debug(format!(
            "active filters — running {} task(s): {}",
            names.len(),
            names.join(", ")
        ));
    }

    Ok(filtered)
}

/// Return filters that do not match any known task.
pub(crate) fn unmatched_filters<'a>(tasks: &[&dyn Task], filters: &'a [String]) -> Vec<&'a str> {
    filters
        .iter()
        .filter(|filter| !tasks.iter().any(|task| task_matches_filter(*task, filter)))
        .map(String::as_str)
        .collect()
}

fn reject_unmatched_filters(tasks: &[&dyn Task], filters: &[&str], flag: &str) -> Result<()> {
    if filters.is_empty() {
        return Ok(());
    }
    let messages = filters
        .iter()
        .map(|filter| {
            closest_selector(tasks, filter).map_or_else(
                || format!("'{filter}'"),
                |selector| format!("'{filter}' (did you mean '{selector}'?)"),
            )
        })
        .collect::<Vec<_>>();
    bail!(
        "{flag} did not match a task selector: {}. Run 'dotfiles tasks' to list selectors",
        messages.join(", ")
    )
}

fn closest_selector<'a>(tasks: &[&'a dyn Task], filter: &str) -> Option<&'a str> {
    let normalized = normalize_task_filter(filter);
    tasks
        .iter()
        .map(|task| {
            (
                task.selector(),
                strsim::jaro_winkler(&normalized, &normalize_task_filter(task.selector())),
            )
        })
        .filter(|(_, score)| *score >= 0.75)
        .max_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(selector, _)| selector)
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
    if !task.visibility().is_visible() {
        return false;
    }

    let normalized_filter = normalize_task_filter(filter);
    if normalized_filter.is_empty() {
        return false;
    }

    normalized_filter == normalize_task_filter(task.selector())
        || normalized_filter == normalize_task_filter(task.name())
}

fn omitted_dependencies<'a>(
    all_tasks: &'a [Box<dyn Task>],
    filtered: &[&'a dyn Task],
) -> Vec<(&'a str, &'a str)> {
    let active = filtered
        .iter()
        .map(|task| task.task_id())
        .collect::<HashSet<_>>();
    let known = all_tasks
        .iter()
        .map(|task| (task.task_id(), task.as_ref()))
        .collect::<HashMap<TaskId, &dyn Task>>();
    let mut omitted = Vec::new();

    for task in filtered {
        for dependency in task
            .dependencies()
            .iter()
            .chain(task.ordering_dependencies())
        {
            let Some(dependency_task) = known.get(dependency) else {
                continue;
            };
            let pair = (task.name(), dependency_task.name());
            if !active.contains(dependency) && !omitted.contains(&pair) {
                omitted.push(pair);
            }
        }
    }

    omitted
}

fn warn_omitted_dependencies(dependencies: &[(&str, &str)], log: &dyn Output) {
    for (task, dependency) in dependencies {
        log.warn(format!(
            "task '{task}' will run without filtered prerequisite '{dependency}'; assuming it is already satisfied"
        ));
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
    use crate::engine::{Context, TaskId, TaskMeta, TaskResult, TaskVisibility};
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

    struct InternalTask;

    impl Task for InternalTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("Reload configuration")
                .with_selector("reload-configuration")
                .with_visibility(TaskVisibility::Internal)
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }
    }

    struct DependentTask;

    impl Task for DependentTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("Dependent").with_selector("dependent")
        }

        fn dependencies(&self) -> &[TaskId] {
            const DEPS: &[TaskId] = &[TaskId::Type(std::any::TypeId::of::<SampleTask>())];
            DEPS
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
    fn internal_tasks_cannot_match_user_filters() {
        let task = InternalTask;
        assert!(!task_matches_filter(&task, "reload-configuration"));
        assert!(!task_matches_filter(&task, "Reload configuration"));
        assert!(
            task_passes_filters(&task, &[], &["reload-configuration".to_string()]),
            "--skip must not remove an internal orchestration boundary"
        );
        assert!(
            !task_passes_filters(&task, &["reload-configuration".to_string()], &[]),
            "--only must not select an internal orchestration task"
        );
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
    fn unmatched_filters_fail_with_a_selector_suggestion() {
        let tasks: [&dyn Task; 2] = [&SampleTask, &OtherTask];
        let filters = ["symlink".to_string()];
        let unmatched = unmatched_filters(&tasks, &filters);
        let error = reject_unmatched_filters(&tasks, &unmatched, "--only")
            .expect_err("unknown selector should fail");

        assert!(error.to_string().contains("did you mean 'symlinks'?"));
    }

    #[test]
    fn omitted_dependencies_are_reported_without_expanding_the_filter() {
        let all: Vec<Box<dyn Task>> = vec![Box::new(SampleTask), Box::new(DependentTask)];
        let log = RecordingOutput::default();

        let filtered = apply_task_filters(
            &all,
            &[],
            &["dependent".to_string()],
            &[],
            false,
            &Logger::new("test"),
        )
        .expect("valid filter");
        assert_eq!(
            filtered.iter().map(|task| task.name()).collect::<Vec<_>>(),
            vec!["Dependent"],
            "targeted execution should retain its existing non-expanding semantics"
        );

        let dependencies = omitted_dependencies(&all, &filtered);
        warn_omitted_dependencies(&dependencies, &log);
        assert_eq!(
            log.warnings(),
            vec![
                "task 'Dependent' will run without filtered prerequisite 'Home symlinks'; assuming it is already satisfied"
                    .to_string()
            ]
        );
    }

    #[test]
    fn with_dependencies_adds_the_transitive_closure() {
        let all: Vec<Box<dyn Task>> = vec![Box::new(SampleTask), Box::new(DependentTask)];
        let filtered = apply_task_filters(
            &all,
            &[],
            &["dependent".to_string()],
            &[],
            true,
            &Logger::new("test"),
        )
        .expect("valid filter");

        assert_eq!(
            filtered.iter().map(|task| task.name()).collect::<Vec<_>>(),
            vec!["Home symlinks", "Dependent"]
        );
    }
}
