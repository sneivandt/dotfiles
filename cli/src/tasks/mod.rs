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
use std::any::TypeId;
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
pub trait Task: 'static {
    /// Human-readable task name.
    fn name(&self) -> &str;

    /// Whether this task should run on the current platform/profile.
    fn should_run(&self, ctx: &Context) -> bool;

    /// Execute the task.
    fn run(&self, ctx: &Context) -> Result<TaskResult>;

    /// `TypeIds` of tasks that must complete before this task can run.
    ///
    /// Dependencies are specified using `TypeId::of::<TaskType>()`.
    /// Default implementation returns no dependencies.
    fn dependencies(&self) -> Vec<TypeId> {
        Vec::new()
    }

    /// Get the `TypeId` for this task type.
    fn type_id(&self) -> TypeId {
        TypeId::of::<Self>()
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
/// Should not panic. Internal unwrap calls are safe because all task `TypeIds`
/// are pre-validated during graph construction.
pub fn sort_by_dependencies<'a>(tasks: &'a [&'a dyn Task]) -> Result<Vec<&'a dyn Task>> {
    use std::collections::{HashMap, VecDeque};

    // Build a TypeId-to-task map for quick lookup
    let mut type_map: HashMap<TypeId, &'a dyn Task> = HashMap::new();
    for task in tasks {
        type_map.insert(task.type_id(), *task);
    }

    // Validate dependencies and build adjacency list
    let mut graph: HashMap<TypeId, Vec<TypeId>> = HashMap::new();
    let mut in_degree: HashMap<TypeId, usize> = HashMap::new();

    for task in tasks {
        let type_id = task.type_id();
        graph.entry(type_id).or_default();
        in_degree.entry(type_id).or_insert(0);

        for dep_id in task.dependencies() {
            if !type_map.contains_key(&dep_id) {
                anyhow::bail!("task '{}' depends on non-existent task type", task.name());
            }
            graph.entry(dep_id).or_default().push(type_id);
            *in_degree.entry(type_id).or_insert(0) += 1;
        }
    }

    // Kahn's algorithm for topological sort
    let mut queue: VecDeque<TypeId> = in_degree
        .iter()
        .filter(|&(_, &degree)| degree == 0)
        .map(|(&type_id, _)| type_id)
        .collect();

    let mut sorted = Vec::new();

    while let Some(current) = queue.pop_front() {
        sorted.push(*type_map.get(&current).unwrap());

        if let Some(neighbors) = graph.get(&current) {
            for &neighbor in neighbors {
                let degree = in_degree.get_mut(&neighbor).unwrap();
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

/// Get all available install tasks.
///
/// Returns a vector of boxed tasks that should be run during installation.
/// The tasks are registered here and will be automatically sorted by dependencies.
#[must_use]
pub fn all_install_tasks() -> Vec<Box<dyn Task>> {
    vec![
        Box::new(developer_mode::EnableDeveloperMode),
        Box::new(sparse_checkout::SparseCheckout),
        Box::new(update::UpdateRepository),
        Box::new(hooks::GitHooks),
        Box::new(git_config::ConfigureGit),
        Box::new(packages::InstallPackages),
        Box::new(packages::InstallParu),
        Box::new(packages::InstallAurPackages),
        Box::new(symlinks::InstallSymlinks),
        Box::new(chmod::ApplyFilePermissions),
        Box::new(shell::ConfigureShell),
        Box::new(vscode::InstallVsCodeExtensions),
        Box::new(copilot_skills::InstallCopilotSkills),
        Box::new(systemd::ConfigureSystemd),
        Box::new(registry::ApplyRegistry),
    ]
}

/// Get all available uninstall tasks.
///
/// Returns a vector of boxed tasks that should be run during uninstallation.
#[must_use]
pub fn all_uninstall_tasks() -> Vec<Box<dyn Task>> {
    vec![
        Box::new(symlinks::UninstallSymlinks),
        Box::new(hooks::UninstallHooks),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestTaskA;
    struct TestTaskB;
    struct TestTaskC;
    struct TestTaskD;

    impl Task for TestTaskA {
        fn name(&self) -> &str {
            "A"
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }
    }

    impl Task for TestTaskB {
        fn name(&self) -> &str {
            "B"
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }

        fn dependencies(&self) -> Vec<TypeId> {
            vec![TypeId::of::<TestTaskA>()]
        }
    }

    impl Task for TestTaskC {
        fn name(&self) -> &str {
            "C"
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }

        fn dependencies(&self) -> Vec<TypeId> {
            vec![TypeId::of::<TestTaskB>()]
        }
    }

    impl Task for TestTaskD {
        fn name(&self) -> &str {
            "D"
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }

        fn dependencies(&self) -> Vec<TypeId> {
            vec![TypeId::of::<TestTaskB>(), TypeId::of::<TestTaskC>()]
        }
    }

    #[test]
    fn sort_tasks_no_dependencies() {
        let task_a = TestTaskA;
        let task_b = TestTaskB; // Different type, no actual dependency

        // Temporarily remove B's dependency for this test
        struct TestTaskBNoDep;
        impl Task for TestTaskBNoDep {
            fn name(&self) -> &str {
                "B"
            }
            fn should_run(&self, _ctx: &Context) -> bool {
                true
            }
            fn run(&self, _ctx: &Context) -> Result<TaskResult> {
                Ok(TaskResult::Ok)
            }
            // No dependencies
        }

        let task_b_no_dep = TestTaskBNoDep;
        let tasks: Vec<&dyn Task> = vec![&task_a, &task_b_no_dep];
        let sorted = sort_by_dependencies(&tasks).unwrap();

        assert_eq!(sorted.len(), 2);
        // Order doesn't matter when no dependencies
    }

    #[test]
    fn sort_tasks_linear_dependency() {
        let task_a = TestTaskA;
        let task_b = TestTaskB;
        let task_c = TestTaskC;

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
        let task_a = TestTaskA;
        let task_b = TestTaskB; // depends on A
        let task_c = TestTaskC; // depends on B
        let task_d = TestTaskD; // depends on B and C

        let tasks: Vec<&dyn Task> = vec![&task_d, &task_c, &task_b, &task_a];
        let sorted = sort_by_dependencies(&tasks).unwrap();

        assert_eq!(sorted.len(), 4);
        assert_eq!(sorted[0].name(), "A");
        // Verify D is last
        assert!(sorted[3].name() == "D");
    }

    #[test]
    fn sort_validates_real_task_dependencies() {
        // This test ensures that if we add dependencies between actual tasks,
        // they reference valid task types and don't create cycles.

        use crate::tasks::*;

        let tasks: Vec<Box<dyn Task>> = vec![
            Box::new(packages::InstallPackages),
            Box::new(packages::InstallParu),
            Box::new(packages::InstallAurPackages),
            Box::new(symlinks::InstallSymlinks),
            Box::new(chmod::ApplyFilePermissions),
            Box::new(sparse_checkout::SparseCheckout),
            Box::new(update::UpdateRepository),
        ];

        let task_refs: Vec<&dyn Task> = tasks.iter().map(std::convert::AsRef::as_ref).collect();

        // This should not panic or error - validates no cycles and all deps exist
        let result = sort_by_dependencies(&task_refs);
        assert!(result.is_ok(), "Task dependency graph should be valid");

        let sorted = result.unwrap();
        assert_eq!(sorted.len(), tasks.len());

        // Verify specific ordering constraints
        let names: Vec<&str> = sorted.iter().map(|t| t.name()).collect();

        // InstallParu must come before InstallAurPackages
        let paru_idx = names.iter().position(|&n| n == "Install paru");
        let aur_idx = names.iter().position(|&n| n == "Install AUR packages");
        if let (Some(paru), Some(aur)) = (paru_idx, aur_idx) {
            assert!(
                paru < aur,
                "Install paru must come before Install AUR packages"
            );
        }

        // InstallSymlinks must come before ApplyFilePermissions
        let symlinks_idx = names.iter().position(|&n| n == "Install symlinks");
        let chmod_idx = names.iter().position(|&n| n == "Apply file permissions");
        if let (Some(sym), Some(chmod)) = (symlinks_idx, chmod_idx) {
            assert!(
                sym < chmod,
                "Install symlinks must come before Apply file permissions"
            );
        }

        // SparseCheckout must come before UpdateRepository
        let sparse_idx = names.iter().position(|&n| n == "Configure sparse checkout");
        let update_idx = names.iter().position(|&n| n == "Update repository");
        if let (Some(sparse), Some(update)) = (sparse_idx, update_idx) {
            assert!(
                sparse < update,
                "Configure sparse checkout must come before Update repository"
            );
        }
    }
}
