//! Error-path coverage for the resource pipeline.
//!
//! Happy-path processing is well covered; what is easy to leave untested is
//! what happens when the *n*th operation in a batch fails. These tests use the
//! shared failure-injection doubles in `test_helpers::failure` to drive partial
//! failures through the real engine, including one executor-backed domain
//! resource so the propagation path from a failing process invocation up to the
//! batch counters is exercised end to end.

use std::sync::Arc;

use crate::domains::system::config::systemd_units::UnitScope;
use crate::domains::system::resources::systemd_unit::SystemdUnitResource;
use crate::engine::mode::ProcessOpts;
use crate::engine::resource::ResourceError;
use crate::engine::{
    IntrinsicState as _, RemovableResource as _, Resource as _, ResourceState, TaskResult,
    process_resources, process_resources_remove,
};
use crate::infra::exec::Executor;
use crate::test_helpers::{
    FailAt, FailingExecutor, FailingResource, ResourceErrorKind, empty_config,
};

use super::{bail_opts, test_context};

/// Extract the batch counters `(changed, already_ok, failed)` from a result.
fn stats(result: &TaskResult) -> (u32, u32, u32) {
    match result {
        TaskResult::Batch(stats) => Some((stats.changed, stats.already_ok, stats.failed)),
        TaskResult::Ok
        | TaskResult::DryRun
        | TaskResult::CheckPassed
        | TaskResult::NotApplicable(_)
        | TaskResult::Skipped(_)
        | TaskResult::Failed(_) => None,
    }
    .expect("resource processing should produce a batch result")
}

