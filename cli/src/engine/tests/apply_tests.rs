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

// -----------------------------------------------------------------------
// process_single
// -----------------------------------------------------------------------

#[test]
fn process_single_correct_increments_already_ok() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = MockResource::new(ResourceState::Correct);
    let opts = default_opts();

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Correct, &opts).unwrap();

    assert_eq!(stats.already_ok, 1);
    assert_eq!(stats.changed, 0);
    assert_eq!(stats.skipped, 0);
}

#[test]
fn process_single_invalid_increments_failed() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = MockResource::new(ResourceState::Invalid {
        reason: "test".to_string(),
    });
    let opts = default_opts();

    let stats = apply::process_single(
        &ctx,
        &resource,
        &ResourceState::Invalid {
            reason: "test".to_string(),
        },
        &opts,
    )
    .unwrap();

    assert_eq!(stats.failed, 1);
    assert_eq!(stats.skipped, 0);
    assert_eq!(stats.changed, 0);
}

#[test]
fn process_single_unknown_increments_failed() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = MockResource::new(ResourceState::Unknown {
        reason: "SHELL not set".to_string(),
    });
    let opts = default_opts();

    let stats = apply::process_single(
        &ctx,
        &resource,
        &ResourceState::Unknown {
            reason: "SHELL not set".to_string(),
        },
        &opts,
    )
    .unwrap();

    assert_eq!(stats.failed, 1);
    assert_eq!(stats.skipped, 0);
    assert_eq!(stats.changed, 0);
}

#[test]
fn process_single_missing_skips_when_fix_missing_false() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = MockResource::new(ResourceState::Missing);
    let opts = ProcessOpts::fix_existing("install");

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();

    assert_eq!(stats.skipped, 1);
    assert_eq!(stats.changed, 0);
}

#[test]
fn process_single_incorrect_skips_when_fix_incorrect_false() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = MockResource::new(ResourceState::Incorrect {
        current: "wrong".to_string(),
    });
    let opts = ProcessOpts::install_missing("install");

    let stats = apply::process_single(
        &ctx,
        &resource,
        &ResourceState::Incorrect {
            current: "wrong".to_string(),
        },
        &opts,
    )
    .unwrap();

    assert_eq!(stats.skipped, 1);
    assert_eq!(stats.changed, 0);
}

#[test]
fn process_single_missing_applies_and_increments_changed() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = MockResource::new(ResourceState::Missing);
    let opts = default_opts();

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();

    assert_eq!(stats.changed, 1);
    assert_eq!(stats.already_ok, 0);
}

#[test]
fn process_single_incorrect_applies_and_increments_changed() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = MockResource::new(ResourceState::Incorrect {
        current: "wrong".to_string(),
    });
    let opts = default_opts();

    let stats = apply::process_single(
        &ctx,
        &resource,
        &ResourceState::Incorrect {
            current: "wrong".to_string(),
        },
        &opts,
    )
    .unwrap();

    assert_eq!(stats.changed, 1);
}

#[test]
fn process_single_dry_run_missing_increments_changed_without_apply() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = dry_run_context(config);
    // Apply would error if called — but dry-run should skip it
    let resource =
        MockResource::new(ResourceState::Missing).with_apply(Err("should not call".into()));
    let opts = default_opts();

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();

    assert_eq!(stats.changed, 1);
}

#[test]
fn process_single_dry_run_incorrect_increments_changed() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = dry_run_context(config);
    let resource = MockResource::new(ResourceState::Incorrect {
        current: "old-value".to_string(),
    });
    let opts = default_opts();

    let stats = apply::process_single(
        &ctx,
        &resource,
        &ResourceState::Incorrect {
            current: "old-value".to_string(),
        },
        &opts,
    )
    .unwrap();

    assert_eq!(stats.changed, 1);
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

// -----------------------------------------------------------------------
// process_single (apply path)
// -----------------------------------------------------------------------

