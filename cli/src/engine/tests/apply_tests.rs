use crate::engine::apply;
use crate::engine::mode::ProcessOpts;
use crate::engine::{Resource, ResourceChange, ResourceResult, ResourceState};
use crate::infra::logging::{MsgKind, Output, TaskRecorder, TaskStatus};
use crate::test_helpers::empty_config;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::{
    MockResource, TypedErrorResource, bail_opts, default_opts, dry_run_context, test_context,
};

#[derive(Debug)]
struct OrderedEventLog {
    events: Arc<Mutex<Vec<String>>>,
}

impl Output for OrderedEventLog {
    fn emit(&self, kind: MsgKind, msg: std::borrow::Cow<'_, str>) {
        if kind == MsgKind::Warn {
            self.events.lock().unwrap().push(format!("warn: {msg}"));
        }
    }
}

impl TaskRecorder for OrderedEventLog {
    fn record_task(&self, _name: &str, _status: TaskStatus, _message: Option<&str>) {}
}

#[derive(Debug)]
struct DestructiveResource {
    events: Arc<Mutex<Vec<String>>>,
}

impl Resource for DestructiveResource {
    fn description(&self) -> String {
        "destructive resource".to_string()
    }

    fn pre_apply_warning(&self) -> ResourceResult<Option<String>> {
        Ok(Some("existing data will be replaced".to_string()))
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        self.events.lock().unwrap().push("apply".to_string());
        Ok(ResourceChange::Applied)
    }
}

fn counts(stats: &crate::engine::TaskStats) -> (u32, u32, u32, u32) {
    (
        stats.changed_count(),
        stats.already_ok_count(),
        stats.skipped_count(),
        stats.failed_count(),
    )
}

#[test]
fn process_single_classifies_resource_states() {
    let cases = [
        (
            "correct",
            ResourceState::Correct,
            default_opts(),
            (0, 1, 0, 0),
        ),
        (
            "invalid",
            ResourceState::Invalid {
                reason: "test".to_string(),
            },
            default_opts(),
            (0, 0, 0, 1),
        ),
        (
            "unknown",
            ResourceState::Unknown {
                reason: "SHELL not set".to_string(),
            },
            default_opts(),
            (0, 0, 0, 1),
        ),
        (
            "missing but only fixing existing",
            ResourceState::Missing,
            ProcessOpts::fix_existing("install"),
            (0, 0, 1, 0),
        ),
        (
            "incorrect but only installing missing",
            ResourceState::Incorrect {
                current: "wrong".to_string(),
            },
            ProcessOpts::install_missing("install"),
            (0, 0, 1, 0),
        ),
        (
            "missing",
            ResourceState::Missing,
            default_opts(),
            (1, 0, 0, 0),
        ),
        (
            "incorrect",
            ResourceState::Incorrect {
                current: "wrong".to_string(),
            },
            default_opts(),
            (1, 0, 0, 0),
        ),
    ];

    for (case, state, opts, expected) in cases {
        let (ctx, _) = test_context(empty_config(PathBuf::from("/tmp")));
        let resource = MockResource::new(state.clone());
        let stats = apply::process_single(&ctx, &resource, &state, &opts).unwrap();
        assert_eq!(counts(&stats), expected, "{case}");
    }
}

#[test]
fn process_single_dry_run_never_applies() {
    for state in [
        ResourceState::Missing,
        ResourceState::Incorrect {
            current: "old-value".to_string(),
        },
    ] {
        let (ctx, _) = dry_run_context(empty_config(PathBuf::from("/tmp")));
        let resource =
            MockResource::new(state.clone()).with_apply(Err("should not call".to_string()));
        let stats = apply::process_single(&ctx, &resource, &state, &default_opts()).unwrap();
        assert_eq!(counts(&stats), (1, 0, 0, 0), "{state:?}");
    }
}

#[test]
fn process_single_warns_before_destructive_apply() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let events = Arc::new(Mutex::new(Vec::new()));
    let ctx = ctx.with_log(Arc::new(OrderedEventLog {
        events: Arc::clone(&events),
    }));
    let resource = DestructiveResource {
        events: Arc::clone(&events),
    };

    let stats = apply::process_single(
        &ctx,
        &resource,
        &ResourceState::Incorrect {
            current: "user data".to_string(),
        },
        &default_opts(),
    )
    .unwrap();

    assert_eq!(stats.changed, 1);
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["warn: existing data will be replaced", "apply",]
    );
}

