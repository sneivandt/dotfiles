use super::*;

#[cfg(test)]
mod reexec_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn re_exec_path_uses_installed_binary_path() {
        let root = Path::new("/repo");
        let expected = if cfg!(windows) {
            "dotfiles.exe"
        } else {
            "dotfiles"
        };
        assert_eq!(re_exec_path(root), root.join("bin").join(expected));
    }

    #[test]
    fn re_exec_command_preserves_arguments_and_sets_loop_guard() {
        let args = vec![
            "install".to_string(),
            "--profile".to_string(),
            "desktop".to_string(),
        ];
        let command = build_reexec_command(Path::new("/repo/bin/dotfiles"), &args);

        assert_eq!(command.get_program(), "/repo/bin/dotfiles");
        assert_eq!(
            command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            args
        );
        let guard = command
            .get_envs()
            .find(|(key, _)| *key == REEXEC_GUARD_VAR)
            .and_then(|(_, value)| value);
        assert_eq!(guard, Some(std::ffi::OsStr::new("1")));
    }
}

#[cfg(test)]
mod startup_log_tests {
    use super::runner::startup_context_line;
    use crate::infra::platform::{Os, Platform};
    use std::path::Path;

    #[test]
    fn startup_context_uses_command_profile_platform_and_dry_run() {
        assert_eq!(
            startup_context_line(
                "Install",
                "workstation",
                Platform::new(Os::Linux, false),
                true,
                None,
            ),
            "Install · dry run · profile workstation · Linux"
        );
    }

    #[test]
    fn overlay_is_the_optional_last_startup_section() {
        assert_eq!(
            startup_context_line(
                "Install",
                "workstation",
                Platform::new(Os::Linux, false),
                false,
                Some(Path::new("/private/overlay")),
            ),
            "Install · profile workstation · Linux · overlay /private/overlay",
            "overlay must be appended to the startup header, not emitted on its own line"
        );
    }

    #[test]
    fn dry_run_follows_the_command_and_overlay_stays_last() {
        assert_eq!(
            startup_context_line(
                "Install",
                "workstation",
                Platform::new(Os::Linux, false),
                true,
                Some(Path::new("/private/overlay")),
            ),
            "Install · dry run · profile workstation · Linux · overlay /private/overlay"
        );
    }
}

#[cfg(test)]
mod task_graph_tests {
    use super::execution::{run_tasks_to_completion, run_tasks_to_completion_with_late_tasks};
    use crate::engine::{Context, Task, TaskId, TaskMeta, TaskResult, task_deps};
    use crate::test_helpers::{empty_config, make_static_context};
    use anyhow::Result;
    use std::path::PathBuf;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    struct CycleTaskA {
        ran: Arc<AtomicBool>,
    }

    impl Task for CycleTaskA {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("cycle-a")
        }

        task_deps![CycleTaskB];

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.ran.store(true, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    struct CycleTaskB {
        ran: Arc<AtomicBool>,
    }

    impl Task for CycleTaskB {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("cycle-b")
        }

        task_deps![CycleTaskA];

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.ran.store(true, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    #[test]
    fn run_tasks_to_completion_bails_on_dependency_cycles() {
        let (ctx, log) = make_static_context(empty_config(PathBuf::from("/repo")));
        let ctx = ctx.with_parallel(true);
        let ran_a = Arc::new(AtomicBool::new(false));
        let ran_b = Arc::new(AtomicBool::new(false));
        let task_a = CycleTaskA {
            ran: Arc::clone(&ran_a),
        };
        let task_b = CycleTaskB {
            ran: Arc::clone(&ran_b),
        };

        let tasks: [&dyn Task; 2] = [&task_a, &task_b];
        let err = run_tasks_to_completion(tasks, &ctx, &log)
            .expect_err("cyclic task graphs should fail fast");

        assert!(format!("{err:#}").contains("dependency cycle: cycle-a -> cycle-b -> cycle-a"));
        assert!(!ran_a.load(Ordering::SeqCst));
        assert!(!ran_b.load(Ordering::SeqCst));
    }

    struct PrerequisiteTask {
        name: &'static str,
        completed: Arc<AtomicUsize>,
    }

    impl Task for PrerequisiteTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new(self.name)
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.completed.fetch_add(1, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    struct DependentTask {
        ran: Arc<AtomicBool>,
        completed_prerequisites: Arc<AtomicUsize>,
        expected_prerequisite_count: usize,
    }

    impl Task for DependentTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("dependent")
        }

        task_deps![PrerequisiteTask];

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.ran.store(true, Ordering::SeqCst);
            let done = self.completed_prerequisites.load(Ordering::SeqCst);
            if done != self.expected_prerequisite_count {
                return Ok(TaskResult::Failed(format!(
                    "dependent started before prerequisite completed: {done}/{}",
                    self.expected_prerequisite_count
                )));
            }
            Ok(TaskResult::Ok)
        }
    }

