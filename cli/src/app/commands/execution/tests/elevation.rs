use super::*;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn elevated_child_args_scope_the_run_to_the_elevating_selectors() {
    let built = build_elevated_child_args(
        &args(&["update"]),
        &["developer-mode"],
        std::path::Path::new("fixture-root"),
        None,
        None,
    );

    assert_eq!(
        built,
        args(&[
            "update",
            "--only",
            "developer-mode",
            "--no-parallel",
            "--elevated-child",
            "--root",
            "fixture-root",
        ]),
    );
}

#[test]
fn elevated_child_args_preserve_configuration_flags() {
    let built = build_elevated_child_args(
        &args(&[
            "update",
            "--profile",
            "desktop",
            "--root",
            "relative-repo",
            "--overlay=relative-overlay",
        ]),
        &["developer-mode"],
        std::path::Path::new(r"C:\resolved repo"),
        Some(std::path::Path::new(r"\\server\resolved overlay")),
        Some("desktop"),
    );

    assert_eq!(
        built.iter().filter(|arg| *arg == "--profile").count(),
        1,
        "resolved profile should appear once: {built:?}"
    );
    assert!(
        built
            .windows(2)
            .any(|pair| pair == ["--profile", "desktop"])
    );
    assert!(built.contains(&"--only".to_string()));
    assert_eq!(
        &built[built.len() - 4..],
        &args(&[
            "--root",
            r"C:\resolved repo",
            "--overlay",
            r"\\server\resolved overlay"
        ])
    );
    assert!(!built.iter().any(|arg| arg.contains("relative-")));
}

#[test]
fn elevated_child_args_inject_and_replace_resolved_profile() {
    for (input, expect_dry_run) in [
        (args(&["update"]), false),
        (args(&["update", "-p", "old"]), false),
        (args(&["update", "-p=old"]), false),
        (args(&["update", "-pold"]), false),
        (args(&["update", "-np", "old"]), true),
        (args(&["update", "--profile", "old"]), false),
        (args(&["update", "--profile=old"]), false),
    ] {
        let built = build_elevated_child_args(
            &input,
            &["developer-mode"],
            std::path::Path::new("fixture-root"),
            None,
            Some("resolved"),
        );

        assert_eq!(
            built.iter().filter(|arg| *arg == "--profile").count(),
            1,
            "input={input:?}, built={built:?}"
        );
        assert!(
            built
                .windows(2)
                .any(|pair| pair == ["--profile", "resolved"]),
            "input={input:?}, built={built:?}"
        );
        assert!(
            !built.iter().any(|arg| arg.contains("old")),
            "stale profile must be stripped: input={input:?}, built={built:?}"
        );
        assert_eq!(
            built.iter().any(|arg| arg == "-n"),
            expect_dry_run,
            "short cluster flags before -p must be preserved: input={input:?}, built={built:?}"
        );
    }
}

#[test]
fn elevated_child_args_drop_inherited_task_filters() {
    for filters in [
        args(&[
            "update",
            "--only",
            "packages",
            "--skip",
            "registry",
            "--root",
            "relative-root",
            "--overlay",
            "relative-overlay",
        ]),
        args(&[
            "update",
            "--only=packages",
            "--skip=registry",
            "--root=relative-root",
            "--overlay=relative-overlay",
        ]),
    ] {
        let built = build_elevated_child_args(
            &filters,
            &["developer-mode"],
            std::path::Path::new("fixture-root"),
            None,
            None,
        );

        assert_eq!(built.iter().filter(|arg| *arg == "--only").count(), 1);
        assert!(!built.iter().any(|arg| arg.contains("packages")));
        assert!(!built.iter().any(|arg| arg.contains("registry")));
        assert!(!built.iter().any(|arg| arg.contains("relative-")));
        assert!(!built.iter().any(|arg| arg == "--overlay"));
    }
}

