//! Mode-independent readiness, outcomes, cancellation, and phase boundaries.

use super::*;

#[test]
fn claims_do_not_propagate_failures_and_are_released_after_errors_and_panics() {
    for behavior in [Behavior::Error, Behavior::PanicStr] {
        conformance("exclusive access is not a dependency", |mode, ctx, log| {
            let claim = AccessClaim::exclusive("fixture", "document");
            let mut failed = TestTask::new("failed").returning(behavior.clone());
            failed.claims = vec![claim.clone()];
            let mut independent = TestTask::new("independent");
            independent.claims = vec![claim.clone()];
            let summary = mode.run(&[&failed, &independent], ctx, log, None);
            failed.assert_ran(true);
            independent.assert_ran(true);
            independent.assert_record(log, &summary, TaskStatus::Ok, TaskOutcome::Satisfied, None);
            assert_eq!(summary.failure_count(), 1);
            assert!(ctx.try_access(&[claim]).is_some());
            summary
        });
    }
}

#[test]
fn claims_are_acquired_after_dependency_waits_and_forwarded_by_decorators() {
    conformance("shared claims across a real edge", |mode, ctx, log| {
        let claim = AccessClaim::exclusive("fixture", "document");
        let mut provider = TestTask::new("provider");
        provider.claims = vec![claim.clone()];
        let mut consumer = TestTask::new("consumer").after(&[&provider]);
        consumer.claims = vec![claim.clone()];
        let consumer = TaskWithExtraDeps::new(Box::new(consumer), &[], &[]);
        assert_eq!(consumer.access_claims(ctx), vec![claim.clone()]);
        let summary = mode.run(&[&consumer, &provider], ctx, log, None);
        assert_eq!(summary.failure_count(), 0);
        assert!(ctx.try_access(&[claim]).is_some());
        summary
    });
}

#[test]
fn independent_missing_and_empty_graphs() {
    conformance("empty", |mode, ctx, log| {
        let summary = mode.run(&[], ctx, log, None);
        assert_eq!(summary, ExecutionSummary::default());
        assert!(log.task_entries().is_empty());
        summary
    });
    for count in [1, 2] {
        conformance("independent and filtered dependencies", |mode, ctx, log| {
            let missing = TestTask::new("filtered-out");
            let first = TestTask::new("first").after(&[&missing]);
            let second = TestTask::new("second");
            let tasks: &[&dyn Task] = &[&first, &second];
            let summary = mode.run(&tasks[..count], ctx, log, None);
            first.assert_ran(true);
            second.assert_ran(count == 2);
            missing.assert_ran(false);
            assert_eq!(summary.failure_count(), 0);
            summary
        });
    }
}

#[test]
fn readiness_follows_edges_not_catalog_order() {
    conformance("diamond with reversed catalog", |mode, ctx, log| {
        let root = TestTask::new("root");
        let left = TestTask::new("left").after(&[&root]);
        let right = TestTask::new("right").after(&[&root]);
        let leaf = TestTask::new("leaf").after(&[&left, &right]);
        let summary = mode.run(&[&leaf, &right, &left, &root], ctx, log, None);
        for task in [&root, &left, &right, &leaf] {
            task.assert_ran(true);
            task.assert_record(log, &summary, TaskStatus::Ok, TaskOutcome::Satisfied, None);
        }
        assert_eq!(summary.failure_count(), 0);
        summary
    });
}

