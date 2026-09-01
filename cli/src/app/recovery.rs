//! Structured run outcomes and dependency-safe retry selection.

use std::collections::HashSet;

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};

use crate::engine::graph::{DependencyEdges, ResolvedTaskGraph};
use crate::engine::scheduler::ExecutionSummary;
use crate::engine::{Context, Task, TaskId};

const FORMAT_VERSION: u8 = 1;
const STATE_FILE: &str = "dotfiles-last-run.json";

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RecoveryState {
    version: u8,
    command: String,
    incomplete_selectors: Vec<String>,
}

pub(crate) fn persist(ctx: &Context, command: &str, summary: &ExecutionSummary) -> Result<()> {
    let path = crate::infra::run_lock::repository_state_path(
        ctx.root(),
        ctx.env().as_ref(),
        ctx.platform(),
        STATE_FILE,
    )?;
    let mut incomplete_selectors = summary
        .incomplete_selectors()
        .into_iter()
        .collect::<Vec<_>>();
    incomplete_selectors.sort();
    let state = RecoveryState {
        version: FORMAT_VERSION,
        command: command.to_string(),
        incomplete_selectors,
    };
    let contents = serde_json::to_vec_pretty(&state).context("serializing recovery state")?;
    crate::infra::fs::write_atomic(&path, contents)
        .with_context(|| format!("writing recovery state {}", path.display()))
}

pub(crate) fn load(ctx: &Context, command: &str) -> Result<HashSet<String>> {
    let path = crate::infra::run_lock::repository_state_path(
        ctx.root(),
        ctx.env().as_ref(),
        ctx.platform(),
        STATE_FILE,
    )?;
    let contents = std::fs::read(&path).with_context(|| {
        format!(
            "reading recovery state {}; run {command} normally first",
            path.display()
        )
    })?;
    let state: RecoveryState =
        serde_json::from_slice(&contents).context("parsing recovery state")?;
    if state.version != FORMAT_VERSION {
        bail!(
            "unsupported recovery state version {}; run {command} normally to replace it",
            state.version
        );
    }
    if state.command != command {
        bail!(
            "the last recorded run was '{}', not '{command}'; retry that command or run {command} normally",
            state.command
        );
    }
    if state.incomplete_selectors.is_empty() {
        bail!("the previous {command} run has no failed or blocked tasks to retry");
    }
    Ok(state.incomplete_selectors.into_iter().collect())
}

