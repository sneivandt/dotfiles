use crate::engine::apply;
use crate::engine::mode::ProcessOpts;
use crate::engine::{
    TaskResult, TaskStats, process_resource_states, process_resources, process_resources_remove,
};
use crate::error::ResourceError;
use crate::resources::{Applicable, Resource, ResourceChange, ResourceState};
use crate::tasks::test_helpers::{empty_config, make_static_context};
use std::path::PathBuf;

// -----------------------------------------------------------------------
// Test doubles
// -----------------------------------------------------------------------

/// A configurable mock resource for testing the processing pipeline.
struct MockResource {
    state_result: Result<ResourceState, String>,
    apply_result: Result<ResourceChange, String>,
    remove_result: Result<ResourceChange, String>,
    desc: String,
}

impl MockResource {
    fn new(state: ResourceState) -> Self {
        Self {
            state_result: Ok(state),
            apply_result: Ok(ResourceChange::Applied),
            remove_result: Ok(ResourceChange::Applied),
            desc: "mock resource".to_string(),
        }
    }

    fn with_desc(mut self, desc: impl Into<String>) -> Self {
        self.desc = desc.into();
        self
    }

    fn with_state_error(mut self, err: impl Into<String>) -> Self {
        self.state_result = Err(err.into());
        self
    }

    fn with_apply(mut self, result: Result<ResourceChange, String>) -> Self {
        self.apply_result = result;
        self
    }

    fn with_remove(mut self, result: Result<ResourceChange, String>) -> Self {
        self.remove_result = result;
        self
    }
}

impl Applicable for MockResource {
    fn description(&self) -> String {
        self.desc.clone()
    }

    fn apply(&self) -> anyhow::Result<ResourceChange> {
        self.apply_result
            .clone()
            .map_err(|s| anyhow::anyhow!("{s}"))
    }

    fn remove(&self) -> anyhow::Result<ResourceChange> {
        self.remove_result
            .clone()
            .map_err(|s| anyhow::anyhow!("{s}"))
    }
}

impl Resource for MockResource {
    fn current_state(&self) -> anyhow::Result<ResourceState> {
        self.state_result
            .clone()
            .map_err(|s| anyhow::anyhow!("{s}"))
    }
}

/// A mock resource that returns a typed [`ResourceError`] from `apply()`.
struct TypedErrorResource {
    error_variant: &'static str,
}

impl Applicable for TypedErrorResource {
    fn description(&self) -> String {
        "typed-error resource".to_string()
    }

    fn apply(&self) -> anyhow::Result<ResourceChange> {
        match self.error_variant {
            "command_failed" => Err(ResourceError::CommandFailed {
                program: "pacman".into(),
                message: "exit code 1".into(),
            }
            .into()),
            "permission_denied" => Err(ResourceError::PermissionDenied {
                path: "/etc/secure".into(),
            }
            .into()),
            "conflicting_state" => Err(ResourceError::ConflictingState {
                resource: "test".into(),
                expected: "a".into(),
                actual: "b".into(),
            }
            .into()),
            "not_supported" => Err(ResourceError::NotSupported {
                reason: "linux only".into(),
            }
            .into()),
            other => Err(anyhow::anyhow!("unknown error variant: {other}")),
        }
    }

    fn remove(&self) -> anyhow::Result<ResourceChange> {
        Ok(ResourceChange::Applied)
    }
}

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

fn test_context(
    config: crate::config::Config,
) -> (
    crate::tasks::Context,
    std::sync::Arc<crate::logging::Logger>,
) {
    make_static_context(config)
}

fn dry_run_context(
    config: crate::config::Config,
) -> (
    crate::tasks::Context,
    std::sync::Arc<crate::logging::Logger>,
) {
    let (mut ctx, log) = test_context(config);
    ctx = ctx.with_dry_run(true);
    (ctx, log)
}

fn parallel_context(
    config: crate::config::Config,
) -> (
    crate::tasks::Context,
    std::sync::Arc<crate::logging::Logger>,
) {
    let (mut ctx, log) = test_context(config);
    ctx = ctx.with_parallel(true);
    (ctx, log)
}

fn default_opts() -> ProcessOpts<'static> {
    ProcessOpts::lenient("install")
}