#[test]
fn elevated_child_selection_does_not_expand_the_parents_dependency_closure() {
    let built = build_elevated_child_args(
        &args(&[
            "update",
            "--only",
            "developer-mode",
            "--with-deps",
            "--skip",
            "symlinks",
        ]),
        &["developer-mode"],
        std::path::Path::new("fixture-root"),
        None,
        None,
    );
    let parsed =
        crate::app::cli::Cli::try_parse_from(std::iter::once("dotfiles".to_string()).chain(built))
            .expect("elevated arguments must remain valid CLI input");
    let crate::app::cli::Command::Update(opts) = parsed.command else {
        panic!("elevation must preserve the install command");
    };
    assert!(
        !opts.tasks.with_deps,
        "a child must not rediscover or expand the parent's resolved scope"
    );
    let store = crate::app::config::store::ConfigStore::from_config(empty_config(
        std::path::PathBuf::from("fixture-root"),
    ));
    let tasks = crate::app::catalog::all_update_tasks(&store);
    let log = Logger::test();
    let selected = crate::app::filter::apply_task_filters(
        &tasks,
        &[],
        &opts.tasks.filters.only,
        &opts.tasks.filters.skip,
        opts.tasks.with_deps,
        &log,
    )
    .expect("child task selection must succeed");

    assert_eq!(
        selected
            .iter()
            .map(|task| task.selector())
            .collect::<Vec<_>>(),
        ["developer-mode"],
        "only the parent's exact delegated tasks may run elevated"
    );
}

#[test]
fn elevated_child_args_do_not_duplicate_repeated_flags() {
    let built = build_elevated_child_args(
        &args(&["update", "--no-parallel", "--elevated-child"]),
        &["developer-mode"],
        std::path::Path::new("fixture-root"),
        None,
        None,
    );

    assert_eq!(
        built.iter().filter(|arg| *arg == "--no-parallel").count(),
        1
    );
    assert_eq!(
        built
            .iter()
            .filter(|arg| *arg == "--elevated-child")
            .count(),
        1
    );
}

#[cfg(unix)]
#[test]
fn unavailable_developer_mode_does_not_block_unprivileged_symlink_work() {
    use crate::app::config::store::ConfigStore;
    use crate::domains::files::config::symlinks::Symlink;
    use crate::infra::logging::TaskStatus;
    use crate::test_helpers::make_windows_context;

    for parallel in [false, true] {
        for (is_file, existing) in [(false, false), (true, true), (true, false)] {
            let fixture = tempfile::tempdir_in(".").unwrap();
            let root = crate::infra::fs::canonicalize(fixture.path()).unwrap();
            let source = root.join("symlinks/example");
            let home = root.join("home");
            std::fs::create_dir_all(root.join("symlinks")).unwrap();
            std::fs::create_dir(&home).unwrap();
            if is_file {
                std::fs::write(&source, "managed content").unwrap();
            } else {
                std::fs::create_dir(&source).unwrap();
            }
            if existing {
                std::os::unix::fs::symlink(&source, home.join(".example")).unwrap();
            }
            let mut config = empty_config(root);
            config.symlinks.push(Symlink {
                source: "example".into(),
                target: None,
                allow_external_target: false,
                origin: None,
            });
            let log = Arc::new(Logger::test());
            let ctx = make_windows_context(config.root.clone(), config.overlay.clone())
                .with_home(home.clone())
                .with_log(Arc::<Logger>::clone(&log))
                .with_non_interactive(true)
                .with_parallel(parallel);
            let tasks = crate::app::catalog::all_update_tasks(&ConfigStore::from_config(config));
            let selected = tasks
                .iter()
                .filter(|task| matches!(task.selector(), "developer-mode" | "symlinks"))
                .map(Box::as_ref)
                .collect::<Vec<_>>();
            let symlinks = selected
                .iter()
                .find(|task| task.selector() == "symlinks")
                .unwrap();
            let unavailable = is_file && !existing;
            assert!(
                !symlinks.needs_elevation(&ctx),
                "home links must never run under a potentially different UAC account"
            );

            run_tasks_to_completion(selected, &ctx, &log).unwrap();

            let entries = log.task_entries();
            let symlink_entry = entries
                .iter()
                .find(|entry| entry.name == "Home symlinks")
                .unwrap();
            assert_eq!(
                symlink_entry.status,
                if unavailable {
                    TaskStatus::Skipped
                } else if existing {
                    TaskStatus::Ok
                } else {
                    TaskStatus::Changed
                },
                "parallel={parallel}, is_file={is_file}, existing={existing}"
            );
            assert_eq!(
                home.join(".example").exists(),
                !unavailable,
                "unprivileged links must converge; pending privileged links must remain skipped"
            );
        }
    }
}