#[test]
fn failure_and_panic_block_transitively_without_stopping_independent_work() {
    let cases = [
        (
            "failed result",
            Behavior::Return(TaskResult::Failed("simulated failure".into())),
            "simulated failure",
        ),
        ("error", Behavior::Error, "simulated error"),
        (
            "borrowed panic",
            Behavior::PanicStr,
            "task panicked: simulated panic",
        ),
        (
            "owned panic",
            Behavior::PanicString,
            "task panicked: owned panic",
        ),
        ("opaque panic", Behavior::PanicOpaque, "task panicked"),
    ];
    for (name, behavior, message) in cases {
        conformance(name, |mode, ctx, log| {
            let root = TestTask::new("root").returning(behavior.clone());
            let child = TestTask::new("child").after(&[&root]);
            let leaf = TestTask::new("leaf").after(&[&child]);
            let independent = TestTask::new("independent");
            let summary = mode.run(&[&leaf, &child, &root, &independent], ctx, log, None);
            root.assert_ran(true);
            root.assert_record(
                log,
                &summary,
                TaskStatus::Failed,
                TaskOutcome::Failed,
                Some(message),
            );
            for task in [&child, &leaf] {
                task.assert_ran(false);
                task.assert_record(
                    log,
                    &summary,
                    TaskStatus::Blocked,
                    TaskOutcome::Blocked,
                    Some("blocked by failed dependency: root"),
                );
            }
            independent.assert_ran(true);
            independent.assert_record(log, &summary, TaskStatus::Ok, TaskOutcome::Satisfied, None);
            assert_eq!(summary.failure_count(), 1, "{name}: only the root failed");
            assert_eq!(log.failure_count(), 1);
            summary
        });
    }
}

#[test]
fn incomplete_dependencies_block_in_both_completion_policies() {
    for require_complete in [false, true] {
        conformance("incomplete dependency", |mode, ctx, log| {
            let ctx = ctx.with_require_complete(require_complete);
            let root = TestTask::new("incomplete").returning(Behavior::Return(TaskResult::unmet(
                "prerequisite unavailable",
            )));
            let child = TestTask::new("child").after(&[&root]);
            let summary = mode.run(&[&child, &root], &ctx, log, None);
            let status = if require_complete {
                TaskStatus::Failed
            } else {
                TaskStatus::Skipped
            };
            root.assert_record(
                log,
                &summary,
                status,
                TaskOutcome::Unmet,
                Some("prerequisite unavailable"),
            );
            child.assert_ran(false);
            child.assert_record(
                log,
                &summary,
                TaskStatus::Blocked,
                TaskOutcome::Blocked,
                Some("blocked by incomplete dependency: incomplete"),
            );
            assert_eq!(summary.failure_count(), usize::from(require_complete));
            summary
        });
    }
}

#[test]
fn harmless_results_and_inapplicability_satisfy_dependencies() {
    let cases = [
        ("ok", TaskResult::Ok, TaskStatus::Ok, None),
        (
            "skip",
            TaskResult::skipped("deliberate skip"),
            TaskStatus::Skipped,
            Some("deliberate skip"),
        ),
        (
            "not applicable",
            TaskResult::NotApplicable("no items".into()),
            TaskStatus::NotApplicable,
            Some("no items"),
        ),
        ("dry run", TaskResult::DryRun, TaskStatus::DryRun, None),
        ("check", TaskResult::CheckPassed, TaskStatus::Passed, None),
    ];
    for (name, result, status, message) in cases {
        conformance(name, |mode, ctx, log| {
            let root = TestTask::new("root").returning(Behavior::Return(result.clone()));
            let child = TestTask::new("child").after(&[&root]);
            let mut inapplicable = TestTask::new("inapplicable").returning(Behavior::PanicStr);
            inapplicable.applicable = false;
            let other = TestTask::new("other").after(&[&inapplicable]);
            let summary = mode.run(&[&child, &other, &root, &inapplicable], ctx, log, None);
            root.assert_record(log, &summary, status, TaskOutcome::Satisfied, message);
            inapplicable.assert_ran(false);
            inapplicable.assert_record(
                log,
                &summary,
                TaskStatus::NotApplicable,
                TaskOutcome::Satisfied,
                None,
            );
            for task in [&child, &other] {
                task.assert_ran(true);
                task.assert_record(log, &summary, TaskStatus::Ok, TaskOutcome::Satisfied, None);
            }
            assert_eq!(summary.failure_count(), 0);
            summary
        });
    }
}