fn bail_opts() -> ProcessOpts<'static> {
    ProcessOpts::strict("install")
}

// -----------------------------------------------------------------------
// TaskStats
// -----------------------------------------------------------------------

#[test]
fn stats_summary_changed_only() {
    let stats = TaskStats {
        changed: 3,
        already_ok: 0,
        skipped: 0,
    };
    assert_eq!(stats.summary(false), "3 changed, 0 already ok");
}

#[test]
fn stats_summary_dry_run() {
    let stats = TaskStats {
        changed: 2,
        already_ok: 5,
        skipped: 0,
    };
    assert_eq!(stats.summary(true), "2 would change, 5 already ok");
}

#[test]
fn stats_summary_with_skipped() {
    let stats = TaskStats {
        changed: 1,
        already_ok: 2,
        skipped: 3,
    };
    assert_eq!(stats.summary(false), "1 changed, 2 already ok, 3 skipped");
}

#[test]
fn stats_finish_returns_dry_run_result() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = dry_run_context(config);
    let stats = TaskStats::new();
    let result = stats.finish(&ctx);
    assert!(matches!(result, TaskResult::DryRun));
}

#[test]
fn stats_finish_returns_ok_result() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let stats = TaskStats::new();
    let result = stats.finish(&ctx);
    assert!(matches!(result, TaskResult::Ok));
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
// process_resources
// -----------------------------------------------------------------------

#[test]
fn process_resources_mixed_states() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resources = vec![
        MockResource::new(ResourceState::Correct),
        MockResource::new(ResourceState::Missing),
        MockResource::new(ResourceState::Invalid {
            reason: "bad".to_string(),
        }),
    ];
    let opts = default_opts();

    let result = process_resources(&ctx, resources, &opts).unwrap();
    assert!(matches!(result, TaskResult::Ok));
}

#[test]
fn process_resources_empty_list() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resources: Vec<MockResource> = vec![];
    let opts = default_opts();

    let result = process_resources(&ctx, resources, &opts).unwrap();
    assert!(matches!(result, TaskResult::Ok));
}

// -----------------------------------------------------------------------
// process_resource_states
// -----------------------------------------------------------------------

#[test]
fn process_resource_states_applies_precomputed() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource_states = vec![
        (
            MockResource::new(ResourceState::Missing),
            ResourceState::Missing,
        ),
        (
            MockResource::new(ResourceState::Correct),
            ResourceState::Correct,
        ),
    ];
    let opts = default_opts();

    let result = process_resource_states(&ctx, resource_states, &opts).unwrap();
    assert!(matches!(result, TaskResult::Ok));
}

// -----------------------------------------------------------------------
// process_resources_remove
// -----------------------------------------------------------------------

#[test]
fn process_resources_remove_removes_correct_resources() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resources = vec![
        MockResource::new(ResourceState::Correct),
        MockResource::new(ResourceState::Missing),
    ];

    let result = process_resources_remove(&ctx, resources, "unlink").unwrap();
    assert!(matches!(result, TaskResult::Ok));
}

#[test]
fn process_resources_remove_dry_run() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = dry_run_context(config);
    // Remove should NOT be called in dry-run
    let resources =
        vec![MockResource::new(ResourceState::Correct).with_remove(Err("should not call".into()))];

    let result = process_resources_remove(&ctx, resources, "unlink").unwrap();
    assert!(matches!(result, TaskResult::DryRun));
}

// -----------------------------------------------------------------------
// Parallel dispatch — process_resources
// -----------------------------------------------------------------------

#[test]
fn process_resources_parallel_accumulates_stats() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = parallel_context(config);
    // Three resources: one already correct, one missing (will be applied), one invalid (skipped).
    let resources = vec![
        MockResource::new(ResourceState::Correct),
        MockResource::new(ResourceState::Missing),
        MockResource::new(ResourceState::Invalid {
            reason: "bad".to_string(),
        }),
    ];
    let opts = default_opts();

    let result = process_resources(&ctx, resources, &opts).unwrap();
    assert!(matches!(result, TaskResult::Ok));
}