#[test]
fn process_single_apply_applied_increments_changed() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = MockResource::new(ResourceState::Missing);
    let opts = default_opts();

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();

    assert_eq!(stats.changed, 1);
}

#[test]
fn process_single_apply_already_correct_increments_already_ok() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource =
        MockResource::new(ResourceState::Missing).with_apply(Ok(ResourceChange::AlreadyCorrect));
    let opts = default_opts();

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();

    assert_eq!(stats.already_ok, 1);
    assert_eq!(stats.changed, 0);
}

#[test]
fn process_single_apply_skipped_no_bail_increments_failed() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = MockResource::new(ResourceState::Missing)
        .with_apply(Ok(ResourceChange::unusable("not supported")));
    let opts = default_opts();

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();

    assert_eq!(stats.failed, 1);
    assert_eq!(stats.skipped, 0);
    assert_eq!(stats.changed, 0);
}

#[test]
fn process_single_apply_benign_skip_increments_skipped() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = MockResource::new(ResourceState::Missing).with_apply(Ok(
        ResourceChange::skipped("not supported on this platform"),
    ));
    let opts = default_opts();

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();

    assert_eq!(stats.skipped, 1);
    assert_eq!(stats.failed, 0);
    assert_eq!(stats.changed, 0);
}

#[test]
fn process_single_apply_error_no_bail_increments_failed() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = MockResource::new(ResourceState::Missing).with_apply(Err("boom".to_string()));
    let opts = default_opts();

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();

    assert_eq!(stats.failed, 1);
    assert_eq!(stats.skipped, 0);
    assert_eq!(stats.changed, 0);
}

#[test]
fn process_single_apply_bail_on_applied() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = MockResource::new(ResourceState::Missing);
    let opts = bail_opts();

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();

    assert_eq!(stats.changed, 1);
}

#[test]
fn process_single_apply_bail_on_already_correct() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource =
        MockResource::new(ResourceState::Missing).with_apply(Ok(ResourceChange::AlreadyCorrect));
    let opts = bail_opts();

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();

    assert_eq!(stats.already_ok, 1);
}

#[test]
fn process_single_apply_bail_on_skipped_records_failure() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = MockResource::new(ResourceState::Missing)
        .with_apply(Ok(ResourceChange::unusable("denied")));
    let opts = bail_opts();

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.skipped, 0);
    assert_eq!(stats.changed, 0);
}

#[test]
fn process_single_apply_bail_on_error_propagates() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource =
        MockResource::new(ResourceState::Missing).with_apply(Err("critical".to_string()));
    let opts = bail_opts();

    let err = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts);
    assert!(err.is_err());
    let rendered = format!("{:#}", err.unwrap_err());
    assert!(rendered.contains("critical"), "got: {rendered}");
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

// -----------------------------------------------------------------------
// remove_single — direct unit tests
// -----------------------------------------------------------------------

#[test]
fn remove_single_correct_increments_changed() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = MockResource::new(ResourceState::Correct);
    let stats = apply::remove_single(&ctx, &resource, &ResourceState::Correct, "unlink").unwrap();
    assert_eq!(stats.changed, 1);
    assert_eq!(stats.already_ok, 0);
}

#[test]
fn remove_single_missing_increments_already_ok() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = MockResource::new(ResourceState::Missing);
    let stats = apply::remove_single(&ctx, &resource, &ResourceState::Missing, "unlink").unwrap();
    assert_eq!(stats.already_ok, 1);
    assert_eq!(stats.changed, 0);
}

#[test]
fn remove_single_incorrect_increments_already_ok() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = MockResource::new(ResourceState::Incorrect {
        current: "other".to_string(),
    });
    let stats = apply::remove_single(
        &ctx,
        &resource,
        &ResourceState::Incorrect {
            current: "other".to_string(),
        },
        "unlink",
    )
    .unwrap();
    assert_eq!(stats.already_ok, 1);
    assert_eq!(stats.changed, 0);
}

