use crate::engine::apply;
use crate::engine::mode::ProcessOpts;
use crate::phases::test_helpers::empty_config;
use crate::resources::{ResourceChange, ResourceState};
use std::path::PathBuf;

use super::{
    MockResource, TypedErrorResource, bail_opts, default_opts, dry_run_context, test_context,
};

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
fn process_single_invalid_increments_skipped() {
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

    assert_eq!(stats.skipped, 1);
    assert_eq!(stats.changed, 0);
}

#[test]
fn process_single_unknown_increments_skipped() {
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

    assert_eq!(stats.skipped, 1);
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
fn process_single_apply_skipped_no_bail_increments_skipped() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource =
        MockResource::new(ResourceState::Missing).with_apply(Ok(ResourceChange::Skipped {
            reason: "not supported".to_string(),
        }));
    let opts = default_opts();

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();

    assert_eq!(stats.skipped, 1);
    assert_eq!(stats.changed, 0);
}

#[test]
fn process_single_apply_error_no_bail_increments_skipped() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = MockResource::new(ResourceState::Missing).with_apply(Err("boom".to_string()));
    let opts = default_opts();

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();

    assert_eq!(stats.skipped, 1);
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
fn process_single_apply_bail_on_skipped_still_skips() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource =
        MockResource::new(ResourceState::Missing).with_apply(Ok(ResourceChange::Skipped {
            reason: "denied".to_string(),
        }));
    let opts = bail_opts();

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();
    assert_eq!(stats.skipped, 1);
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
    assert!(err.unwrap_err().to_string().contains("critical"));
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
    assert!(result.unwrap_err().to_string().contains("remove failed"));
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
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("permission denied")
    );
}

// -----------------------------------------------------------------------
// categorize_error — exercised via process_single with typed ResourceError
// -----------------------------------------------------------------------

#[test]
fn process_single_command_failed_error_lenient_skips() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = TypedErrorResource {
        error_variant: "command_failed",
    };
    let opts = default_opts();

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();
    assert_eq!(stats.skipped, 1);
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
    assert!(err.unwrap_err().to_string().contains("permission denied"));
}

#[test]
fn process_single_conflicting_state_error_lenient_skips() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = TypedErrorResource {
        error_variant: "conflicting_state",
    };
    let opts = default_opts();

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();
    assert_eq!(stats.skipped, 1);
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
    assert!(err.unwrap_err().to_string().contains("not supported"));
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
    assert!(err.unwrap_err().to_string().contains("exit code 1"));
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
fn process_single_permission_denied_error_lenient_skips() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = TypedErrorResource {
        error_variant: "permission_denied",
    };
    let opts = default_opts();

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();
    assert_eq!(stats.skipped, 1);
    assert_eq!(stats.changed, 0);
}

#[test]
fn process_single_not_supported_error_lenient_skips() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource = TypedErrorResource {
        error_variant: "not_supported",
    };
    let opts = default_opts();

    let stats = apply::process_single(&ctx, &resource, &ResourceState::Missing, &opts).unwrap();
    assert_eq!(stats.skipped, 1);
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