#[test]
fn process_resources_parallel_single_resource_runs_sequentially() {
    // When there is only one resource, the sequential path is taken even if parallel=true.
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = parallel_context(config);
    let resources = vec![MockResource::new(ResourceState::Missing)];
    let opts = default_opts();

    let result = process_resources(&ctx, resources, &opts).unwrap();
    assert!(matches!(result, TaskResult::Ok));
}

#[test]
fn process_resources_parallel_bail_on_error_propagates() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = parallel_context(config);
    let resources = vec![
        MockResource::new(ResourceState::Missing).with_apply(Err("fatal".into())),
        MockResource::new(ResourceState::Missing).with_apply(Err("fatal".into())),
    ];
    let opts = bail_opts();

    let result = process_resources(&ctx, resources, &opts);
    assert!(result.is_err());
}

// -----------------------------------------------------------------------
// Parallel dispatch — process_resource_states
// -----------------------------------------------------------------------

#[test]
fn process_resource_states_parallel_dispatch() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = parallel_context(config);
    let resource_states = vec![
        (
            MockResource::new(ResourceState::Missing),
            ResourceState::Missing,
        ),
        (
            MockResource::new(ResourceState::Correct),
            ResourceState::Correct,
        ),
        (
            MockResource::new(ResourceState::Incorrect {
                current: "old".to_string(),
            }),
            ResourceState::Incorrect {
                current: "old".to_string(),
            },
        ),
    ];
    let opts = default_opts();

    let result = process_resource_states(&ctx, resource_states, &opts).unwrap();
    assert!(matches!(result, TaskResult::Ok));
}

// -----------------------------------------------------------------------
// Parallel dispatch — process_resources_remove
// -----------------------------------------------------------------------

#[test]
fn process_resources_remove_parallel_dispatch() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = parallel_context(config);
    let resources = vec![
        MockResource::new(ResourceState::Correct),
        MockResource::new(ResourceState::Missing),
    ];

    let result = process_resources_remove(&ctx, resources, "unlink").unwrap();
    assert!(matches!(result, TaskResult::Ok));
}

// -----------------------------------------------------------------------
// Error propagation — current_state() failures
// -----------------------------------------------------------------------

#[test]
fn process_resources_current_state_error_propagates() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resources =
        vec![MockResource::new(ResourceState::Missing).with_state_error("state failed")];
    let opts = default_opts();

    let result = process_resources(&ctx, resources, &opts);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("state failed"));
}

#[test]
fn process_resources_remove_current_state_error_propagates() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resources =
        vec![MockResource::new(ResourceState::Missing).with_state_error("state failed")];

    let result = process_resources_remove(&ctx, resources, "unlink");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("state failed"));
}

// -----------------------------------------------------------------------
// End-to-end bail-on-error through process_resources (sequential)
// -----------------------------------------------------------------------

#[test]
fn process_resources_bail_on_apply_error_propagates() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resources = vec![MockResource::new(ResourceState::Missing).with_apply(Err("fatal".into()))];
    let opts = bail_opts();

    let result = process_resources(&ctx, resources, &opts);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("fatal"));
}

// -----------------------------------------------------------------------
// Stats accumulation across multiple resources in process_resource_states
// -----------------------------------------------------------------------

#[test]
fn process_resource_states_stats_accumulate_across_resources() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    // 2 correct, 1 missing (applied), 1 invalid (skipped)
    let resource_states = vec![
        (
            MockResource::new(ResourceState::Correct),
            ResourceState::Correct,
        ),
        (
            MockResource::new(ResourceState::Correct),
            ResourceState::Correct,
        ),
        (
            MockResource::new(ResourceState::Missing),
            ResourceState::Missing,
        ),
        (
            MockResource::new(ResourceState::Invalid {
                reason: "bad".to_string(),
            }),
            ResourceState::Invalid {
                reason: "bad".to_string(),
            },
        ),
    ];
    let opts = default_opts();

    // Just verify it succeeds — individual counts are exercised by process_single tests
    let result = process_resource_states(&ctx, resource_states, &opts).unwrap();
    assert!(matches!(result, TaskResult::Ok));
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
// Parallel — dry-run behaviour
// -----------------------------------------------------------------------

