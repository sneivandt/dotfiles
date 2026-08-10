//! Dependency resolution, ordering, and blocking behaviour.

use super::*;

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

#[test]
fn independent_task_runs_normally() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let ran = Arc::new(AtomicBool::new(false));
    let task = FlagTask {
        ran: Arc::clone(&ran),
    };

    run_test_tasks(&[&task], &ctx, &log);

    assert!(
        ran.load(Ordering::SeqCst),
        "independent task should have run"
    );
}

#[test]
fn dependent_task_is_skipped_when_dependency_panics() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let ran = Arc::new(AtomicBool::new(false));
    let panic_task = PanicTask;
    let dep_task = DepOnPanicTask {
        ran: Arc::clone(&ran),
    };

    run_test_tasks(&[&panic_task, &dep_task], &ctx, &log);

    assert!(
        !ran.load(Ordering::SeqCst),
        "dependent task should not have run"
    );
    let entries = log.task_entries();
    assert!(
        entries
            .iter()
            .any(|e| e.name == "panic-task" && e.status == TaskStatus::Failed),
        "panicked task should be recorded as Failed"
    );
    assert!(
        entries
            .iter()
            .any(|e| e.name == "dep-on-panic" && e.status == TaskStatus::Skipped),
        "dependent task should be recorded as Skipped"
    );
}

#[test]
fn dependent_task_is_skipped_when_dependency_fails() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let ran = Arc::new(AtomicBool::new(false));
    let failed_task = FailedTask;
    let dep_task = DepOnFailedTask {
        ran: Arc::clone(&ran),
    };

    run_test_tasks(&[&failed_task, &dep_task], &ctx, &log);

    assert!(
        !ran.load(Ordering::SeqCst),
        "dependent task should not have run"
    );
    let entries = log.task_entries();
    assert!(
        entries
            .iter()
            .any(|e| e.name == "failed-task" && e.status == TaskStatus::Failed),
        "failed task should be recorded as Failed"
    );
    assert!(
        entries
            .iter()
            .any(|e| e.name == "dep-on-failed" && e.status == TaskStatus::Skipped),
        "dependent task should be recorded as Skipped"
    );
}

#[test]
fn ordering_dependency_failure_does_not_block_successor() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let ran = Arc::new(AtomicBool::new(false));
    let failed_task = FailedTask;
    let ordered_task = OrderedAfterFailedTask {
        ran: Arc::clone(&ran),
    };
    let tasks: Vec<&dyn Task> = vec![&ordered_task, &failed_task];
    let graph = ResolvedTaskGraph::resolve(&tasks).unwrap();
    let assessments = tasks
        .iter()
        .map(|task| (task.task_id(), task.assess(&ctx)))
        .collect();

    let summary = run_tasks_parallel(&tasks, &graph, &assessments, &ctx, &log);

    assert!(
        ran.load(Ordering::SeqCst),
        "ordering-only successors must run after a failed predecessor completes"
    );
    assert_eq!(
        summary.failure_count(),
        1,
        "the execution summary must retain the predecessor failure"
    );
    let entries = log.task_entries();
    assert!(
        entries
            .iter()
            .any(|entry| entry.name == "ordered-after-failed" && entry.status == TaskStatus::Ok)
    );
}

#[test]
fn dynamic_tasks_with_the_same_display_name_keep_distinct_records() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let successful = SameNameDynamicTask {
        key: "successful",
        fails: false,
    };
    let failed = SameNameDynamicTask {
        key: "failed",
        fails: true,
    };

    run_test_tasks(&[&successful, &failed], &ctx, &log);

    let entries: Vec<_> = log
        .task_entries()
        .into_iter()
        .filter(|entry| entry.name == "same-display-name")
        .collect();
    assert_eq!(entries.len(), 2);
    assert_ne!(entries[0].task_id, entries[1].task_id);
    assert!(entries.iter().any(|entry| entry.status == TaskStatus::Ok));
    assert!(
        entries
            .iter()
            .any(|entry| entry.status == TaskStatus::Failed)
    );
}

#[test]
fn skipped_dependency_satisfies_dependent_task() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let ran = Arc::new(AtomicBool::new(false));
    let skipped_task = SkippedTask;
    let dep_task = DepOnSkippedTask {
        ran: Arc::clone(&ran),
    };

    run_test_tasks(&[&skipped_task, &dep_task], &ctx, &log);

    assert!(
        ran.load(Ordering::SeqCst),
        "deliberately skipped dependencies should not block dependents"
    );
    let entries = log.task_entries();
    assert!(
        entries
            .iter()
            .any(|e| e.name == "skipped-task" && e.status == TaskStatus::Skipped),
        "dependency should be recorded as Skipped"
    );
    assert!(
        entries
            .iter()
            .any(|e| e.name == "dep-on-skipped" && e.status == TaskStatus::Ok),
        "dependent task should be recorded as Ok"
    );
}