    #[test]
    fn run_tasks_to_completion_obeys_dependencies_regardless_of_input_order() {
        let (ctx, log) = make_static_context(empty_config(PathBuf::from("/repo")));
        let ctx = ctx.with_parallel(true);

        let completed_prerequisites = Arc::new(AtomicUsize::new(0));
        let dependent_ran = Arc::new(AtomicBool::new(false));

        let prerequisite = PrerequisiteTask {
            name: "prerequisite",
            completed: Arc::clone(&completed_prerequisites),
        };
        let dependent = DependentTask {
            ran: Arc::clone(&dependent_ran),
            completed_prerequisites: Arc::clone(&completed_prerequisites),
            expected_prerequisite_count: 1,
        };

        // Intentionally pass the dependent first: graph edges, not catalog
        // order, control execution.
        let tasks: [&dyn Task; 2] = [&dependent, &prerequisite];
        run_tasks_to_completion(tasks, &ctx, &log)
            .expect("dependency should complete before its dependent");

        assert_eq!(completed_prerequisites.load(Ordering::SeqCst), 1);
        assert!(dependent_ran.load(Ordering::SeqCst));
    }

    struct BoundaryPrerequisiteTask {
        completed: Arc<AtomicBool>,
    }

    impl Task for BoundaryPrerequisiteTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("boundary-prerequisite")
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.completed.store(true, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    struct DiscoveryBoundaryTask {
        prerequisite_completed: Arc<AtomicBool>,
        completed: Arc<AtomicBool>,
    }

    impl Task for DiscoveryBoundaryTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("discovery-boundary")
        }

        task_deps![BoundaryPrerequisiteTask];

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            if !self.prerequisite_completed.load(Ordering::SeqCst) {
                return Ok(TaskResult::Failed(
                    "boundary ran before its prerequisite".to_string(),
                ));
            }
            self.completed.store(true, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    struct RemainingStaticTask {
        boundary_completed: Arc<AtomicBool>,
        ran: Arc<AtomicBool>,
    }

    impl Task for RemainingStaticTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("remaining-static")
        }