#[test]
fn process_single_dry_run_neither_warns_nor_applies() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = dry_run_context(config);
    let events = Arc::new(Mutex::new(Vec::new()));
    let ctx = ctx.with_log(Arc::new(OrderedEventLog {
        events: Arc::clone(&events),
    }));
    let resource = DestructiveResource {
        events: Arc::clone(&events),
    };

    let stats = apply::process_single(
        &ctx,
        &resource,
        &ResourceState::Incorrect {
            current: "user data".to_string(),
        },
        &default_opts(),
    )
    .unwrap();

    assert_eq!(stats.changed, 1);
    assert!(
        events.lock().unwrap().is_empty(),
        "dry-run must not enter the mutation boundary"
    );
}

#[test]
fn process_single_classifies_apply_outcomes_in_each_mode() {
    let cases = [
        (
            "applied",
            Ok(ResourceChange::Applied),
            default_opts(),
            (1, 0, 0, 0),
        ),
        (
            "already correct",
            Ok(ResourceChange::AlreadyCorrect),
            default_opts(),
            (0, 1, 0, 0),
        ),
        (
            "unusable",
            Ok(ResourceChange::unusable("not supported")),
            default_opts(),
            (0, 0, 0, 1),
        ),
        (
            "benign skip",
            Ok(ResourceChange::skipped("not supported on this platform")),
            default_opts(),
            (0, 0, 1, 0),
        ),
        (
            "lenient error",
            Err("boom".to_string()),
            default_opts(),
            (0, 0, 0, 1),
        ),
        (
            "strict applied",
            Ok(ResourceChange::Applied),
            bail_opts(),
            (1, 0, 0, 0),
        ),
        (
            "strict already correct",
            Ok(ResourceChange::AlreadyCorrect),
            bail_opts(),
            (0, 1, 0, 0),
        ),
        (
            "strict unusable",
            Ok(ResourceChange::unusable("denied")),
            bail_opts(),
            (0, 0, 0, 1),
        ),
    ];

    for (case, outcome, opts, expected) in cases {
        let (ctx, _) = test_context(empty_config(PathBuf::from("/tmp")));
        let resource = MockResource::new(ResourceState::Missing).with_apply(outcome);
        let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();
        assert_eq!(counts(&stats), expected, "{case}");
    }
}

#[test]
fn process_single_strict_apply_error_propagates() {
    let (ctx, _) = test_context(empty_config(PathBuf::from("/tmp")));
    let resource =
        MockResource::new(ResourceState::Missing).with_apply(Err("critical".to_string()));
    let error = apply::process_single(&ctx, &resource, &ResourceState::Missing, &bail_opts())
        .expect_err("strict processing must propagate apply failures");
    assert!(format!("{error:#}").contains("critical"));
}

#[test]
fn process_single_apply_error_names_the_failing_resource() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = MockResource::new(ResourceState::Missing)
        .with_desc("~/.bashrc")
        .with_apply(Err("critical".to_string()));
    let opts = bail_opts();

    let err = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts)
        .expect_err("bail_on_error must propagate the failure");

    let rendered = format!("{err:#}");
    assert!(
        rendered.contains("~/.bashrc"),
        "propagated error must identify the resource, got: {rendered}"
    );
    assert!(rendered.contains("critical"), "got: {rendered}");
}

#[test]
fn process_single_apply_error_preserves_typed_category() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = TypedErrorResource {
        error_variant: "command_failed",
    };
    let opts = bail_opts();

    let err = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts)
        .expect_err("bail_on_error must propagate the failure");

    let category = err
        .downcast_ref::<crate::engine::resource::ResourceError>()
        .map(crate::engine::resource::ResourceError::category);
    assert_eq!(
        category,
        Some("command_failed"),
        "context must not erase the typed error category"
    );
}