#[test]
fn ordering_only_edges_wait_without_propagating_failure() {
    conformance("ordering-only chain and mixed edge", |mode, ctx, log| {
        let failed = TestTask::new("failed").returning(Behavior::Error);
        let blocked = TestTask::new("blocked").after(&[&failed]);
        let incomplete =
            TestTask::new("incomplete").returning(Behavior::Return(TaskResult::unmet("missing")));
        let ordered = TestTask::new("ordered").ordered_after(&[&failed, &blocked, &incomplete]);
        let mixed = TestTask::new("mixed")
            .after(&[&failed])
            .ordered_after(&[&failed]);
        let summary = mode.run(
            &[&mixed, &ordered, &blocked, &failed, &incomplete],
            ctx,
            log,
            None,
        );
        ordered.assert_ran(true);
        ordered.assert_record(log, &summary, TaskStatus::Ok, TaskOutcome::Satisfied, None);
        mixed.assert_ran(false);
        mixed.assert_record(
            log,
            &summary,
            TaskStatus::Blocked,
            TaskOutcome::Blocked,
            Some("blocked by failed dependency: failed"),
        );
        assert_eq!(summary.failure_count(), 1);
        summary
    });
}

#[test]
fn dynamic_identity_disambiguates_same_label_records_and_edges() {
    conformance("same display name", |mode, ctx, log| {
        let mut successful = TestTask::new("successful");
        let mut failed = TestTask::new("failed").returning(Behavior::Error);
        successful.name = "same-display-name";
        failed.name = "same-display-name";
        let allowed = TestTask::new("allowed").after(&[&successful]);
        let blocked = TestTask::new("blocked").after(&[&failed]);
        let summary = mode.run(&[&blocked, &allowed, &failed, &successful], ctx, log, None);
        successful.assert_record(log, &summary, TaskStatus::Ok, TaskOutcome::Satisfied, None);
        failed.assert_record(
            log,
            &summary,
            TaskStatus::Failed,
            TaskOutcome::Failed,
            Some("simulated error"),
        );
        allowed.assert_ran(true);
        blocked.assert_ran(false);
        blocked.assert_record(
            log,
            &summary,
            TaskStatus::Blocked,
            TaskOutcome::Blocked,
            Some("blocked by failed dependency: same-display-name"),
        );
        assert_eq!(summary.failure_count(), 1);
        summary
    });
}

#[test]
fn cancellation_before_dispatch_and_after_start_records_every_task() {
    for pre_cancelled in [false, true] {
        conformance("cooperative cancellation", |mode, ctx, log| {
            let root = TestTask::new("root").returning(Behavior::Cancel);
            let child = TestTask::new("child").after(&[&root]);
            let ordered = TestTask::new("ordered").ordered_after(&[&child]);
            if pre_cancelled {
                ctx.cancellation_token().cancel();
            }
            let summary = mode.run(&[&ordered, &child, &root], ctx, log, None);
            root.assert_ran(!pre_cancelled);
            if pre_cancelled {
                root.assert_record(
                    log,
                    &summary,
                    TaskStatus::Interrupted,
                    TaskOutcome::Cancelled,
                    Some("cancelled"),
                );
            } else {
                root.assert_record(log, &summary, TaskStatus::Ok, TaskOutcome::Satisfied, None);
            }
            for task in [&child, &ordered] {
                task.assert_ran(false);
                task.assert_record(
                    log,
                    &summary,
                    TaskStatus::Interrupted,
                    TaskOutcome::Cancelled,
                    Some("cancelled"),
                );
            }
            assert_eq!(summary.failure_count(), 0);
            summary
        });
    }
}

#[test]
fn interrupted_execution_cancels_dependents_without_a_global_request() {
    conformance("typed cancellation", |mode, ctx, log| {
        let root = TestTask::new("interrupted").returning(Behavior::Interrupted);
        let child = TestTask::new("child").after(&[&root]);
        let ordered = TestTask::new("ordered").ordered_after(&[&root]);
        let summary = mode.run(&[&child, &ordered, &root], ctx, log, None);
        root.assert_record(
            log,
            &summary,
            TaskStatus::Interrupted,
            TaskOutcome::Cancelled,
            Some("interrupted"),
        );
        for task in [&child, &ordered] {
            task.assert_ran(false);
            task.assert_record(
                log,
                &summary,
                TaskStatus::Interrupted,
                TaskOutcome::Cancelled,
                Some("cancelled"),
            );
        }
        assert!(
            !ctx.is_cancelled(),
            "typed interruption alone propagates over both edge kinds"
        );
        assert_eq!(summary.failure_count(), 0);
        summary
    });
}