fn systemd_units(count: usize, executor: &Arc<dyn Executor>) -> Vec<SystemdUnitResource> {
    (0..count)
        .map(|idx| {
            SystemdUnitResource::new(
                format!("unit-{idx}.service"),
                UnitScope::User,
                Arc::clone(executor),
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Executor-driven failures
// ---------------------------------------------------------------------------

#[test]
fn a_failing_executor_call_aborts_the_batch_it_belongs_to() {
    let (ctx, _log) = test_context(empty_config("/dotfiles".into()));
    // `is-enabled` reports "disabled", so every unit needs applying; the third
    // systemctl invocation then fails.
    let failing = Arc::new(
        FailingExecutor::new(FailAt::Call(3))
            .only_program("systemctl")
            .with_stdout("disabled"),
    );
    let executor: Arc<dyn Executor> = Arc::<FailingExecutor>::clone(&failing);

    let err = process_resources(&ctx, systemd_units(4, &executor), &bail_opts())
        .expect_err("a failing state check must surface");
    assert!(
        err.to_string().contains("injected failure"),
        "unexpected error: {err}"
    );
    assert_eq!(
        failing.matched_calls(),
        3,
        "processing should stop at the failing invocation"
    );
}

#[test]
fn a_failing_executor_records_every_invocation_it_saw() {
    let failing = Arc::new(
        FailingExecutor::new(FailAt::Never)
            .only_program("systemctl")
            .with_stdout("disabled")
            .with_exit_success(false),
    );
    let executor: Arc<dyn Executor> = Arc::<FailingExecutor>::clone(&failing);

    for unit in &systemd_units(2, &executor) {
        assert_eq!(
            unit.current_state().expect("state check should succeed"),
            ResourceState::Missing,
            "a disabled unit should read as missing"
        );
        unit.apply().expect("apply should not error");
    }

    assert_eq!(
        failing.calls(),
        vec![
            "systemctl --user is-enabled unit-0.service".to_string(),
            "systemctl --user enable --now unit-0.service".to_string(),
            "systemctl --user is-enabled unit-1.service".to_string(),
            "systemctl --user enable --now unit-1.service".to_string(),
        ]
    );
    assert_eq!(failing.matched_calls(), 4);
}

#[test]
fn the_program_filter_scopes_both_counting_and_failure() {
    let failing = FailingExecutor::new(FailAt::Call(1)).only_program("pacman");

    failing
        .run("git", &["status"])
        .expect("unmatched programs never fail");
    let err = failing
        .run("pacman", &["-S", "vim"])
        .expect_err("the first pacman call must fail");
    assert!(err.to_string().contains("pacman call 1"), "{err}");

    assert_eq!(failing.calls().len(), 2, "every call is still recorded");
    assert_eq!(failing.matched_calls(), 1, "only pacman calls are counted");
}

#[test]
fn which_result_is_configurable_for_availability_gated_code() {
    let available = FailingExecutor::new(FailAt::Never);
    assert!(available.which("systemctl"));
    assert!(available.which_path("systemctl").is_ok());

    let missing = FailingExecutor::new(FailAt::Never).with_which(false);
    assert!(!missing.which("systemctl"));
    assert!(missing.which_path("systemctl").is_err());
}

// ---------------------------------------------------------------------------
// Resource-level failures
// ---------------------------------------------------------------------------

#[test]
fn a_lenient_batch_records_failures_and_keeps_going() {
    let (ctx, _log) = test_context(empty_config("/dotfiles".into()));
    let resources = vec![
        FailingResource::new("first", FailAt::Never),
        FailingResource::new("second", FailAt::Always),
        FailingResource::new("third", FailAt::Never),
    ];

    let result = process_resources(&ctx, resources, &ProcessOpts::lenient("install"))
        .expect("lenient mode should not abort");

    let (changed, _already_ok, failed) = stats(&result);
    assert_eq!(changed, 2, "resources after the failure must still apply");
    assert_eq!(failed, 1);
}

#[test]
fn a_strict_batch_stops_at_the_first_failure() {
    let (ctx, _log) = test_context(empty_config("/dotfiles".into()));
    let resources = vec![
        FailingResource::new("first", FailAt::Always),
        FailingResource::new("second", FailAt::Never),
    ];

    let err = process_resources(&ctx, resources, &bail_opts())
        .expect_err("strict mode must surface the failure");
    assert!(err.to_string().contains("first"), "unexpected error: {err}");
}

#[test]
fn injected_resource_errors_keep_their_typed_category() {
    let command_failed = FailingResource::new("pkg", FailAt::Always)
        .with_error(ResourceErrorKind::CommandFailed)
        .apply()
        .expect_err("apply should fail");
    assert_eq!(command_failed.category(), "command_failed");

    let permission_denied = FailingResource::new("/etc/secure", FailAt::Always)
        .with_error(ResourceErrorKind::PermissionDenied)
        .apply()
        .expect_err("apply should fail");
    assert_eq!(permission_denied.category(), "permission_denied");
    assert!(matches!(
        permission_denied,
        ResourceError::PermissionDenied { .. }
    ));

    let untyped = FailingResource::new("thing", FailAt::Always)
        .apply()
        .expect_err("apply should fail");
    assert_eq!(untyped.category(), "unknown");
}

#[test]
fn call_scoped_failures_only_trip_the_chosen_attempt() {
    let resource = FailingResource::new("retryable", FailAt::Call(2));

    assert!(resource.apply().is_ok(), "first attempt should succeed");
    assert!(resource.apply().is_err(), "second attempt should fail");
    assert!(resource.apply().is_ok(), "third attempt should succeed");
    assert_eq!(resource.apply_calls(), 3);
}

#[test]
fn remove_failures_are_injected_independently_of_apply() {
    let (ctx, _log) = test_context(empty_config("/dotfiles".into()));
    let resources = vec![
        FailingResource::new("keep", FailAt::Never).with_state(ResourceState::Correct),
        FailingResource::new("stuck", FailAt::Never)
            .with_state(ResourceState::Correct)
            .with_remove_failure(FailAt::Always),
    ];

    let err = process_resources_remove(&ctx, resources, "remove")
        .expect_err("removal propagates the failure to the caller");
    assert!(
        format!("{err:#}").contains("injected failure"),
        "unexpected error: {err:#}"
    );
}

#[test]
fn removal_skips_resources_that_are_not_in_the_desired_state() {
    let (ctx, _log) = test_context(empty_config("/dotfiles".into()));
    let resource = FailingResource::new("absent", FailAt::Never).with_state(ResourceState::Missing);

    let result =
        process_resources_remove(&ctx, vec![resource], "remove").expect("removal should succeed");

    let (changed, _already_ok, failed) = stats(&result);
    assert_eq!(changed, 0);
    assert_eq!(failed, 0);
}

#[test]
fn remove_call_counts_are_observable() {
    let resource = FailingResource::new("counted", FailAt::Never);
    assert_eq!(resource.remove_calls(), 0);
    resource.remove().expect("remove should succeed");
    resource.remove().expect("remove should succeed");
    assert_eq!(resource.remove_calls(), 2);
}