#[test]
fn blocked_dependents_cascades_to_transitive_dependents() {
    // Mirrors the real Windows shape: symlinks cannot run unelevated, and
    // APM declares a dependency on it in the catalog.
    let trace = trace();
    let root = ProbeTask::new("Home symlinks", 1, &trace);
    let direct = ProbeTask::new("Agent packages", 2, &trace).depends_on(&[1]);
    let indirect = ProbeTask::new("APM updates", 3, &trace).depends_on(&[2]);
    let unrelated = ProbeTask::new("Registry settings", 4, &trace);
    let tasks: Vec<&dyn Task> = vec![&root, &direct, &indirect, &unrelated];
    let roots = HashMap::from([(numeric_task_id(1), "Home symlinks")]);

    let blocked = blocked_dependents(&tasks, &roots);

    // The reason names the root so the message stays actionable, rather
    // than pointing at the intermediate hop.
    assert_eq!(blocked.get(&numeric_task_id(2)), Some(&"Home symlinks"));
    assert_eq!(blocked.get(&numeric_task_id(3)), Some(&"Home symlinks"));
    assert!(!blocked.contains_key(&numeric_task_id(4)));
    assert!(!blocked.contains_key(&numeric_task_id(1)));
}

#[test]
fn blocked_dependents_is_independent_of_slice_order() {
    // The task slice is not topologically sorted, so a dependent can be
    // visited before the dependency that blocks it.
    let trace = trace();
    let root = ProbeTask::new("Home symlinks", 1, &trace);
    let direct = ProbeTask::new("Agent packages", 2, &trace).depends_on(&[1]);
    let indirect = ProbeTask::new("APM updates", 3, &trace).depends_on(&[2]);
    let tasks: Vec<&dyn Task> = vec![&indirect, &direct, &root];
    let roots = HashMap::from([(numeric_task_id(1), "Home symlinks")]);

    let blocked = blocked_dependents(&tasks, &roots);

    assert_eq!(blocked.get(&numeric_task_id(3)), Some(&"Home symlinks"));
    assert_eq!(blocked.len(), 2);
}

#[test]
fn blocked_dependents_ignores_dependencies_absent_from_the_slice() {
    let trace = trace();
    let orphan = ProbeTask::new("Registry settings", 2, &trace).depends_on(&[99]);
    let tasks: Vec<&dyn Task> = vec![&orphan];
    let roots = HashMap::from([(numeric_task_id(1), "Home symlinks")]);

    assert!(blocked_dependents(&tasks, &roots).is_empty());
}

#[test]
fn unavailable_elevation_prunes_roots_and_transitive_dependents() {
    let trace = trace();
    let root = ProbeTask::new("Home symlinks", 1, &trace);
    let dependent = ProbeTask::new("Agent packages", 2, &trace).depends_on(&[1]);
    let transitive = ProbeTask::new("APM updates", 3, &trace).depends_on(&[2]);
    let unrelated = ProbeTask::new("Registry settings", 4, &trace);
    let mut tasks: Vec<&dyn Task> = vec![&root, &dependent, &transitive, &unrelated];
    let assessments = HashMap::from([(
        root.task_id(),
        TaskAssessment::applicable().with_elevation(true),
    )]);
    let (ctx, log) = execution_context(false);
    let ctx = ctx.with_non_interactive(true);

    let graph = crate::engine::graph::ResolvedTaskGraph::resolve(&tasks).expect("valid graph");
    let summary = ElevationBroker::new(&ctx, &log, None).prepare(&mut tasks, &assessments, &graph);

    assert_eq!(
        tasks.iter().map(|task| task.name()).collect::<Vec<_>>(),
        vec!["Registry settings"]
    );
    assert_eq!(summary.outcome(&root.task_id()), Some(TaskOutcome::Unmet));
    assert_eq!(
        summary.outcome(&dependent.task_id()),
        Some(TaskOutcome::Blocked)
    );
    assert_eq!(
        summary.outcome(&transitive.task_id()),
        Some(TaskOutcome::Blocked)
    );
    assert_eq!(summary.failure_count(), 0);
}