pub(crate) fn select_tasks<'a>(
    tasks: &'a [Box<dyn Task>],
    additional_known_tasks: &[Box<dyn Task>],
    selectors: &HashSet<String>,
    required: &[TaskId],
) -> Result<Vec<&'a dyn Task>> {
    let known = tasks
        .iter()
        .chain(additional_known_tasks)
        .map(|task| task.selector())
        .collect::<HashSet<_>>();
    let mut unmatched = selectors
        .iter()
        .filter(|selector| !known.contains(selector.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    unmatched.sort();
    if !unmatched.is_empty() {
        bail!(
            "recovery tasks are no longer available in this configuration: {}",
            unmatched.join(", ")
        );
    }

    let mut selected = tasks
        .iter()
        .filter(|task| selectors.contains(task.selector()))
        .map(|task| task.task_id())
        .chain(required.iter().cloned())
        .collect::<HashSet<_>>();
    let task_refs = tasks.iter().map(Box::as_ref).collect::<Vec<_>>();
    ResolvedTaskGraph::resolve(&task_refs)?
        .extend_dependency_closure(&mut selected, DependencyEdges::All);

    Ok(tasks
        .iter()
        .filter(|task| selected.contains(&task.task_id()))
        .map(Box::as_ref)
        .collect())
}

#[must_use]
pub(crate) fn task_selected(task: &dyn Task, selectors: &HashSet<String>) -> bool {
    selectors.contains(task.selector())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{TaskMeta, TaskOutcome, TaskResult};
    use crate::test_helpers::{empty_config, make_static_context};

    #[derive(Debug)]
    struct RecoveryTask {
        name: &'static str,
        id: u64,
        dependencies: Vec<TaskId>,
    }

    impl Task for RecoveryTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new(self.name).with_selector(self.name)
        }

        fn task_id(&self) -> TaskId {
            TaskId::Dynamic(self.id)
        }

        fn dependencies(&self) -> &[TaskId] {
            &self.dependencies
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }
    }

    #[test]
    fn retry_selection_includes_transitive_dependencies() {
        let tasks: Vec<Box<dyn Task>> = vec![
            Box::new(RecoveryTask {
                name: "root",
                id: 1,
                dependencies: Vec::new(),
            }),
            Box::new(RecoveryTask {
                name: "middle",
                id: 2,
                dependencies: vec![TaskId::Dynamic(1)],
            }),
            Box::new(RecoveryTask {
                name: "failed",
                id: 3,
                dependencies: vec![TaskId::Dynamic(2)],
            }),
            Box::new(RecoveryTask {
                name: "unrelated",
                id: 4,
                dependencies: Vec::new(),
            }),
        ];

        let selected = select_tasks(&tasks, &[], &HashSet::from(["failed".to_string()]), &[])
            .expect("selection");
        let names = selected.iter().map(|task| task.name()).collect::<Vec<_>>();

        assert_eq!(names, vec!["root", "middle", "failed"]);
    }

    fn recovery_context() -> (tempfile::TempDir, Context) {
        let root = tempfile::tempdir().expect("temp repository");
        git2::Repository::init(root.path()).expect("initialize repository");
        let (ctx, _log) = make_static_context(empty_config(root.path().to_path_buf()));
        (root, ctx)
    }

    fn state_path(ctx: &Context) -> std::path::PathBuf {
        crate::infra::run_lock::repository_state_path(
            ctx.root(),
            ctx.env().as_ref(),
            ctx.platform(),
            STATE_FILE,
        )
        .expect("recovery state path")
    }

    #[test]
    fn persisted_recovery_state_round_trips_incomplete_selectors() {
        let (_root, ctx) = recovery_context();
        let mut summary = ExecutionSummary::default();
        summary.record(TaskId::Dynamic(1), "zeta", "Zeta task", TaskOutcome::Failed);
        summary.record(
            TaskId::Dynamic(2),
            "alpha",
            "Alpha task",
            TaskOutcome::Blocked,
        );

        persist(&ctx, "install", &summary).expect("persist recovery state");
        let loaded = load(&ctx, "install").expect("load recovery state");

        assert_eq!(
            loaded,
            HashSet::from(["alpha".to_string(), "zeta".to_string()])
        );
        let contents = std::fs::read_to_string(state_path(&ctx)).expect("read recovery state");
        assert!(
            contents.find("alpha") < contents.find("zeta"),
            "persisted selectors should be deterministic"
        );
    }

    #[test]
    fn recovery_state_rejects_a_different_command() {
        let (_root, ctx) = recovery_context();
        let mut summary = ExecutionSummary::default();
        summary.record(
            TaskId::Dynamic(1),
            "packages",
            "Packages",
            TaskOutcome::Failed,
        );
        persist(&ctx, "install", &summary).expect("persist recovery state");

        let error = load(&ctx, "uninstall").expect_err("command mismatch should fail");

        assert!(
            error
                .to_string()
                .contains("last recorded run was 'install'")
        );
    }

    #[test]
    fn recovery_state_rejects_malformed_json() {
        let (_root, ctx) = recovery_context();
        std::fs::write(state_path(&ctx), "{not-json").expect("write malformed state");

        let error = load(&ctx, "install").expect_err("malformed state should fail");

        assert!(format!("{error:#}").contains("parsing recovery state"));
    }

    #[test]
    fn recovery_state_rejects_runs_without_incomplete_tasks() {
        let (_root, ctx) = recovery_context();
        persist(&ctx, "install", &ExecutionSummary::default()).expect("persist recovery state");

        let error = load(&ctx, "install").expect_err("empty retry set should fail");

        assert!(error.to_string().contains("no failed or blocked tasks"));
    }
}