#[test]
fn process_resources_parallel_dry_run() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (mut ctx, _log) = parallel_context(config);
    ctx = ctx.with_dry_run(true);
    // apply() would error if called — dry-run must skip it
    let resources = vec![
        MockResource::new(ResourceState::Missing).with_apply(Err("no apply".into())),
        MockResource::new(ResourceState::Missing).with_apply(Err("no apply".into())),
    ];
    let opts = default_opts();
    let result = process_resources(&ctx, resources, &opts).unwrap();
    assert!(matches!(result, TaskResult::DryRun));
}

#[test]
fn process_resources_remove_parallel_dry_run() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (mut ctx, _log) = parallel_context(config);
    ctx = ctx.with_dry_run(true);
    let resources = vec![
        MockResource::new(ResourceState::Correct).with_remove(Err("no remove".into())),
        MockResource::new(ResourceState::Correct).with_remove(Err("no remove".into())),
    ];
    let result = process_resources_remove(&ctx, resources, "unlink").unwrap();
    assert!(matches!(result, TaskResult::DryRun));
}

#[test]
fn process_resource_states_parallel_no_bail_skips_errors() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = parallel_context(config);
    let resource_states = vec![
        (
            MockResource::new(ResourceState::Missing).with_apply(Err("oops".into())),
            ResourceState::Missing,
        ),
        (
            MockResource::new(ResourceState::Correct),
            ResourceState::Correct,
        ),
    ];
    let opts = default_opts(); // no_bail
    let result = process_resource_states(&ctx, resource_states, &opts).unwrap();
    assert!(matches!(result, TaskResult::Ok));
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
// TaskStats AddAssign
// -----------------------------------------------------------------------

#[test]
fn stats_add_assign_accumulates() {
    let mut a = TaskStats {
        changed: 1,
        already_ok: 2,
        skipped: 3,
    };
    let b = TaskStats {
        changed: 10,
        already_ok: 20,
        skipped: 30,
    };
    a += b;
    assert_eq!(a.changed, 11);
    assert_eq!(a.already_ok, 22);
    assert_eq!(a.skipped, 33);
}

// -----------------------------------------------------------------------
// process_resource_states — empty list
// -----------------------------------------------------------------------

#[test]
fn process_resource_states_empty_list() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource_states: Vec<(MockResource, ResourceState)> = vec![];
    let opts = default_opts();

    let result = process_resource_states(&ctx, resource_states, &opts).unwrap();
    assert!(matches!(result, TaskResult::Ok));
}

// -----------------------------------------------------------------------
// process_resources_remove — empty list
// -----------------------------------------------------------------------

#[test]
fn process_resources_remove_empty_list() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resources: Vec<MockResource> = vec![];

    let result = process_resources_remove(&ctx, resources, "unlink").unwrap();
    assert!(matches!(result, TaskResult::Ok));
}

// -----------------------------------------------------------------------
// process_resource_states — bail on error
// -----------------------------------------------------------------------

#[test]
fn process_resource_states_bail_on_error_propagates() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource_states = vec![(
        MockResource::new(ResourceState::Missing).with_apply(Err("fatal".into())),
        ResourceState::Missing,
    )];
    let opts = bail_opts();

    let result = process_resource_states(&ctx, resource_states, &opts);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("fatal"));
}

// -----------------------------------------------------------------------
// process_resource_states — lenient error skipping
// -----------------------------------------------------------------------

#[test]
fn process_resource_states_lenient_skips_errors() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resource_states = vec![
        (
            MockResource::new(ResourceState::Missing).with_apply(Err("oops".into())),
            ResourceState::Missing,
        ),
        (
            MockResource::new(ResourceState::Correct),
            ResourceState::Correct,
        ),
    ];
    let opts = default_opts();

    let result = process_resource_states(&ctx, resource_states, &opts).unwrap();
    assert!(matches!(result, TaskResult::Ok));
}

// -----------------------------------------------------------------------
// process_resources — lenient error skipping at orchestration level
// -----------------------------------------------------------------------

#[test]
fn process_resources_lenient_skips_apply_errors() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resources = vec![
        MockResource::new(ResourceState::Missing).with_apply(Err("oops".into())),
        MockResource::new(ResourceState::Correct),
    ];
    let opts = default_opts();

    let result = process_resources(&ctx, resources, &opts).unwrap();
    assert!(matches!(result, TaskResult::Ok));
}