        task_deps![DiscoveryBoundaryTask];

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            if !self.boundary_completed.load(Ordering::SeqCst) {
                return Ok(TaskResult::Failed(
                    "remaining task ran before boundary".to_string(),
                ));
            }
            self.ran.store(true, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    struct LateDiscoveredTask {
        boundary_completed: Arc<AtomicBool>,
        ran: Arc<AtomicBool>,
    }

    impl Task for LateDiscoveredTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("late-discovered")
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            if !self.boundary_completed.load(Ordering::SeqCst) {
                return Ok(TaskResult::Failed(
                    "late task ran before the discovery boundary completed".to_string(),
                ));
            }
            self.ran.store(true, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    #[test]
    fn late_tasks_are_built_after_dependency_boundary() {
        let (ctx, log) = make_static_context(empty_config(PathBuf::from("/repo")));
        let prerequisite_completed = Arc::new(AtomicBool::new(false));
        let boundary_completed = Arc::new(AtomicBool::new(false));
        let late_ran = Arc::new(AtomicBool::new(false));
        let remaining_ran = Arc::new(AtomicBool::new(false));
        let prerequisite = BoundaryPrerequisiteTask {
            completed: Arc::clone(&prerequisite_completed),
        };
        let boundary = DiscoveryBoundaryTask {
            prerequisite_completed: Arc::clone(&prerequisite_completed),
            completed: Arc::clone(&boundary_completed),
        };
        let remaining = RemainingStaticTask {
            boundary_completed: Arc::clone(&boundary_completed),
            ran: Arc::clone(&remaining_ran),
        };
        let provider_prerequisite_completed = Arc::clone(&prerequisite_completed);
        let provider_boundary_completed = Arc::clone(&boundary_completed);
        let task_boundary_completed = Arc::clone(&boundary_completed);
        let task_late_ran = Arc::clone(&late_ran);
        let tasks: [&dyn Task; 3] = [&remaining, &boundary, &prerequisite];

        run_tasks_to_completion_with_late_tasks(
            tasks,
            &ctx,
            &log,
            TaskId::Type(std::any::TypeId::of::<DiscoveryBoundaryTask>()),
            move || {
                assert!(
                    provider_prerequisite_completed.load(Ordering::SeqCst),
                    "boundary dependency closure must complete before late discovery"
                );
                assert!(
                    provider_boundary_completed.load(Ordering::SeqCst),
                    "late task provider must run after the boundary completes"
                );
                vec![Box::new(LateDiscoveredTask {
                    boundary_completed: task_boundary_completed,
                    ran: task_late_ran,
                })]
            },
        )
        .expect("late task should execute in the same pipeline");

        assert!(late_ran.load(Ordering::SeqCst));
        assert!(remaining_ran.load(Ordering::SeqCst));
    }

    struct FailingBoundaryTask;

    impl Task for FailingBoundaryTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("failing-boundary")
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Failed("boundary failed".to_string()))
        }
    }

    #[test]
    fn boundary_failure_suppresses_late_task_discovery() {
        let (ctx, log) = make_static_context(empty_config(PathBuf::from("/repo")));
        let provider_called = Arc::new(AtomicBool::new(false));
        let provider_called_by_closure = Arc::clone(&provider_called);
        let boundary = FailingBoundaryTask;
        let tasks: [&dyn Task; 1] = [&boundary];

        let result = run_tasks_to_completion_with_late_tasks(
            tasks,
            &ctx,
            &log,
            TaskId::Type(std::any::TypeId::of::<FailingBoundaryTask>()),
            move || {
                provider_called_by_closure.store(true, Ordering::SeqCst);
                Vec::new()
            },
        );

        assert!(result.is_err());
        assert!(!provider_called.load(Ordering::SeqCst));
    }

    struct StaticAfterProviderTask {
        provider_called: Arc<AtomicBool>,
        ran: Arc<AtomicBool>,
    }

    impl Task for StaticAfterProviderTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("static-after-provider")
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            if !self.provider_called.load(Ordering::SeqCst) {
                return Ok(TaskResult::Failed(
                    "static task ran before provider".to_string(),
                ));
            }
            self.ran.store(true, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    #[test]
    fn missing_boundary_discovers_late_tasks_before_single_graph() {
        let (ctx, log) = make_static_context(empty_config(PathBuf::from("/repo")));
        let provider_called = Arc::new(AtomicBool::new(false));
        let static_ran = Arc::new(AtomicBool::new(false));
        let static_task = StaticAfterProviderTask {
            provider_called: Arc::clone(&provider_called),
            ran: Arc::clone(&static_ran),
        };
        let provider_called_by_closure = Arc::clone(&provider_called);
        let tasks: [&dyn Task; 1] = [&static_task];

        run_tasks_to_completion_with_late_tasks(
            tasks,
            &ctx,
            &log,
            TaskId::Dynamic(42),
            move || {
                provider_called_by_closure.store(true, Ordering::SeqCst);
                Vec::new()
            },
        )
        .expect("tasks should run when the discovery boundary is filtered out");

        assert!(provider_called.load(Ordering::SeqCst));
        assert!(static_ran.load(Ordering::SeqCst));
    }
}