#[test]
fn remove_single_invalid_increments_already_ok() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = MockResource::new(ResourceState::Invalid {
        reason: "bad".to_string(),
    });
    let stats = apply::remove_single(
        &ctx,
        &resource,
        &ResourceState::Invalid {
            reason: "bad".to_string(),
        },
        "unlink",
    )
    .unwrap();
    assert_eq!(stats.already_ok, 1);
    assert_eq!(stats.changed, 0);
}

#[test]
fn remove_single_unknown_increments_skipped() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = MockResource::new(ResourceState::Unknown {
        reason: "detection failed".to_string(),
    });
    let stats = apply::remove_single(
        &ctx,
        &resource,
        &ResourceState::Unknown {
            reason: "detection failed".to_string(),
        },
        "unlink",
    )
    .unwrap();
    assert_eq!(stats.skipped, 1);
    assert_eq!(stats.changed, 0);
    assert_eq!(stats.already_ok, 0);
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

// -----------------------------------------------------------------------
// remove_single — typed error propagation
// -----------------------------------------------------------------------

#[test]
fn remove_single_typed_error_propagates() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource =
        MockResource::new(ResourceState::Correct).with_remove(Err("permission denied".into()));

    let result = apply::remove_single(&ctx, &resource, &ResourceState::Correct, "unlink");
    assert!(result.is_err());
    assert!(format!("{:#}", result.unwrap_err()).contains("permission denied"));
}

// -----------------------------------------------------------------------
// categorize_error — exercised via process_single with typed ResourceError
// -----------------------------------------------------------------------

#[test]
fn process_single_command_failed_error_lenient_fails_nonfatally() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = TypedErrorResource {
        error_variant: "command_failed",
    };
    let opts = default_opts();

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.skipped, 0);
    assert_eq!(stats.changed, 0);
}

#[test]
fn process_single_permission_denied_error_bail_propagates() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = TypedErrorResource {
        error_variant: "permission_denied",
    };
    let opts = bail_opts();

    let err = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts);
    assert!(err.is_err());
    assert!(format!("{:#}", err.unwrap_err()).contains("permission denied"));
}

#[test]
fn process_single_conflicting_state_error_lenient_fails_nonfatally() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = TypedErrorResource {
        error_variant: "conflicting_state",
    };
    let opts = default_opts();

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.skipped, 0);
}

#[test]
fn process_single_not_supported_error_bail_propagates() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = TypedErrorResource {
        error_variant: "not_supported",
    };
    let opts = bail_opts();

    let err = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts);
    assert!(err.is_err());
    assert!(format!("{:#}", err.unwrap_err()).contains("not supported"));
}

// -----------------------------------------------------------------------
// process_single — typed error variants with bail mode
// -----------------------------------------------------------------------

#[test]
fn process_single_command_failed_error_bail_propagates() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = TypedErrorResource {
        error_variant: "command_failed",
    };
    let opts = bail_opts();

    let err = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts);
    assert!(err.is_err());
    assert!(format!("{:#}", err.unwrap_err()).contains("exit code 1"));
}

#[test]
fn process_single_conflicting_state_error_bail_propagates() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = TypedErrorResource {
        error_variant: "conflicting_state",
    };
    let opts = bail_opts();

    let err = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts);
    assert!(err.is_err());
}

#[test]
fn process_single_permission_denied_error_lenient_fails_nonfatally() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = TypedErrorResource {
        error_variant: "permission_denied",
    };
    let opts = default_opts();

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.skipped, 0);
    assert_eq!(stats.changed, 0);
}

#[test]
fn process_single_not_supported_error_lenient_fails_nonfatally() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = TypedErrorResource {
        error_variant: "not_supported",
    };
    let opts = default_opts();

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.skipped, 0);
    assert_eq!(stats.changed, 0);
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