#[test]
fn remove_single_classifies_resource_states() {
    let cases = [
        ("correct", ResourceState::Correct, (1, 0, 0, 0)),
        ("missing", ResourceState::Missing, (0, 1, 0, 0)),
        (
            "incorrect",
            ResourceState::Incorrect {
                current: "other".to_string(),
            },
            (0, 1, 0, 0),
        ),
        (
            "invalid",
            ResourceState::Invalid {
                reason: "bad".to_string(),
            },
            (0, 1, 0, 0),
        ),
        (
            "unknown",
            ResourceState::Unknown {
                reason: "detection failed".to_string(),
            },
            (0, 0, 1, 0),
        ),
    ];

    for (case, state, expected) in cases {
        let (ctx, _) = test_context(empty_config(PathBuf::from("/tmp")));
        let resource = MockResource::new(state.clone());
        let stats = apply::remove_single(&ctx, &resource, &state, "unlink").unwrap();
        assert_eq!(counts(&stats), expected, "{case}");
    }
}

#[test]
fn remove_single_dry_run_does_not_call_remove() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = dry_run_context(config);
    // remove() would error if called, but dry-run skips it
    let resource =
        MockResource::new(ResourceState::Correct).with_remove(Err("should not call".into()));
    let stats = apply::remove_single(&ctx, &resource, &ResourceState::Correct, "unlink").unwrap();
    assert_eq!(stats.changed, 1);
}

#[test]
fn remove_single_error_propagates() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource =
        MockResource::new(ResourceState::Correct).with_remove(Err("remove failed".into()));
    let result = apply::remove_single(&ctx, &resource, &ResourceState::Correct, "unlink");
    assert!(result.is_err());
    assert!(format!("{:#}", result.unwrap_err()).contains("remove failed"));
}

#[test]
fn typed_resource_errors_are_nonfatal_in_lenient_mode() {
    for variant in [
        "command_failed",
        "permission_denied",
        "conflicting_state",
        "not_supported",
    ] {
        let (ctx, _) = test_context(empty_config(PathBuf::from("/tmp")));
        let resource = TypedErrorResource {
            error_variant: variant,
        };
        let stats =
            apply::process_single(&ctx, &resource, &ResourceState::Missing, &default_opts())
                .unwrap();
        assert_eq!(counts(&stats), (0, 0, 0, 1), "{variant}");
    }
}

#[test]
fn typed_resource_errors_propagate_in_strict_mode() {
    let cases = [
        ("command_failed", "exit code 1"),
        ("permission_denied", "permission denied"),
        ("conflicting_state", "conflicting"),
        ("not_supported", "not supported"),
    ];

    for (variant, expected) in cases {
        let (ctx, _) = test_context(empty_config(PathBuf::from("/tmp")));
        let resource = TypedErrorResource {
            error_variant: variant,
        };
        let error = apply::process_single(&ctx, &resource, &ResourceState::Missing, &bail_opts())
            .expect_err("strict processing must propagate typed resource errors");
        assert!(
            format!("{error:#}").contains(expected),
            "{variant}: {error:#}"
        );
    }
}

#[test]
fn cancellation_propagates_in_lenient_mode() {
    let (ctx, _) = test_context(empty_config(PathBuf::from("/tmp")));
    let resource = TypedErrorResource {
        error_variant: "cancelled",
    };
    let error = apply::process_single(&ctx, &resource, &ResourceState::Missing, &default_opts())
        .expect_err("cancellation must propagate through lenient resource processing");

    assert!(error.chain().any(|cause| {
        cause
            .downcast_ref::<crate::infra::exec::ExecError>()
            .is_some_and(crate::infra::exec::ExecError::is_cancelled)
            || cause
                .downcast_ref::<crate::engine::resource::ResourceError>()
                .is_some_and(crate::engine::resource::ResourceError::is_cancelled)
    }));
}

// -----------------------------------------------------------------------
// Resource description propagation
// -----------------------------------------------------------------------

#[test]
fn process_single_uses_resource_description() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = MockResource::new(ResourceState::Missing).with_desc("custom desc");
    let opts = default_opts();

    // Should succeed — verifies description doesn't interfere with processing
    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();
    assert_eq!(stats.changed, 1);
}
