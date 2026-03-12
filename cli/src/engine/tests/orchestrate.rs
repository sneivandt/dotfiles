use crate::engine::mode::ProcessOpts;
use crate::engine::{
    TaskResult, process_resource_states, process_resources, process_resources_remove,
};
use crate::resources::ResourceState;
use crate::tasks::test_helpers::empty_config;
use std::path::PathBuf;

use super::{
    MockResource, bail_opts, default_opts, dry_run_context, parallel_context, test_context,
};

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

// -----------------------------------------------------------------------
// process_resources — multiple sequential failures in lenient mode
// -----------------------------------------------------------------------

#[test]
fn process_resources_lenient_skips_multiple_apply_errors() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    let resources = vec![
        MockResource::new(ResourceState::Missing).with_apply(Err("error1".into())),
        MockResource::new(ResourceState::Missing).with_apply(Err("error2".into())),
        MockResource::new(ResourceState::Correct),
    ];
    let opts = default_opts();

    let result = process_resources(&ctx, resources, &opts).unwrap();
    assert!(matches!(result, TaskResult::Ok));
}

// -----------------------------------------------------------------------
// process_resources_remove — parallel remove error propagation
// -----------------------------------------------------------------------

#[test]
fn process_resources_remove_parallel_error_propagates() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = parallel_context(config);
    let resources = vec![
        MockResource::new(ResourceState::Correct).with_remove(Err("rm error".into())),
        MockResource::new(ResourceState::Correct).with_remove(Err("rm error".into())),
    ];

    let result = process_resources_remove(&ctx, resources, "unlink");
    assert!(result.is_err());
}

// -----------------------------------------------------------------------
// Cancellation — sequential process_resources
// -----------------------------------------------------------------------

#[test]
fn process_resources_stops_on_cancellation() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    // Cancel before processing begins
    ctx.cancelled.cancel();
    // apply() would error if called — cancellation should prevent it
    let resources = vec![
        MockResource::new(ResourceState::Missing).with_apply(Err("should not call".into())),
        MockResource::new(ResourceState::Missing).with_apply(Err("should not call".into())),
    ];
    let opts = default_opts();

    let result = process_resources(&ctx, resources, &opts).unwrap();
    // Finishes with zero stats (no resources processed)
    assert!(matches!(result, TaskResult::Ok));
}

// -----------------------------------------------------------------------
// Cancellation — sequential process_resource_states
// -----------------------------------------------------------------------

#[test]
fn process_resource_states_stops_on_cancellation() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    ctx.cancelled.cancel();
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
    assert!(matches!(result, TaskResult::Ok));
}

// -----------------------------------------------------------------------
// Cancellation — sequential process_resources_remove
// -----------------------------------------------------------------------

#[test]
fn process_resources_remove_stops_on_cancellation() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    ctx.cancelled.cancel();
    // remove() would error if called
    let resources = vec![
        MockResource::new(ResourceState::Correct).with_remove(Err("no remove".into())),
        MockResource::new(ResourceState::Correct).with_remove(Err("no remove".into())),
    ];

    let result = process_resources_remove(&ctx, resources, "unlink").unwrap();
    assert!(matches!(result, TaskResult::Ok));
}

// -----------------------------------------------------------------------
// ProcessOpts sequential flag — forces sequential even with parallel ctx
// -----------------------------------------------------------------------

#[test]
fn sequential_opts_forces_sequential_processing() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = parallel_context(config);
    // Use sequential opts — should not dispatch to parallel path
    let resources = vec![
        MockResource::new(ResourceState::Correct),
        MockResource::new(ResourceState::Missing),
        MockResource::new(ResourceState::Correct),
    ];
    let opts = ProcessOpts::strict("install").sequential();

    let result = process_resources(&ctx, resources, &opts).unwrap();
    assert!(matches!(result, TaskResult::Ok));
}

#[test]
fn sequential_opts_forces_sequential_for_resource_states() {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = parallel_context(config);
    let resource_states = vec![
        (
            MockResource::new(ResourceState::Correct),
            ResourceState::Correct,
        ),
        (
            MockResource::new(ResourceState::Missing),
            ResourceState::Missing,
        ),
    ];
    let opts = ProcessOpts::strict("install").sequential();

    let result = process_resource_states(&ctx, resource_states, &opts).unwrap();
    assert!(matches!(result, TaskResult::Ok));
}