#[test]
fn dependency_failure_precedes_cancellation_and_incomplete_results() {
    for reversed in [false, true] {
        conformance("deterministic blocking reason", |mode, ctx, log| {
            let failed = TestTask::new("failed").returning(Behavior::Error);
            let incomplete = TestTask::new("incomplete")
                .returning(Behavior::Return(TaskResult::unmet("missing")));
            let cancel = TestTask::new("cancel")
                .returning(Behavior::Cancel)
                .ordered_after(&[&failed, &incomplete]);
            let interrupted = TestTask::new("interrupted").after(&[&cancel]);
            let deps = if reversed {
                [&interrupted, &incomplete, &failed]
            } else {
                [&failed, &incomplete, &interrupted]
            };
            let child = TestTask::new("child").after(&deps);
            let summary = mode.run(
                &[&child, &interrupted, &cancel, &incomplete, &failed],
                ctx,
                log,
                None,
            );
            cancel.assert_ran(true);
            child.assert_ran(false);
            child.assert_record(
                log,
                &summary,
                TaskStatus::Blocked,
                TaskOutcome::Blocked,
                Some("blocked by failed dependency: failed"),
            );
            assert!(ctx.is_cancelled());
            assert_eq!(summary.failure_count(), 1);
            summary
        });
    }
}

#[test]
fn previous_phase_outcomes_preserve_blocking_and_ordering_semantics() {
    let cases = [
        (TaskOutcome::Satisfied, None),
        (
            TaskOutcome::Unmet,
            Some("blocked by incomplete dependency: earlier"),
        ),
        (
            TaskOutcome::Failed,
            Some("blocked by failed dependency: earlier"),
        ),
        (TaskOutcome::Blocked, Some("blocked by dependency: earlier")),
        (TaskOutcome::Cancelled, Some("cancelled")),
    ];
    for (outcome, reason) in cases {
        conformance("previous-phase result", |mode, ctx, log| {
            let earlier = TestTask::new("earlier");
            let mut prior = ExecutionSummary::default();
            prior.record(earlier.task_id(), earlier.name(), outcome);
            let failed_count = usize::from(outcome == TaskOutcome::Failed);
            prior.add_failures(failed_count);
            let child = TestTask::new("child").after(&[&earlier]);
            let ordered = TestTask::new("ordered").ordered_after(&[&earlier]);
            let mut summary = mode.run(&[&child, &ordered], ctx, log, Some(&prior));
            earlier.assert_ran(false);
            child.assert_ran(reason.is_none());
            let (status, expected) = match outcome {
                TaskOutcome::Satisfied => (TaskStatus::Ok, TaskOutcome::Satisfied),
                TaskOutcome::Cancelled => (TaskStatus::Interrupted, TaskOutcome::Cancelled),
                TaskOutcome::Unmet | TaskOutcome::Failed | TaskOutcome::Blocked => {
                    (TaskStatus::Blocked, TaskOutcome::Blocked)
                }
            };
            child.assert_record(log, &summary, status, expected, reason);
            let cancelled = outcome == TaskOutcome::Cancelled;
            ordered.assert_ran(!cancelled);
            ordered.assert_record(
                log,
                &summary,
                if cancelled {
                    TaskStatus::Interrupted
                } else {
                    TaskStatus::Ok
                },
                if cancelled {
                    TaskOutcome::Cancelled
                } else {
                    TaskOutcome::Satisfied
                },
                cancelled.then_some("cancelled"),
            );
            assert_eq!(
                summary.failure_count(),
                0,
                "prior failures must not be counted twice"
            );
            assert_eq!(summary.outcome(&earlier.task_id()), None);
            summary.merge(prior);
            assert_eq!(summary.failure_count(), failed_count);
            assert_eq!(summary.outcome(&earlier.task_id()), Some(outcome));
            summary
        });
    }
}

