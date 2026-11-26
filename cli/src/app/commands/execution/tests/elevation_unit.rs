use super::*;
use crate::test_helpers::make_static_context;

#[test]
fn elevation_helper_failure_is_not_a_benign_skip() {
    use crate::infra::elevation::ElevationOutcome;

    let (_, log) = make_static_context("fixture-root".into(), None);
    for (result, expected) in [
        (Ok(ElevationOutcome::Completed), ElevationPlan::Delegated),
        (
            Ok(ElevationOutcome::Declined),
            ElevationPlan::Unavailable {
                reason: "elevation declined",
            },
        ),
        (
            Ok(ElevationOutcome::Failed(130)),
            ElevationPlan::Interrupted,
        ),
        (
            Ok(ElevationOutcome::Failed(7)),
            ElevationPlan::Failed {
                reason: "elevated step failed",
            },
        ),
        (
            Err(anyhow::anyhow!("fixture: helper unavailable")),
            ElevationPlan::Failed {
                reason: "failed to request elevation",
            },
        ),
    ] {
        assert_eq!(windows_elevation_result(&log, result), expected);
    }
}

#[cfg(windows)]
#[test]
fn windows_elevation_only_prompts_for_interactive_pending_work() {
    let (ctx, log) = make_static_context("fixture-root".into(), None);
    let ctx = ctx.with_non_interactive(true);
    assert_eq!(
        prepare_windows_elevation(&ctx, &log, &[], &[], None),
        ElevationPlan::Ready
    );
    assert_eq!(
        prepare_windows_elevation(
            &ctx,
            &log,
            &["Windows Developer Mode"],
            &["developer-mode"],
            None,
        ),
        ElevationPlan::Unavailable {
            reason: "elevation unavailable in a non-interactive session",
        }
    );
}

fn elevated_child_scope_result(command_args: &[&str], marker: bool) -> anyhow::Result<()> {
    use crate::app::cli::{Cli, Command, EngineCommand};

    let root = std::path::absolute("fixture-root").unwrap();
    let parsed = Cli::try_parse_from(
        std::iter::once("dotfiles")
            .chain(command_args.iter().copied())
            .map(str::to_string)
            .chain(["--root".to_string(), root.display().to_string()]),
    )
    .unwrap();
    let color = parsed.color.into_color_mode();
    let command = match parsed.command {
        Command::Update(opts) => {
            let (global, opts, verbose) = opts.into_engine_parts(color);
            EngineCommand::Update {
                global,
                opts,
                verbose,
            }
        }
        Command::Remove(opts) => {
            let (global, opts, verbose) = opts.into_engine_parts(color);
            EngineCommand::Remove {
                global,
                opts,
                verbose,
            }
        }
        Command::Check(opts) => {
            let (global, opts, verbose) = opts.into_engine_parts(color);
            EngineCommand::Check {
                global,
                opts,
                verbose,
            }
        }
        Command::List(_)
        | Command::Explain(_)
        | Command::Log(_)
        | Command::Completions(_)
        | Command::SystemFileHelper(_) => panic!("scope fixture must use an engine command"),
    };
    let mut env = crate::infra::env::MapEnv::new();
    if marker {
        env = env.with(crate::infra::elevation::ELEVATED_CHILD_VAR, "1");
    }
    let runtime = super::super::super::RuntimePolicy::new(
        command.global(),
        false,
        env.into_handle(),
        false,
        false,
    );
    validate_elevated_child_command(&command, &runtime)
}

#[test]
fn elevated_child_scope_accepts_machine_children_and_normal_user_runs() {
    for command_args in [
        vec!["update", "--elevated-child", "--only", "developer-mode"],
        vec!["update", "--only", "symlinks"],
    ] {
        elevated_child_scope_result(&command_args, false).unwrap();
    }
}

#[test]
fn elevated_child_scope_rejects_user_work_before_startup() {
    for (name, command_args, marker) in [
        ("unscoped child", vec!["update", "--elevated-child"], false),
        (
            "home child",
            vec!["update", "--elevated-child", "--only", "symlinks"],
            false,
        ),
        (
            "HKCU child",
            vec!["update", "--elevated-child", "--only", "registry"],
            false,
        ),
        (
            "mixed child",
            vec![
                "update",
                "--elevated-child",
                "--only",
                "developer-mode,symlinks",
            ],
            false,
        ),
        (
            "dependency expansion",
            vec![
                "update",
                "--elevated-child",
                "--only",
                "developer-mode",
                "--with-deps",
            ],
            false,
        ),
        (
            "skip filter",
            vec![
                "update",
                "--elevated-child",
                "--only",
                "developer-mode",
                "--skip",
                "symlinks",
            ],
            false,
        ),
        (
            "remove child",
            vec!["remove", "--elevated-child", "--only", "symlinks"],
            false,
        ),
        (
            "inherited marker",
            vec!["update", "--only", "registry"],
            true,
        ),
        ("inherited marker on check", vec!["check"], true),
    ] {
        assert!(
            elevated_child_scope_result(&command_args, marker).is_err(),
            "{name}"
        );
    }
}