// -----------------------------------------------------------------------
// Parallel — process_resource_states bail on error
// -----------------------------------------------------------------------

#[test]
fn process_resource_states_parallel_bail_on_error_propagates() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = parallel_context(config);
    let resource_states = vec![
        (
            MockResource::new(ResourceState::Missing).with_apply(Err("fatal".into())),
            ResourceState::Missing,
        ),
        (
            MockResource::new(ResourceState::Missing).with_apply(Err("fatal".into())),
            ResourceState::Missing,
        ),
    ];
    let opts = bail_opts();

    let result = process_resource_states(&ctx, resource_states, &opts);
    assert!(result.is_err());
}

// -----------------------------------------------------------------------
// Parallel — process_resources_remove with state error
// -----------------------------------------------------------------------

#[test]
fn process_resources_remove_parallel_state_error_propagates() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = parallel_context(config);
    let resources = vec![
        MockResource::new(ResourceState::Correct).with_state_error("state failed"),
        MockResource::new(ResourceState::Correct).with_state_error("state failed"),
    ];

    let result = process_resources_remove(&ctx, resources, "unlink");
    assert!(result.is_err());
}

// -----------------------------------------------------------------------
// Parallel — process_resources with state error
// -----------------------------------------------------------------------

#[test]
fn process_resources_parallel_state_error_propagates() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = parallel_context(config);
    let resources = vec![
        MockResource::new(ResourceState::Correct).with_state_error("state failed"),
        MockResource::new(ResourceState::Correct).with_state_error("state failed"),
    ];
    let opts = default_opts();

    let result = process_resources(&ctx, resources, &opts);
    assert!(result.is_err());
}

// -----------------------------------------------------------------------
// process_resources_remove — all missing (nothing to remove)
// -----------------------------------------------------------------------

#[test]
fn process_resources_remove_all_missing_skips_silently() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resources = vec![
        MockResource::new(ResourceState::Missing),
        MockResource::new(ResourceState::Incorrect {
            current: "other".to_string(),
        }),
        MockResource::new(ResourceState::Invalid {
            reason: "bad".to_string(),
        }),
    ];

    let result = process_resources_remove(&ctx, resources, "unlink").unwrap();
    assert!(matches!(result, TaskResult::Ok));
}

// -----------------------------------------------------------------------
// process_resources_remove — error during remove propagates
// -----------------------------------------------------------------------

#[test]
fn process_resources_remove_error_propagates() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resources =
        vec![MockResource::new(ResourceState::Correct).with_remove(Err("rm failed".into()))];

    let result = process_resources_remove(&ctx, resources, "unlink");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("rm failed"));
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

// -----------------------------------------------------------------------
// process_resource_states — dry-run
// -----------------------------------------------------------------------

#[test]
fn process_resource_states_dry_run() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = dry_run_context(config);
    let resource_states = vec![
        (
            MockResource::new(ResourceState::Missing).with_apply(Err("should not call".into())),
            ResourceState::Missing,
        ),
        (
            MockResource::new(ResourceState::Correct),
            ResourceState::Correct,
        ),
    ];
    let opts = default_opts();

    let result = process_resource_states(&ctx, resource_states, &opts).unwrap();
    assert!(matches!(result, TaskResult::DryRun));
}

// -----------------------------------------------------------------------
// process_resource_states — parallel dry-run
// -----------------------------------------------------------------------

#[test]
fn process_resource_states_parallel_dry_run() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (mut ctx, _log) = parallel_context(config);
    ctx = ctx.with_dry_run(true);
    let resource_states = vec![
        (
            MockResource::new(ResourceState::Missing).with_apply(Err("no apply".into())),
            ResourceState::Missing,
        ),
        (
            MockResource::new(ResourceState::Missing).with_apply(Err("no apply".into())),
            ResourceState::Missing,
        ),
    ];
    let opts = default_opts();

    let result = process_resource_states(&ctx, resource_states, &opts).unwrap();
    assert!(matches!(result, TaskResult::DryRun));
}