#[test]
fn unavailable_elevation_preserves_task_presentation_metadata() {
    use crate::infra::logging::TaskStatus;

    struct PresentedTask;
    impl Task for PresentedTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("Presented task")
                .with_selector("presented")
                .with_visibility(crate::engine::TaskVisibility::Internal)
                .with_result_display(crate::engine::TaskResultDisplay::RestartNotice)
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            panic!("unavailable elevation must prevent execution");
        }
    }

    for strict in [false, true] {
        let task = PresentedTask;
        let mut tasks: Vec<&dyn Task> = vec![&task];
        let assessments = HashMap::from([(
            task.task_id(),
            TaskAssessment::applicable().with_elevation(true),
        )]);
        let graph = crate::engine::graph::ResolvedTaskGraph::resolve(&tasks).expect("valid graph");
        let (ctx, log) = execution_context(false);
        let ctx = ctx.with_non_interactive(true).with_require_complete(strict);
        let summary =
            ElevationBroker::new(&ctx, &log, None).prepare(&mut tasks, &assessments, &graph);

        assert!(tasks.is_empty());
        assert_eq!(summary.outcome(&task.task_id()), Some(TaskOutcome::Unmet));
        let recorded = log.task_entries();
        let entry = recorded.first().expect("elevation result must be recorded");
        assert_eq!(entry.task_id, task.log_key());
        assert_eq!(entry.name, task.name());
        assert_eq!(entry.selector.as_deref(), Some("presented"));
        assert_eq!(entry.visibility, crate::engine::TaskVisibility::Internal);
        assert_eq!(
            entry.result_display,
            crate::engine::TaskResultDisplay::RestartNotice
        );
        assert_eq!(
            entry.status,
            if strict {
                TaskStatus::Failed
            } else {
                TaskStatus::Skipped
            }
        );
    }
}

fn elevated_child_context() -> (Context, Arc<Logger>) {
    let global = super::super::super::tests::global(&["--elevated-child", "--non-interactive"]);
    let runtime = crate::app::commands::RuntimePolicy::new(
        &global,
        false,
        crate::infra::env::MapEnv::new()
            .with("HOME", "/fixture-home")
            .into_handle(),
        false,
        false,
    );
    let (base, log) = execution_context(false);
    let ctx = Context::new_with_policy(
        base.root().to_path_buf(),
        None,
        base.platform(),
        Arc::<Logger>::clone(&log),
        base.executor_arc(),
        Arc::clone(&runtime.env),
        runtime.execution,
    )
    .unwrap();
    (ctx, log)
}

#[test]
fn elevated_child_does_not_request_elevation_again() {
    let trace = trace();
    let task = ProbeTask::new("Home symlinks", 1, &trace);
    let mut tasks: Vec<&dyn Task> = vec![&task];
    let assessments = HashMap::from([(
        task.task_id(),
        TaskAssessment::applicable().with_elevation(true),
    )]);
    let (ctx, log) = elevated_child_context();
    let graph = crate::engine::graph::ResolvedTaskGraph::resolve(&tasks).unwrap();
    let summary = ElevationBroker::new(&ctx, &log, None).prepare(&mut tasks, &assessments, &graph);
    assert_eq!(
        tasks.len(),
        1,
        "the child must execute its own elevating task"
    );
    assert_eq!(summary.failure_count(), 0);
    assert_eq!(
        summary.outcome(&task.task_id()),
        None,
        "no delegation or unmet result"
    );
}

