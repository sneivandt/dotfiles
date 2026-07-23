use super::*;

#[cfg(all(test, windows))]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code uses panicking helpers"
)]
mod windows_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn windows_restart_helper_script_relaunches_with_splatting_and_guard() {
        let script = build_windows_restart_helper_script(
            Path::new("C:\\Program Files\\dotfiles.exe"),
            Path::new("C:\\Program Files\\.dotfiles-update.pending"),
            Path::new("C:\\Program Files\\.dotfiles-update.version"),
            Path::new("C:\\Program Files\\.dotfiles-version-cache"),
            &["--root".to_string(), "C:\\Users\\Me\\My Repo".to_string()],
        );

        assert!(script.contains("$env:DOTFILES_REEXEC_GUARD = '1';"));
        assert!(script.contains("& $exe @args;"));
        assert!(script.contains("exit $LASTEXITCODE"));
        assert!(!script.contains("Start-Process -FilePath $exe -ArgumentList $args"));
    }

    #[test]
    fn windows_restart_helper_script_uses_safe_atomic_update() {
        let script = build_windows_restart_helper_script(
            Path::new("C:\\Program Files\\dotfiles.exe"),
            Path::new("C:\\Program Files\\.dotfiles-update.pending"),
            Path::new("C:\\Program Files\\.dotfiles-update.version"),
            Path::new("C:\\Program Files\\.dotfiles-version-cache"),
            &["--root".to_string()],
        );

        // The exe must NOT be deleted directly before the pending file is moved.
        assert!(
            !script.contains("Remove-Item $exe"),
            "script must not delete $exe before the pending file is in place"
        );

        // The backup must be created (by moving $exe) before the pending move.
        let backup_pos = script
            .find("Move-Item -Path $exe -Destination $backup")
            .expect("script must back up $exe before moving $pending");
        let move_pending_pos = script
            .find("Move-Item -Path $pending -Destination $exe")
            .expect("script must move $pending to $exe");
        assert!(
            backup_pos < move_pending_pos,
            "backup of $exe must precede the move of $pending into place"
        );

        // On failure, the backup must be restored before rethrowing.
        assert!(
            script.contains("Move-Item -Path $backup -Destination $exe -Force"),
            "script must restore $exe from backup on failure"
        );

        // On success, the backup must be cleaned up.
        assert!(
            script.contains("Remove-Item $backup -Force"),
            "script must remove the backup after a successful update"
        );
    }
}

#[cfg(test)]
#[cfg(unix)]
mod unix_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn re_exec_path_uses_installed_binary_path() {
        let root = Path::new("/repo");
        assert_eq!(re_exec_path(root), root.join("bin").join("dotfiles"));
    }
}

#[cfg(test)]
mod startup_log_tests {
    use super::runner::startup_context_line;
    use super::*;
    use crate::infra::logging::Output;
    use crate::infra::platform::{Os, Platform};
    use std::path::Path;
    use std::sync::{Mutex, PoisonError};

    #[derive(Default)]
    struct CapturingOutput {
        always_lines: Mutex<Vec<String>>,
    }

    impl CapturingOutput {
        fn lines(&self) -> Vec<String> {
            self.always_lines
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }
    }

    impl Output for CapturingOutput {
        fn stage(&self, _msg: &str) {}

        fn info(&self, _msg: &str) {}

        fn debug(&self, _msg: &str) {}

        fn warn(&self, _msg: &str) {}

        fn error(&self, _msg: &str) {}

        fn dry_run(&self, _msg: &str) {}

        fn always(&self, msg: &str) {
            self.always_lines
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(msg.to_string());
        }
    }

    #[test]
    fn startup_context_uses_command_profile_platform_and_preview() {
        assert_eq!(
            startup_context_line(
                "Install",
                "workstation",
                Platform::new(Os::Linux, false),
                true,
            ),
            "Install · profile workstation · Linux · preview"
        );
    }

    #[test]
    fn overlay_path_is_optional_second_startup_line() {
        let log = CapturingOutput::default();

        log.always("Install · profile workstation · Linux");
        log_overlay_path(Some(Path::new("/private/overlay")), &log);

        let lines = log.lines();
        assert_eq!(
            lines,
            vec![
                "Install · profile workstation · Linux".to_string(),
                "\x1b[2moverlay\x1b[0m /private/overlay".to_string(),
            ],
            "overlay line must immediately follow startup context and must not be indented"
        );
    }

    #[test]
    fn absent_overlay_does_not_emit_separator() {
        let log = CapturingOutput::default();

        log.always("Install · profile workstation · Linux");
        log_overlay_path(None, &log);

        assert_eq!(
            log.lines(),
            vec!["Install · profile workstation · Linux".to_string()]
        );
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod task_graph_tests {
    use super::execution::{run_tasks_to_completion, run_tasks_to_completion_with_late_tasks};
    use crate::engine::{Context, Task, TaskId, TaskResult, task_deps};
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
        fn name(&self) -> &'static str {
            "cycle-a"
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
        fn name(&self) -> &'static str {
            "cycle-b"
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

        assert!(format!("{err:#}").contains("dependency cycle detected"));
        assert!(!ran_a.load(Ordering::SeqCst));
        assert!(!ran_b.load(Ordering::SeqCst));
    }

    struct PrerequisiteTask {
        name: &'static str,
        completed: Arc<AtomicUsize>,
    }

    impl Task for PrerequisiteTask {
        fn name(&self) -> &'static str {
            self.name
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
        fn name(&self) -> &'static str {
            "dependent"
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
        fn name(&self) -> &'static str {
            "boundary-prerequisite"
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
        fn name(&self) -> &'static str {
            "discovery-boundary"
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
        fn name(&self) -> &'static str {
            "remaining-static"
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
        fn name(&self) -> &'static str {
            "late-discovered"
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
        fn name(&self) -> &'static str {
            "failing-boundary"
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
        fn name(&self) -> &'static str {
            "static-after-provider"
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