#[test]
fn elevated_child_scope_requires_resolved_absolute_paths() {
    use crate::app::cli::{Cli, Command, EngineCommand};

    let root = std::path::absolute("fixture-root")
        .unwrap()
        .display()
        .to_string();
    let overlay = std::path::absolute("fixture-overlay")
        .unwrap()
        .display()
        .to_string();
    for (paths, allowed) in [
        (vec![], false),
        (vec!["--root", "relative-root"], false),
        (vec!["--root", root.as_str()], true),
        (
            vec!["--root", root.as_str(), "--overlay", "relative-overlay"],
            false,
        ),
        (
            vec!["--root", root.as_str(), "--overlay", overlay.as_str()],
            true,
        ),
    ] {
        let Command::Update(opts) = Cli::try_parse_from(
            [
                "dotfiles",
                "update",
                "--elevated-child",
                "--only",
                "developer-mode",
            ]
            .into_iter()
            .chain(paths),
        )
        .unwrap()
        .command
        else {
            panic!("path fixture must use update")
        };
        let (global, opts, verbose) =
            opts.into_engine_parts(crate::infra::logging::ColorMode::Auto);
        let command = EngineCommand::Update {
            global,
            opts,
            verbose,
        };
        let runtime = super::super::super::RuntimePolicy::new(
            command.global(),
            false,
            crate::infra::env::MapEnv::new().into_handle(),
            false,
            false,
        );
        assert_eq!(
            validate_elevated_child_command(&command, &runtime).is_ok(),
            allowed,
            "{command:?}"
        );
    }
}

#[test]
fn sudo_primes_foreground_credentials_for_single_and_sequential_tasks() {
    let cases: &[(&str, bool, &[&str])] = &[
        ("single parallel", true, &["System configuration files"]),
        ("single sequential", false, &["System configuration files"]),
        (
            "multiple sequential",
            false,
            &["System configuration files", "Packages"],
        ),
        (
            "multiple parallel",
            true,
            &["System configuration files", "Packages"],
        ),
    ];
    for &(case, parallel, names) in cases {
        let (ctx, log) = make_static_context("fixture-root".into(), None);
        let ctx = ctx.with_parallel(parallel).with_non_interactive(false);
        let primed = std::cell::Cell::new(false);
        let plan = prepare_sudo_elevation(
            &ctx,
            &log,
            names,
            true,
            || false,
            || {
                primed.set(true);
                Ok(true)
            },
        );

        assert_eq!(plan, ElevationPlan::Ready, "{case}");
        assert!(
            primed.get(),
            "{case}: uncached credentials must be primed before executor commands run"
        );
    }
}

#[test]
fn sudo_does_not_prompt_when_cached_or_non_interactive() {
    for (cached, non_interactive, expected) in [
        (true, false, ElevationPlan::Ready),
        (true, true, ElevationPlan::Ready),
        (
            false,
            true,
            ElevationPlan::Unavailable {
                reason: "sudo credentials unavailable in a non-interactive session",
            },
        ),
    ] {
        let (ctx, log) = make_static_context("fixture-root".into(), None);
        let ctx = ctx.with_non_interactive(non_interactive);
        let plan = prepare_sudo_elevation(
            &ctx,
            &log,
            &["System configuration files"],
            true,
            || cached,
            || panic!("cached or non-interactive runs must not prompt"),
        );
        assert_eq!(
            plan, expected,
            "cached={cached}, non_interactive={non_interactive}"
        );
    }
}

#[test]
fn sudo_unavailable_does_not_check_credentials_or_prompt() {
    let (ctx, log) = make_static_context("fixture-root".into(), None);
    let plan = prepare_sudo_elevation(
        &ctx,
        &log,
        &["System configuration files"],
        false,
        || panic!("missing sudo must not be invoked"),
        || panic!("missing sudo must not prompt"),
    );
    assert_eq!(
        plan,
        ElevationPlan::Unavailable {
            reason: "sudo credentials unavailable",
        }
    );
}

