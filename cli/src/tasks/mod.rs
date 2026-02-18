pub mod chmod;
pub mod copilot_skills;
pub mod developer_mode;
pub mod git_config;
pub mod hooks;
pub mod packages;
pub mod registry;
pub mod shell;
pub mod sparse_checkout;
pub mod symlinks;
pub mod systemd;
pub mod update;
pub mod vscode;

use anyhow::Result;
use std::path::Path;

use crate::config::Config;
use crate::logging::{Logger, TaskStatus};
use crate::platform::Platform;

/// Shared context for task execution.
pub struct Context<'a> {
    pub config: &'a Config,
    pub platform: &'a Platform,
    pub log: &'a Logger,
    pub dry_run: bool,
    pub home: std::path::PathBuf,
}

impl<'a> Context<'a> {
    pub fn new(
        config: &'a Config,
        platform: &'a Platform,
        log: &'a Logger,
        dry_run: bool,
    ) -> Result<Self> {
        let home = if cfg!(target_os = "windows") {
            std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .map_err(|_| {
                    anyhow::anyhow!("neither USERPROFILE nor HOME environment variable is set")
                })?
        } else {
            std::env::var("HOME")
                .map_err(|_| anyhow::anyhow!("HOME environment variable is not set"))?
        };

        Ok(Self {
            config,
            platform,
            log,
            dry_run,
            home: std::path::PathBuf::from(home),
        })
    }

    /// Root directory of the dotfiles repository.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.config.root
    }

    /// Symlinks source directory.
    #[must_use]
    pub fn symlinks_dir(&self) -> std::path::PathBuf {
        self.config.root.join("symlinks")
    }

    /// Hooks source directory.
    #[must_use]
    pub fn hooks_dir(&self) -> std::path::PathBuf {
        self.config.root.join("hooks")
    }
}

/// Result of a single task execution.
pub enum TaskResult {
    /// Task completed successfully.
    Ok,
    /// Task was skipped (not applicable to this platform/profile).
    Skipped(String),
    /// Task ran in dry-run mode.
    DryRun,
}

/// Counters for batch tasks that process many items.
///
/// Provides consistent summary logging across all tasks.
#[derive(Default)]
pub struct TaskStats {
    pub changed: u32,
    pub already_ok: u32,
    pub skipped: u32,
}

impl TaskStats {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Format the summary string (e.g. "3 changed, 10 already ok, 1 skipped").
    #[must_use]
    pub fn summary(&self, dry_run: bool) -> String {
        let verb = if dry_run { "would change" } else { "changed" };
        if self.skipped > 0 {
            format!(
                "{} {verb}, {} already ok, {} skipped",
                self.changed, self.already_ok, self.skipped
            )
        } else {
            format!("{} {verb}, {} already ok", self.changed, self.already_ok)
        }
    }

    /// Log the summary and return the appropriate `TaskResult`.
    #[must_use]
    pub fn finish(self, ctx: &Context) -> TaskResult {
        ctx.log.info(&self.summary(ctx.dry_run));
        if ctx.dry_run {
            TaskResult::DryRun
        } else {
            TaskResult::Ok
        }
    }
}

/// A named, executable task.
pub trait Task {
    /// Human-readable task name.
    fn name(&self) -> &str;

    /// Whether this task should run on the current platform/profile.
    fn should_run(&self, ctx: &Context) -> bool;

    /// Execute the task.
    fn run(&self, ctx: &Context) -> Result<TaskResult>;

    /// Names of tasks that must complete before this task can run.
    ///
    /// Dependencies are specified by task name (case-sensitive).
    /// Default implementation returns no dependencies.
    fn dependencies(&self) -> Vec<&str> {
        Vec::new()
    }
}

/// Execute a task, recording the result in the logger.
pub fn execute(task: &dyn Task, ctx: &Context) {
    if !task.should_run(ctx) {
        ctx.log
            .debug(&format!("skipping task: {} (not applicable)", task.name()));
        ctx.log
            .record_task(task.name(), TaskStatus::NotApplicable, None);
        return;
    }

    ctx.log.stage(task.name());

    match task.run(ctx) {
        Ok(TaskResult::Ok) => {
            ctx.log.record_task(task.name(), TaskStatus::Ok, None);
        }
        Ok(TaskResult::Skipped(reason)) => {
            ctx.log.info(&format!("skipped: {reason}"));
            ctx.log
                .record_task(task.name(), TaskStatus::Skipped, Some(&reason));
        }
        Ok(TaskResult::DryRun) => {
            ctx.log.record_task(task.name(), TaskStatus::DryRun, None);
        }
        Err(e) => {
            ctx.log.error(&format!("{}: {e:#}", task.name()));
            ctx.log
                .record_task(task.name(), TaskStatus::Failed, Some(&format!("{e:#}")));
        }
    }
}