#[test]
fn elevated_child_rejects_user_scoped_graphs_before_any_task_runs() {
    struct ScopedTask {
        selector: &'static str,
        ran: Arc<AtomicUsize>,
    }
    impl Task for ScopedTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new(self.selector).with_selector(self.selector)
        }
        fn task_id(&self) -> TaskId {
            TaskId::dynamic::<Self>(self.selector)
        }
        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.ran.fetch_add(1, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    for parallel in [false, true] {
        for selector in ["developer-mode", "symlinks", "registry"] {
            let ran = Arc::new(AtomicUsize::new(0));
            let machine = ScopedTask {
                selector: "developer-mode",
                ran: Arc::clone(&ran),
            };
            let selected = ScopedTask {
                selector,
                ran: Arc::clone(&ran),
            };
            let (ctx, log) = elevated_child_context();
            let ctx = ctx.with_parallel(parallel);
            let tasks: Vec<&dyn Task> = if selector == "developer-mode" {
                vec![&machine]
            } else {
                vec![&machine, &selected]
            };
            let result = run_tasks_to_completion(tasks, &ctx, &log);
            assert_eq!(result.is_ok(), selector == "developer-mode", "{selector}");
            assert_eq!(
                ran.load(Ordering::SeqCst),
                usize::from(selector == "developer-mode")
            );
            if selector != "developer-mode" {
                assert!(
                    log.task_entries().is_empty(),
                    "reject the whole graph before scheduling"
                );
            }
        }
    }
}

#[test]
fn windows_broker_does_not_delegate_user_scoped_assessments() {
    let trace = trace();
    let task = ProbeTask::new("Home symlinks", 1, &trace);
    let mut tasks: Vec<&dyn Task> = vec![&task];
    let assessments = HashMap::from([(
        task.task_id(),
        TaskAssessment::applicable().with_elevation(true),
    )]);
    let graph = crate::engine::graph::ResolvedTaskGraph::resolve(&tasks).unwrap();
    let (base, log) = execution_context(false);
    let ctx = crate::test_helpers::make_windows_context(base.root().to_path_buf(), None)
        .with_non_interactive(true);
    let summary = ElevationBroker::new(&ctx, &log, None).prepare(&mut tasks, &assessments, &graph);
    assert_eq!(
        tasks.len(),
        1,
        "user-scoped tasks must remain in the parent"
    );
    assert_eq!(summary.outcome(&task.task_id()), None);
}

#[test]
fn strict_completion_counts_unavailable_elevation_as_failure() {
    let trace = trace();
    let root = ProbeTask::new("Home symlinks", 1, &trace);
    let mut tasks: Vec<&dyn Task> = vec![&root];
    let assessments = HashMap::from([(
        root.task_id(),
        TaskAssessment::applicable().with_elevation(true),
    )]);
    let (ctx, log) = execution_context(false);
    let ctx = ctx.with_non_interactive(true).with_require_complete(true);

    let graph = crate::engine::graph::ResolvedTaskGraph::resolve(&tasks).expect("valid graph");
    let summary = ElevationBroker::new(&ctx, &log, None).prepare(&mut tasks, &assessments, &graph);

    assert!(tasks.is_empty());
    assert_eq!(summary.failure_count(), 1);
    assert_eq!(summary.outcome(&root.task_id()), Some(TaskOutcome::Unmet));
}

#[test]
fn cancellation_during_assessment_stops_remaining_task_assessments() {
    use crate::app::commands::error::CommandInterrupted;

    struct CancellingAssessTask {
        token: crate::engine::CancellationToken,
    }
    impl Task for CancellingAssessTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("Cancelling assess")
        }
        fn should_run(&self, _ctx: &Context) -> bool {
            self.token.cancel();
            true
        }
        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            panic!("cancelled task must not run");
        }
    }

    struct MustNotAssessTask {
        assessed: Arc<AtomicUsize>,
    }
    impl Task for MustNotAssessTask {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("Later task")
        }
        fn should_run(&self, _ctx: &Context) -> bool {
            self.assessed.fetch_add(1, Ordering::SeqCst);
            true
        }
        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            panic!("cancelled task must not run");
        }
    }

    let token = crate::engine::CancellationToken::new();
    let assessed = Arc::new(AtomicUsize::new(0));
    let first = CancellingAssessTask {
        token: token.clone(),
    };
    let second = MustNotAssessTask {
        assessed: Arc::clone(&assessed),
    };
    let (ctx, log) = execution_context(false);
    let ctx = ctx.with_cancellation(token);

    let tasks: Vec<&dyn Task> = vec![&first, &second];
    let err = run_tasks_to_completion(tasks, &ctx, &log).unwrap_err();
    assert!(
        err.downcast_ref::<CommandInterrupted>().is_some(),
        "expected CommandInterrupted, got {err:#}"
    );
    assert_eq!(
        assessed.load(Ordering::SeqCst),
        0,
        "remaining tasks must not be assessed once cancelled"
    );
}