#[test]
fn sudo_interrupted_during_prime_returns_interrupted_without_auth_error() {
    for require_complete in [false, true] {
        let (ctx, log) = make_static_context("fixture-root".into(), None);
        let token = crate::engine::CancellationToken::new();
        let ctx = ctx
            .with_non_interactive(false)
            .with_require_complete(require_complete)
            .with_cancellation(token.clone());

        let plan = prepare_sudo_elevation(
            &ctx,
            &log,
            &["System configuration files"],
            true,
            || false,
            || {
                token.cancel();
                Ok(false)
            },
        );
        assert_eq!(
            plan,
            ElevationPlan::Interrupted,
            "require_complete={require_complete}"
        );
    }
}

#[test]
fn broker_records_interrupted_elevation_and_dependents_without_failures() {
    use crate::app::commands::error::CommandInterrupted;
    use crate::engine::{TaskMeta, TaskResult};

    struct StubTask {
        id: u8,
        name: &'static str,
        deps: Vec<TaskId>,
    }
    impl Task for StubTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new(self.name)
        }
        fn task_id(&self) -> TaskId {
            TaskId::dynamic::<Self>(self.id.to_string())
        }
        fn dependencies(&self) -> &[TaskId] {
            &self.deps
        }
        fn run(&self, _ctx: &Context) -> anyhow::Result<TaskResult> {
            panic!("interrupted task should not run");
        }
    }

    for require_complete in [false, true] {
        let root = StubTask {
            id: 1,
            name: "System configuration files",
            deps: Vec::new(),
        };
        let dependent = StubTask {
            id: 2,
            name: "Systemd units",
            deps: vec![root.task_id()],
        };
        let unrelated = StubTask {
            id: 3,
            name: "Home symlinks",
            deps: Vec::new(),
        };
        let mut tasks: Vec<&dyn Task> = vec![&root, &dependent, &unrelated];
        let assessments = HashMap::from([(
            root.task_id(),
            TaskAssessment::applicable().with_elevation(true),
        )]);
        let graph = ResolvedTaskGraph::resolve(&tasks).expect("valid graph");
        let (ctx, log) = make_static_context("fixture-root".into(), None);
        let token = crate::engine::CancellationToken::new();
        let ctx = ctx
            .with_require_complete(require_complete)
            .with_cancellation(token.clone());

        let summary = ElevationBroker::new(&ctx, &log, None).prepare_with(
            &mut tasks,
            &assessments,
            &graph,
            |_, _, _, _, _| {
                token.cancel();
                ElevationPlan::Interrupted
            },
        );

        assert_eq!(
            tasks.iter().map(|task| task.name()).collect::<Vec<_>>(),
            vec!["Home symlinks"],
            "require_complete={require_complete}"
        );
        assert_eq!(summary.failure_count(), 0);
        assert!(summary.was_interrupted());
        assert_eq!(
            summary.outcome(&root.task_id()),
            Some(TaskOutcome::Cancelled)
        );
        assert_eq!(
            summary.outcome(&dependent.task_id()),
            Some(TaskOutcome::Cancelled)
        );
        let entries = log.task_entries();
        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .all(|entry| entry.status == TaskStatus::Interrupted),
            "elevating task and dependents must be Interrupted: {entries:?}"
        );
        let finish_err =
            crate::app::commands::execution::finish_run(&ctx, &log, &summary).unwrap_err();
        assert!(
            finish_err.downcast_ref::<CommandInterrupted>().is_some(),
            "expected CommandInterrupted (exit 130), got {finish_err:#}"
        );
    }
}

struct RecordingStubTask {
    id: u8,
    name: &'static str,
    deps: Vec<TaskId>,
    executed: Arc<std::sync::atomic::AtomicBool>,
}

impl Task for RecordingStubTask {
    fn meta(&self) -> crate::engine::TaskMeta<'_> {
        crate::engine::TaskMeta::new(self.name)
    }
    fn task_id(&self) -> TaskId {
        TaskId::dynamic::<Self>(self.id.to_string())
    }
    fn dependencies(&self) -> &[TaskId] {
        &self.deps
    }
    fn run(&self, _ctx: &Context) -> anyhow::Result<crate::engine::TaskResult> {
        self.executed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        panic!(
            "task {} must not run after interrupted elevation",
            self.name
        );
    }
}