#[test]
fn failure_propagates_through_dependency_chain() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let ran_b = Arc::new(AtomicBool::new(false));
    let ran_c = Arc::new(AtomicBool::new(false));
    let panic_task = PanicTask;
    let chain_b = ChainB {
        ran: Arc::clone(&ran_b),
    };
    let chain_c = ChainC {
        ran: Arc::clone(&ran_c),
    };

    run_test_tasks(&[&panic_task, &chain_b, &chain_c], &ctx, &log);

    assert!(!ran_b.load(Ordering::SeqCst), "chain-b should not have run");
    assert!(!ran_c.load(Ordering::SeqCst), "chain-c should not have run");
    let entries = log.task_entries();
    assert!(
        entries
            .iter()
            .any(|e| e.name == "panic-task" && e.status == TaskStatus::Failed),
        "panicked task should be recorded as Failed"
    );
    assert!(
        entries
            .iter()
            .any(|e| e.name == "chain-b" && e.status == TaskStatus::Skipped),
        "chain-b should be recorded as Skipped"
    );
    assert!(
        entries
            .iter()
            .any(|e| e.name == "chain-c" && e.status == TaskStatus::Skipped),
        "chain-c should be recorded as Skipped"
    );
}

#[test]
fn multiple_independent_tasks_all_run() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let ran_1 = Arc::new(AtomicBool::new(false));
    let ran_2 = Arc::new(AtomicBool::new(false));
    let task_1 = FlagTask {
        ran: Arc::clone(&ran_1),
    };
    let task_2 = FlagTask2 {
        ran: Arc::clone(&ran_2),
    };

    run_test_tasks(&[&task_1, &task_2], &ctx, &log);

    assert!(
        ran_1.load(Ordering::SeqCst),
        "first independent task should have run"
    );
    assert!(
        ran_2.load(Ordering::SeqCst),
        "second independent task should have run"
    );
}

#[test]
fn task_runs_after_dependency_completes() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let ran_flag = Arc::new(AtomicBool::new(false));
    let ran_dep = Arc::new(AtomicBool::new(false));
    let flag_task = FlagTask {
        ran: Arc::clone(&ran_flag),
    };
    let dep_task = DepOnFlagTask {
        ran: Arc::clone(&ran_dep),
    };

    run_test_tasks(&[&flag_task, &dep_task], &ctx, &log);

    assert!(
        ran_flag.load(Ordering::SeqCst),
        "dependency (FlagTask) should have run"
    );
    assert!(
        ran_dep.load(Ordering::SeqCst),
        "dependent task should have run after its dependency"
    );
}

#[test]
fn diamond_dependency_all_tasks_run() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let ran_a = Arc::new(AtomicBool::new(false));
    let ran_b = Arc::new(AtomicBool::new(false));
    let ran_d = Arc::new(AtomicBool::new(false));
    let task_a = DiamondA {
        ran: Arc::clone(&ran_a),
    };
    let task_b = DiamondB {
        ran: Arc::clone(&ran_b),
    };
    let task_d = DiamondD {
        ran: Arc::clone(&ran_d),
    };

    run_test_tasks(&[&task_a, &task_b, &task_d], &ctx, &log);

    assert!(ran_a.load(Ordering::SeqCst), "diamond-a should have run");
    assert!(ran_b.load(Ordering::SeqCst), "diamond-b should have run");
    assert!(
        ran_d.load(Ordering::SeqCst),
        "diamond-d should have run after both parents completed"
    );
}

#[test]
fn empty_task_list_completes_without_panic() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let empty: Vec<&dyn Task> = vec![];
    run_test_tasks(&empty, &ctx, &log);
    // No panic = success
}

#[test]
fn dependency_not_in_list_is_ignored() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();
    let ran = Arc::new(AtomicBool::new(false));
    let task = DepOnMissing {
        ran: Arc::clone(&ran),
    };

    // PanicTask is not in the task list, so its dep is filtered out.
    // DepOnMissing should run as if it has no dependencies.
    run_test_tasks(&[&task], &ctx, &log);

    assert!(
        ran.load(Ordering::SeqCst),
        "task with missing dependency should run (dep filtered out)"
    );
}

#[test]
fn dependency_ordering_is_respected() {
    let (log, ctx, _dispatch_lock) = make_test_log_and_ctx();

    // Use the existing FlagTask → DepOnFlagTask relationship:
    // FlagTask must run before DepOnFlagTask. Verify using order of
    // task completion recorded in the logger.
    let flag_ran = Arc::new(AtomicBool::new(false));
    let dep_ran = Arc::new(AtomicBool::new(false));
    let flag_task = FlagTask {
        ran: Arc::clone(&flag_ran),
    };
    let dep_task = DepOnFlagTask {
        ran: Arc::clone(&dep_ran),
    };

    run_test_tasks(&[&dep_task, &flag_task], &ctx, &log);

    // Both must run.
    assert!(flag_ran.load(Ordering::SeqCst), "flag-task should have run");
    assert!(
        dep_ran.load(Ordering::SeqCst),
        "dep-on-flag should have run"
    );

    // dep-on-flag depends on FlagTask, so FlagTask must complete first.
    // The logger records tasks in completion order.
    let entries = log.task_entries();
    let flag_pos = entries.iter().position(|e| e.name == "flag-task");
    let dep_pos = entries.iter().position(|e| e.name == "dep-on-flag");
    assert!(
        flag_pos.is_some() && dep_pos.is_some(),
        "both tasks should be recorded in the logger"
    );
    assert!(
        flag_pos.unwrap() < dep_pos.unwrap(),
        "flag-task should complete before dep-on-flag (dependency ordering)"
    );
}