#[test]
fn mixed_phase_dependencies_keep_the_original_failure_and_edge_policy() {
    for (prior_name, current_name, expected_cause) in [
        ("z-prior", "a-current", "z-prior"),
        ("a-prior", "z-current", "z-current"),
    ] {
        conformance("mixed-phase failure precedence", |mode, ctx, log| {
            let earlier = TestTask::new(prior_name);
            let cancelled = TestTask::new("earlier-cancelled");
            let mut prior = ExecutionSummary::default();
            prior.record(earlier.task_id(), earlier.name(), TaskOutcome::Failed);
            prior.record(
                cancelled.task_id(),
                cancelled.name(),
                TaskOutcome::Cancelled,
            );
            prior.add_failures(1);
            let current = TestTask::new(current_name).returning(Behavior::Error);
            let middle = TestTask::new("middle").after(&[&current]);
            let leaf = TestTask::new("leaf")
                .after(&[&middle, &earlier])
                .ordered_after(&[&earlier, &cancelled]);
            let ordered = TestTask::new("ordered").ordered_after(&[&middle, &earlier]);

            let mut summary = mode.run(
                &[&leaf, &ordered, &middle, &current],
                ctx,
                log,
                Some(&prior),
            );

            middle.assert_ran(false);
            middle.assert_record(
                log,
                &summary,
                TaskStatus::Blocked,
                TaskOutcome::Blocked,
                Some(&format!("blocked by failed dependency: {current_name}")),
            );
            leaf.assert_ran(false);
            leaf.assert_record(
                log,
                &summary,
                TaskStatus::Blocked,
                TaskOutcome::Blocked,
                Some(&format!("blocked by failed dependency: {expected_cause}")),
            );
            ordered.assert_ran(true);
            assert_eq!(summary.failure_count(), 1);
            summary.merge(prior);
            assert_eq!(summary.failure_count(), 2);
            summary
        });
    }
}

#[test]
fn completed_phase_results_are_used_by_later_dispatch() {
    conformance("two actual execution phases", |mode, ctx, log| {
        let root = TestTask::new("root").returning(Behavior::Error);
        let child = TestTask::new("child").after(&[&root]);
        let ordered = TestTask::new("ordered").ordered_after(&[&root]);
        let mut first = mode.run(&[&root], ctx, log, None);
        let second = mode.run(&[&child, &ordered], ctx, log, Some(&first));
        child.assert_ran(false);
        child.assert_record(
            log,
            &second,
            TaskStatus::Blocked,
            TaskOutcome::Blocked,
            Some("blocked by failed dependency: root"),
        );
        ordered.assert_ran(true);
        first.merge(second);
        assert_eq!(first.failure_count(), 1);
        assert_eq!(first.outcome(&child.task_id()), Some(TaskOutcome::Blocked));
        first.add_failures(1);
        assert_eq!(
            first.failure_count(),
            2,
            "pre-dispatch failures survive merging"
        );
        first
    });
}

#[test]
fn satisfied_ordering_never_waives_blocking_dependencies() {
    conformance(
        "readiness does not waive failure blocking",
        |mode, ctx, log| {
            let provider = TestTask::new("provider").returning(Behavior::Error);
            let mut ordered = TestTask::new("ordered").ordered_after(&[&provider]);
            ordered.satisfied_ordering = vec![provider.task_id()];
            let mut blocking = TestTask::new("blocking").after(&[&provider]);
            blocking.satisfied_ordering = vec![provider.task_id()];
            let summary = mode.run(&[&ordered, &blocking, &provider], ctx, log, None);
            ordered.assert_ran(true);
            blocking.assert_ran(false);
            assert_eq!(summary.failure_count(), 1);
            summary
        },
    );
}

#[test]
fn conditional_ordering_applies_to_prior_phase_outcomes() {
    conformance("prior phase readiness", |mode, ctx, log| {
        let provider = TestTask::new("earlier-provider");
        let mut ready = TestTask::new("ready").ordered_after(&[&provider]);
        ready.satisfied_ordering = vec![provider.task_id()];
        let waiting = TestTask::new("waiting").ordered_after(&[&provider]);
        let mut prior = ExecutionSummary::default();
        prior.record(provider.task_id(), provider.name(), TaskOutcome::Cancelled);
        let summary = mode.run(&[&ready, &waiting], ctx, log, Some(&prior));
        ready.assert_ran(true);
        waiting.assert_ran(false);
        assert_eq!(
            summary.outcome(&waiting.task_id()),
            Some(TaskOutcome::Cancelled)
        );
        summary
    });
}