#[test]
fn failed_elevated_child_is_preserved_even_when_parent_is_cancelled() {
    use super::super::elevation::ElevationPlan;
    use crate::infra::logging::TaskStatus;

    let trace = trace();
    let root = ProbeTask::new("Developer Mode", 1, &trace);
    let dependent = ProbeTask::new("Home symlinks", 2, &trace).depends_on(&[1]);
    let mut tasks: Vec<&dyn Task> = vec![&root, &dependent];
    let assessments = HashMap::from([(
        root.task_id(),
        TaskAssessment::applicable().with_elevation(true),
    )]);
    let graph = crate::engine::graph::ResolvedTaskGraph::resolve(&tasks).expect("valid graph");
    let token = crate::engine::CancellationToken::new();
    let (ctx, log) = execution_context(false);
    let ctx = ctx.with_cancellation(token.clone());

    let summary = ElevationBroker::new(&ctx, &log, None).prepare_with(
        &mut tasks,
        &assessments,
        &graph,
        |_, _, _, _, _| {
            token.cancel();
            ElevationPlan::Failed {
                reason: "elevated step failed",
            }
        },
    );

    assert!(tasks.is_empty());
    assert_eq!(summary.failure_count(), 1);
    assert_eq!(summary.outcome(&root.task_id()), Some(TaskOutcome::Unmet));
    assert_eq!(
        summary.outcome(&dependent.task_id()),
        Some(TaskOutcome::Blocked)
    );
    let root_entry = log
        .task_entries()
        .into_iter()
        .find(|entry| entry.name == "Developer Mode")
        .expect("root failure must be logged");
    assert_eq!(root_entry.status, TaskStatus::Failed);
    assert_eq!(root_entry.message.as_deref(), Some("elevated step failed"));
}

#[test]
fn windows_elevation_maps_child_and_parent_interrupts() {
    use super::super::elevation::{ElevationPlan, windows_elevation_result};
    use crate::infra::elevation::ElevationOutcome;
    use crate::infra::exec::{ExecError, ExecResult};

    let log = Logger::test();
    assert_eq!(
        windows_elevation_result(&log, Ok(ElevationOutcome::Failed(130))),
        ElevationPlan::Interrupted
    );
    let parent_cancelled = anyhow::Error::new(ExecError::Cancelled {
        command: "pwsh".into(),
        result: ExecResult::failure("", "", None),
    })
    .context("failed to start elevated process");
    assert_eq!(
        windows_elevation_result(&log, Err(parent_cancelled)),
        ElevationPlan::Interrupted
    );
    let direct_cancelled = anyhow::Error::new(ExecError::Cancelled {
        command: "pwsh".into(),
        result: ExecResult::failure("", "", None),
    });
    assert_eq!(
        windows_elevation_result(&log, Err(direct_cancelled)),
        ElevationPlan::Interrupted
    );
    assert_eq!(
        windows_elevation_result(&log, Ok(ElevationOutcome::Failed(1))),
        ElevationPlan::Failed {
            reason: "elevated step failed",
        }
    );
    let helper_timeout = anyhow::Error::new(ExecError::TimedOut {
        command: "pwsh".into(),
        timeout: std::time::Duration::from_secs(30),
        result: ExecResult::failure("", "", None),
    })
    .context("failed to start elevated process");
    assert_eq!(
        windows_elevation_result(&log, Err(helper_timeout)),
        ElevationPlan::Failed {
            reason: "failed to request elevation",
        }
    );
}