#[test]
fn interrupted_elevation_cancels_parent_token_and_skips_remaining_tasks() {
    use std::sync::atomic::{AtomicBool, Ordering};

    use crate::app::commands::error::CommandInterrupted;

    for (parallel, require_complete) in [(false, false), (false, true), (true, false), (true, true)]
    {
        let executed = Arc::new(AtomicBool::new(false));
        let root = RecordingStubTask {
            id: 1,
            name: "Windows Developer Mode",
            deps: Vec::new(),
            executed: Arc::clone(&executed),
        };
        let dependent = RecordingStubTask {
            id: 2,
            name: "Dependent task",
            deps: vec![root.task_id()],
            executed: Arc::clone(&executed),
        };
        let unrelated = RecordingStubTask {
            id: 3,
            name: "Home symlinks",
            deps: Vec::new(),
            executed: Arc::clone(&executed),
        };
        let mut tasks: Vec<&dyn Task> = vec![&root, &dependent, &unrelated];
        let mut assessments = HashMap::from([
            (
                root.task_id(),
                TaskAssessment::applicable().with_elevation(true),
            ),
            (dependent.task_id(), TaskAssessment::applicable()),
            (unrelated.task_id(), TaskAssessment::applicable()),
        ]);
        let graph = ResolvedTaskGraph::resolve_assessed(&tasks, &assessments).expect("valid graph");
        let (ctx, log) = make_static_context("fixture-root".into(), None);
        let ctx = ctx
            .with_parallel(parallel)
            .with_require_complete(require_complete)
            .with_cancellation(crate::engine::CancellationToken::new());

        // Return Interrupted without pre-cancelling the token, modeling a
        // Windows elevated child that exited with Ctrl-C (130).
        let mut summary = ElevationBroker::new(&ctx, &log, None).prepare_with(
            &mut tasks,
            &assessments,
            &graph,
            |_, _, _, _, _| ElevationPlan::Interrupted,
        );
        assert!(ctx.is_cancelled());

        let remaining_graph =
            ResolvedTaskGraph::resolve_assessed(&tasks, &assessments).expect("valid graph");
        assessments.retain(|id, _| remaining_graph.contains(id));
        let prior = summary.clone();
        summary.merge(crate::engine::scheduler::run_tasks(
            &tasks,
            &remaining_graph,
            &assessments,
            &ctx,
            &log,
            Some(&prior),
        ));

        assert!(!executed.load(Ordering::SeqCst));
        assert_eq!(summary.failure_count(), 0);
        assert!(summary.was_interrupted());
        for task_id in [root.task_id(), dependent.task_id(), unrelated.task_id()] {
            assert_eq!(summary.outcome(&task_id), Some(TaskOutcome::Cancelled));
        }
        let entries = log.task_entries();
        assert_eq!(entries.len(), 3);
        assert!(
            entries
                .iter()
                .all(|entry| entry.status == TaskStatus::Interrupted)
        );
        let finish_err =
            crate::app::commands::execution::finish_run(&ctx, &log, &summary).unwrap_err();
        assert!(finish_err.downcast_ref::<CommandInterrupted>().is_some());
    }
}

#[cfg(unix)]
#[test]
fn collect_utf8_args_rejects_non_utf8_without_panicking() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt as _;

    let invalid = OsString::from_vec(vec![b'b', b'a', b'd', 0xff]);
    let err = collect_utf8_args([OsString::from("update"), invalid.clone()]).unwrap_err();
    assert_eq!(err, invalid);
}

#[test]
fn sudo_priming_failure_leaves_elevation_unavailable() {
    for result in [Ok(false), Err(std::io::Error::other("fixture failure"))] {
        let (ctx, log) = make_static_context("fixture-root".into(), None);
        let ctx = ctx.with_parallel(false).with_non_interactive(false);
        let plan = prepare_sudo_elevation(
            &ctx,
            &log,
            &["System configuration files"],
            true,
            || false,
            || result,
        );
        assert_eq!(
            plan,
            ElevationPlan::Unavailable {
                reason: "sudo credentials unavailable",
            }
        );
    }
}

#[test]
fn elevation_disposition_distinguishes_delegation_unavailability_and_failure() {
    for strict in [false, true] {
        for (plan, expected) in [
            (ElevationPlan::Ready, None),
            (
                ElevationPlan::Delegated,
                Some((
                    "ran in elevated session",
                    TaskOutcome::Satisfied,
                    TaskStatus::Skipped,
                )),
            ),
            (
                ElevationPlan::Unavailable {
                    reason: "unavailable",
                },
                Some((
                    "unavailable",
                    TaskOutcome::Unmet,
                    if strict {
                        TaskStatus::Failed
                    } else {
                        TaskStatus::Skipped
                    },
                )),
            ),
            (
                ElevationPlan::Failed {
                    reason: "elevated step failed",
                },
                Some((
                    "elevated step failed",
                    TaskOutcome::Unmet,
                    TaskStatus::Failed,
                )),
            ),
        ] {
            assert_eq!(
                elevation_plan_disposition(plan, strict),
                expected,
                "{plan:?}, strict={strict}"
            );
        }
    }
}
