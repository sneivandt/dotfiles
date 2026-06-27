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
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod task_graph_tests {
    use super::*;
    use crate::tasks::{
        TaskResult, task_deps,
        test_helpers::{empty_config, make_static_context},
    };
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

        fn phase(&self) -> TaskPhase {
            TaskPhase::Provision
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

        fn phase(&self) -> TaskPhase {
            TaskPhase::Provision
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

        let err = run_tasks_to_completion([&task_a as &dyn Task, &task_b as &dyn Task], &ctx, &log)
            .expect_err("cyclic task graphs should fail fast");

        assert!(format!("{err:#}").contains("dependency cycle detected"));
        assert!(!ran_a.load(Ordering::SeqCst));
        assert!(!ran_b.load(Ordering::SeqCst));
    }

    struct BootstrapMarkTask {
        name: &'static str,
        completed: Arc<AtomicUsize>,
    }

    impl Task for BootstrapMarkTask {
        fn name(&self) -> &'static str {
            self.name
        }

        fn phase(&self) -> TaskPhase {
            TaskPhase::Bootstrap
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.completed.fetch_add(1, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    struct ProvisionAfterBootstrapTask {
        ran: Arc<AtomicBool>,
        completed_bootstrap: Arc<AtomicUsize>,
        expected_bootstrap_count: usize,
    }

    impl Task for ProvisionAfterBootstrapTask {
        fn name(&self) -> &'static str {
            "provision-after-bootstrap"
        }

        fn phase(&self) -> TaskPhase {
            TaskPhase::Provision
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.ran.store(true, Ordering::SeqCst);
            let done = self.completed_bootstrap.load(Ordering::SeqCst);
            if done != self.expected_bootstrap_count {
                return Ok(TaskResult::Failed(format!(
                    "provision started before bootstrap completed: {done}/{}",
                    self.expected_bootstrap_count
                )));
            }
            Ok(TaskResult::Ok)
        }
    }

    #[test]
    fn run_tasks_to_completion_completes_bootstrap_phase_before_provision() {
        let (ctx, log) = make_static_context(empty_config(PathBuf::from("/repo")));
        let ctx = ctx.with_parallel(true);

        let completed_bootstrap = Arc::new(AtomicUsize::new(0));
        let provision_ran = Arc::new(AtomicBool::new(false));

        let bootstrap = BootstrapMarkTask {
            name: "bootstrap-mark",
            completed: Arc::clone(&completed_bootstrap),
        };
        let provision = ProvisionAfterBootstrapTask {
            ran: Arc::clone(&provision_ran),
            completed_bootstrap: Arc::clone(&completed_bootstrap),
            expected_bootstrap_count: 1,
        };

        // Intentionally pass provision first to ensure phase gating, not input
        // order, controls execution.
        run_tasks_to_completion(
            [&provision as &dyn Task, &bootstrap as &dyn Task],
            &ctx,
            &log,
        )
        .expect("phase barriers should run all bootstrap tasks before provision");

        assert_eq!(completed_bootstrap.load(Ordering::SeqCst), 1);
        assert!(provision_ran.load(Ordering::SeqCst));
    }
}