/// Sort tasks by dependencies using topological sort.
///
/// # Errors
///
/// Returns an error if a dependency cycle is detected or if a dependency
/// references a non-existent task.
///
/// # Panics
///
/// Should not panic. Internal unwrap calls are safe because all task names
/// are pre-validated during graph construction.
pub fn sort_by_dependencies<'a>(tasks: &'a [&'a dyn Task]) -> Result<Vec<&'a dyn Task>> {
    use std::collections::{HashMap, VecDeque};

    // Build a name-to-task map for quick lookup
    let mut name_map: HashMap<&str, &'a dyn Task> = HashMap::new();
    for task in tasks {
        name_map.insert(task.name(), *task);
    }

    // Validate dependencies and build adjacency list
    let mut graph: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut in_degree: HashMap<&str, usize> = HashMap::new();

    for task in tasks {
        let name = task.name();
        graph.entry(name).or_default();
        in_degree.entry(name).or_insert(0);

        for dep in task.dependencies() {
            if !name_map.contains_key(dep) {
                anyhow::bail!("task '{name}' depends on non-existent task '{dep}'");
            }
            graph.entry(dep).or_default().push(name);
            *in_degree.entry(name).or_insert(0) += 1;
        }
    }

    // Kahn's algorithm for topological sort
    let mut queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|&(_, &degree)| degree == 0)
        .map(|(&name, _)| name)
        .collect();

    let mut sorted = Vec::new();

    while let Some(current) = queue.pop_front() {
        sorted.push(*name_map.get(current).unwrap());

        if let Some(neighbors) = graph.get(current) {
            for &neighbor in neighbors {
                let degree = in_degree.get_mut(neighbor).unwrap();
                *degree -= 1;
                if *degree == 0 {
                    queue.push_back(neighbor);
                }
            }
        }
    }

    if sorted.len() != tasks.len() {
        anyhow::bail!("dependency cycle detected in task dependencies");
    }

    Ok(sorted)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestTask {
        name: &'static str,
        deps: Vec<&'static str>,
    }

    impl Task for TestTask {
        fn name(&self) -> &str {
            self.name
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }

        fn dependencies(&self) -> Vec<&str> {
            self.deps.clone()
        }
    }

    #[test]
    fn sort_tasks_no_dependencies() {
        let task_a = TestTask {
            name: "A",
            deps: vec![],
        };
        let task_b = TestTask {
            name: "B",
            deps: vec![],
        };

        let tasks: Vec<&dyn Task> = vec![&task_a, &task_b];
        let sorted = sort_by_dependencies(&tasks).unwrap();

        assert_eq!(sorted.len(), 2);
        // Order doesn't matter when no dependencies
    }

    #[test]
    fn sort_tasks_linear_dependency() {
        let task_a = TestTask {
            name: "A",
            deps: vec![],
        };
        let task_b = TestTask {
            name: "B",
            deps: vec!["A"],
        };
        let task_c = TestTask {
            name: "C",
            deps: vec!["B"],
        };

        // Submit in wrong order
        let tasks: Vec<&dyn Task> = vec![&task_c, &task_a, &task_b];
        let sorted = sort_by_dependencies(&tasks).unwrap();

        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].name(), "A");
        assert_eq!(sorted[1].name(), "B");
        assert_eq!(sorted[2].name(), "C");
    }

    #[test]
    fn sort_tasks_diamond_dependency() {
        let task_a = TestTask {
            name: "A",
            deps: vec![],
        };
        let task_b = TestTask {
            name: "B",
            deps: vec!["A"],
        };
        let task_c = TestTask {
            name: "C",
            deps: vec!["A"],
        };
        let task_d = TestTask {
            name: "D",
            deps: vec!["B", "C"],
        };

        let tasks: Vec<&dyn Task> = vec![&task_d, &task_c, &task_b, &task_a];
        let sorted = sort_by_dependencies(&tasks).unwrap();

        assert_eq!(sorted.len(), 4);
        assert_eq!(sorted[0].name(), "A");
        // B and C can be in any order
        assert!(sorted[3].name() == "D");
    }

    #[test]
    fn sort_tasks_cycle_detection() {
        let task_a = TestTask {
            name: "A",
            deps: vec!["B"],
        };
        let task_b = TestTask {
            name: "B",
            deps: vec!["A"],
        };

        let tasks: Vec<&dyn Task> = vec![&task_a, &task_b];
        let result = sort_by_dependencies(&tasks);

        assert!(result.is_err());
        let err_msg = result.err().unwrap().to_string();
        assert!(err_msg.contains("dependency cycle"));
    }

    #[test]
    fn sort_tasks_missing_dependency() {
        let task_a = TestTask {
            name: "A",
            deps: vec!["NonExistent"],
        };

        let tasks: Vec<&dyn Task> = vec![&task_a];
        let result = sort_by_dependencies(&tasks);

        assert!(result.is_err());
        let err_msg = result.err().unwrap().to_string();
        assert!(err_msg.contains("non-existent"));
    }
}