#[test]
fn windows_elevation_broker_distinguishes_parent_cancel_from_failure() {
    use super::super::elevation::windows_elevation_result;
    use crate::infra::exec::{ExecError, ExecResult};
    use crate::infra::logging::TaskStatus;

    for cancelled_err in [true, false] {
        let trace = trace();
        let root = ProbeTask::new("Developer Mode", 1, &trace);
        let dependent = ProbeTask::new("Home symlinks", 2, &trace).depends_on(&[1]);
        let mut tasks: Vec<&dyn Task> = vec![&root, &dependent];
        let assessments = HashMap::from([(
            root.task_id(),
            TaskAssessment::applicable().with_elevation(true),
        )]);
        let graph = crate::engine::graph::ResolvedTaskGraph::resolve(&tasks).expect("valid graph");
        let token = crate::engine::CancellationToken::new();
        let (ctx, broker_log) = execution_context(false);
        let ctx = ctx.with_cancellation(token.clone());

        let summary = ElevationBroker::new(&ctx, &broker_log, None).prepare_with(
            &mut tasks,
            &assessments,
            &graph,
            |_, target_log, _, _, _| {
                token.cancel();
                let result = if cancelled_err {
                    Err(anyhow::Error::new(ExecError::Cancelled {
                        command: "pwsh".into(),
                        result: ExecResult::failure("", "", None),
                    })
                    .context("failed to start elevated process"))
                } else {
                    Err(anyhow::anyhow!("fixture: helper failed to start"))
                };
                windows_elevation_result(target_log, result)
            },
        );

        assert!(tasks.is_empty());
        let root_entry = broker_log
            .task_entries()
            .into_iter()
            .find(|entry| entry.name == "Developer Mode")
            .expect("root result must be logged");
        if cancelled_err {
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
            assert_eq!(root_entry.status, TaskStatus::Interrupted);
        } else {
            assert_eq!(summary.failure_count(), 1);
            assert_eq!(summary.outcome(&root.task_id()), Some(TaskOutcome::Unmet));
            assert_eq!(
                summary.outcome(&dependent.task_id()),
                Some(TaskOutcome::Blocked)
            );
            assert_eq!(root_entry.status, TaskStatus::Failed);
            assert_eq!(
                root_entry.message.as_deref(),
                Some("failed to request elevation")
            );
        }
    }
}

#[test]
fn elevated_child_args_place_injected_flags_before_double_dash_terminator() {
    let built = build_elevated_child_args(
        &args(&["update", "--"]),
        &["developer-mode"],
        std::path::Path::new("fixture-root"),
        Some(std::path::Path::new("fixture-overlay")),
        Some("desktop"),
    );
    let linked = crate::infra::logging::records::child_args(&built, "parent-run");

    let terminator = linked
        .iter()
        .position(|arg| arg == "--")
        .expect("trailing terminator should be preserved");
    let before = &linked[..terminator];
    assert!(
        before
            .windows(2)
            .any(|pair| pair == ["--profile", "desktop"])
    );
    assert!(
        before
            .windows(2)
            .any(|pair| pair == ["--only", "developer-mode"])
    );
    assert!(before.iter().any(|arg| arg == "--no-parallel"));
    assert!(before.iter().any(|arg| arg == "--elevated-child"));
    assert!(
        before
            .windows(2)
            .any(|pair| pair == ["--root", "fixture-root"])
    );
    assert!(
        before
            .windows(2)
            .any(|pair| pair == ["--overlay", "fixture-overlay"])
    );
    assert!(
        before
            .windows(2)
            .any(|pair| pair == ["--parent-run-id", "parent-run"])
    );
    assert_eq!(&linked[terminator..], &["--"]);

    let parsed =
        crate::app::cli::Cli::try_parse_from(std::iter::once("dotfiles".to_string()).chain(linked))
            .expect("elevated child args with trailing `--` must remain valid CLI input");
    assert_eq!(parsed.parent_run_id.as_deref(), Some("parent-run"));
    let crate::app::cli::Command::Update(opts) = parsed.command else {
        panic!("expected Update command");
    };
    assert_eq!(opts.repository.profile.as_deref(), Some("desktop"));
    assert!(opts.elevated_child);
}
