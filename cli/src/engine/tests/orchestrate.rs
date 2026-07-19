use crate::engine::mode::ProcessOpts;
use crate::engine::{
    Resource, ResourceChange, ResourceResult, ResourceState, ResourceStateProvider,
};
use crate::engine::{
    TaskResult, process_resources, process_resources_remove, process_resources_with_provider,
};
use crate::test_helpers::empty_config;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use super::{
    MockResource, bail_opts, default_opts, dry_run_context, parallel_context, test_context,
};

struct PrecomputedResource {
    resource: MockResource,
    state: ResourceState,
}

impl Resource for PrecomputedResource {
    fn description(&self) -> String {
        self.resource.description()
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        self.resource.apply()
    }

    fn remove(&self) -> ResourceResult<ResourceChange> {
        self.resource.remove()
    }
}

struct PrecomputedStateProvider;

impl ResourceStateProvider<PrecomputedResource> for PrecomputedStateProvider {
    type Cache = ();

    fn load(&self, _resources: &[PrecomputedResource]) -> anyhow::Result<Self::Cache> {
        Ok(())
    }

    fn current_state(
        &self,
        resource: &PrecomputedResource,
        _cache: &Self::Cache,
    ) -> anyhow::Result<ResourceState> {
        Ok(resource.state.clone())
    }
}

struct CountingStateProvider {
    loads: Arc<AtomicUsize>,
}

impl ResourceStateProvider<PrecomputedResource> for CountingStateProvider {
    type Cache = ();

    fn load(&self, _resources: &[PrecomputedResource]) -> anyhow::Result<Self::Cache> {
        self.loads.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn current_state(
        &self,
        resource: &PrecomputedResource,
        _cache: &Self::Cache,
    ) -> anyhow::Result<ResourceState> {
        Ok(resource.state.clone())
    }
}

const fn is_success(result: &TaskResult) -> bool {
    matches!(result, TaskResult::Ok | TaskResult::OkWithMessage(_))
        || matches!(
            result,
            TaskResult::Batch(stats) if stats.failed == 0
        )
}

const fn is_batch_failure(result: &TaskResult) -> bool {
    matches!(result, TaskResult::Batch(stats) if stats.failed > 0)
}

const fn is_batch_change(result: &TaskResult) -> bool {
    matches!(result, TaskResult::Batch(stats) if stats.changed > 0)
}

fn process_precomputed_states(
    ctx: &crate::engine::Context,
    resource_states: impl IntoIterator<Item = (MockResource, ResourceState)>,
    opts: &ProcessOpts<'_>,
) -> anyhow::Result<TaskResult> {
    let resources = resource_states
        .into_iter()
        .map(|(resource, state)| PrecomputedResource { resource, state });
    process_resources_with_provider(ctx, resources, &PrecomputedStateProvider, opts)
}

fn test_ctx() -> crate::engine::Context {
    test_context(empty_config(PathBuf::from("/tmp"))).0
}

fn parallel_ctx() -> crate::engine::Context {
    parallel_context(empty_config(PathBuf::from("/tmp"))).0
}

fn dry_ctx() -> crate::engine::Context {
    dry_run_context(empty_config(PathBuf::from("/tmp"))).0
}

// -----------------------------------------------------------------------
// process_resources
// -----------------------------------------------------------------------

#[test]
fn process_resources_mixed_states() {
    let ctx = test_ctx();
    let resources = vec![
        MockResource::new(ResourceState::Correct),
        MockResource::new(ResourceState::Missing),
        MockResource::new(ResourceState::Invalid {
            reason: "bad".to_string(),
        }),
    ];
    let opts = default_opts();

    let result = process_resources(&ctx, resources, &opts).unwrap();
    assert!(is_batch_failure(&result));
}

#[test]
fn process_resources_empty_list() {
    let ctx = test_ctx();
    let resources: Vec<MockResource> = vec![];
    let opts = default_opts();

    let result = process_resources(&ctx, resources, &opts).unwrap();
    assert!(is_success(&result));
}

// -----------------------------------------------------------------------
// process_precomputed_states
// -----------------------------------------------------------------------

#[test]
fn process_precomputed_states_applies_precomputed() {
    let ctx = test_ctx();
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

    let result = process_precomputed_states(&ctx, resource_states, &opts).unwrap();
    assert!(is_success(&result));
}

#[test]
fn process_resources_with_provider_empty_list_skips_provider_load() {
    let ctx = test_ctx();
    let resources: Vec<PrecomputedResource> = vec![];
    let opts = default_opts();
    let loads = Arc::new(AtomicUsize::new(0));
    let provider = CountingStateProvider {
        loads: Arc::clone(&loads),
    };

    let result = process_resources_with_provider(&ctx, resources, &provider, &opts).unwrap();

    assert!(is_success(&result));
    assert_eq!(loads.load(Ordering::SeqCst), 0);
}

// -----------------------------------------------------------------------
// process_resources_remove
// -----------------------------------------------------------------------

#[test]
fn process_resources_remove_removes_correct_resources() {
    let ctx = test_ctx();
    let resources = vec![
        MockResource::new(ResourceState::Correct),
        MockResource::new(ResourceState::Missing),
    ];

    let result = process_resources_remove(&ctx, resources, "unlink").unwrap();
    assert!(is_success(&result));
}

#[test]
fn process_resources_remove_dry_run() {
    let ctx = dry_ctx();
    // Remove should NOT be called in dry-run
    let resources =
        vec![MockResource::new(ResourceState::Correct).with_remove(Err("should not call".into()))];

    let result = process_resources_remove(&ctx, resources, "unlink").unwrap();
    assert!(is_batch_change(&result));
}

// -----------------------------------------------------------------------
// Parallel dispatch — process_resources
// -----------------------------------------------------------------------

#[test]
fn process_resources_parallel_accumulates_stats() {
    let ctx = parallel_ctx();
    // Three resources: one already correct, one missing (will be applied), one invalid (failed).
    let resources = vec![
        MockResource::new(ResourceState::Correct),
        MockResource::new(ResourceState::Missing),
        MockResource::new(ResourceState::Invalid {
            reason: "bad".to_string(),
        }),
    ];
    let opts = default_opts();

    let result = process_resources(&ctx, resources, &opts).unwrap();
    assert!(is_batch_failure(&result));
}

#[test]
fn process_resources_parallel_single_resource_runs_sequentially() {
    // When there is only one resource, the sequential path is taken even if parallel=true.
    let ctx = parallel_ctx();
    let resources = vec![MockResource::new(ResourceState::Missing)];
    let opts = default_opts();

    let result = process_resources(&ctx, resources, &opts).unwrap();
    assert!(is_success(&result));
}

#[test]
fn process_resources_parallel_bail_on_error_propagates() {
    let ctx = parallel_ctx();
    let resources = vec![
        MockResource::new(ResourceState::Missing).with_apply(Err("fatal".into())),
        MockResource::new(ResourceState::Missing).with_apply(Err("fatal".into())),
    ];
    let opts = bail_opts();

    let result = process_resources(&ctx, resources, &opts);
    assert!(result.is_err());
}

// -----------------------------------------------------------------------
// Parallel dispatch — process_precomputed_states
// -----------------------------------------------------------------------

#[test]
fn process_precomputed_states_parallel_dispatch() {
    let ctx = parallel_ctx();
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

    let result = process_precomputed_states(&ctx, resource_states, &opts).unwrap();
    assert!(is_success(&result));
}

// -----------------------------------------------------------------------
// Parallel dispatch — process_resources_remove
// -----------------------------------------------------------------------

#[test]
fn process_resources_remove_parallel_dispatch() {
    let ctx = parallel_ctx();
    let resources = vec![
        MockResource::new(ResourceState::Correct),
        MockResource::new(ResourceState::Missing),
    ];

    let result = process_resources_remove(&ctx, resources, "unlink").unwrap();
    assert!(is_success(&result));
}

// -----------------------------------------------------------------------
// Error propagation — current_state() failures
// -----------------------------------------------------------------------

#[test]
fn process_resources_current_state_error_propagates() {
    let ctx = test_ctx();
    let resources =
        vec![MockResource::new(ResourceState::Missing).with_state_error("state failed")];
    let opts = default_opts();

    let result = process_resources(&ctx, resources, &opts);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("state failed"));
}

#[test]
fn process_resources_remove_current_state_error_propagates() {
    let ctx = test_ctx();
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
    let ctx = test_ctx();
    let resources = vec![MockResource::new(ResourceState::Missing).with_apply(Err("fatal".into()))];
    let opts = bail_opts();

    let result = process_resources(&ctx, resources, &opts);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("fatal"));
}

// -----------------------------------------------------------------------
// Stats accumulation across multiple resources in process_precomputed_states
// -----------------------------------------------------------------------

#[test]
fn process_precomputed_states_stats_accumulate_across_resources() {
    let ctx = test_ctx();
    // 2 correct, 1 missing (applied), 1 invalid (failed)
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
    let result = process_precomputed_states(&ctx, resource_states, &opts).unwrap();
    assert!(is_batch_failure(&result));
}

// -----------------------------------------------------------------------
// Parallel — dry-run behaviour
// -----------------------------------------------------------------------

#[test]
fn process_resources_parallel_dry_run() {
    let ctx = parallel_ctx().with_dry_run(true);
    // apply() would error if called — dry-run must skip it
    let resources = vec![
        MockResource::new(ResourceState::Missing).with_apply(Err("no apply".into())),
        MockResource::new(ResourceState::Missing).with_apply(Err("no apply".into())),
    ];
    let opts = default_opts();
    let result = process_resources(&ctx, resources, &opts).unwrap();
    assert!(is_batch_change(&result));
}

#[test]
fn process_resources_remove_parallel_dry_run() {
    let ctx = parallel_ctx().with_dry_run(true);
    let resources = vec![
        MockResource::new(ResourceState::Correct).with_remove(Err("no remove".into())),
        MockResource::new(ResourceState::Correct).with_remove(Err("no remove".into())),
    ];
    let result = process_resources_remove(&ctx, resources, "unlink").unwrap();
    assert!(is_batch_change(&result));
}

#[test]
fn process_precomputed_states_parallel_no_bail_reports_failure() {
    let ctx = parallel_ctx();
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
    let result = process_precomputed_states(&ctx, resource_states, &opts).unwrap();
    assert!(is_batch_failure(&result));
}

// -----------------------------------------------------------------------
// process_precomputed_states — empty list
// -----------------------------------------------------------------------

#[test]
fn process_precomputed_states_empty_list() {
    let ctx = test_ctx();
    let resource_states: Vec<(MockResource, ResourceState)> = vec![];
    let opts = default_opts();

    let result = process_precomputed_states(&ctx, resource_states, &opts).unwrap();
    assert!(is_success(&result));
}

// -----------------------------------------------------------------------
// process_resources_remove — empty list
// -----------------------------------------------------------------------

#[test]
fn process_resources_remove_empty_list() {
    let ctx = test_ctx();
    let resources: Vec<MockResource> = vec![];

    let result = process_resources_remove(&ctx, resources, "unlink").unwrap();
    assert!(is_success(&result));
}

// -----------------------------------------------------------------------
// process_precomputed_states — bail on error
// -----------------------------------------------------------------------

#[test]
fn process_precomputed_states_bail_on_error_propagates() {
    let ctx = test_ctx();
    let resource_states = vec![(
        MockResource::new(ResourceState::Missing).with_apply(Err("fatal".into())),
        ResourceState::Missing,
    )];
    let opts = bail_opts();

    let result = process_precomputed_states(&ctx, resource_states, &opts);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("fatal"));
}

// -----------------------------------------------------------------------
// process_precomputed_states — lenient error skipping
// -----------------------------------------------------------------------

#[test]
fn process_precomputed_states_lenient_reports_failure() {
    let ctx = test_ctx();
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

    let result = process_precomputed_states(&ctx, resource_states, &opts).unwrap();
    assert!(is_batch_failure(&result));
}

// -----------------------------------------------------------------------
// process_resources — lenient error skipping at orchestration level
// -----------------------------------------------------------------------

#[test]
fn process_resources_lenient_reports_apply_errors() {
    let ctx = test_ctx();
    let resources = vec![
        MockResource::new(ResourceState::Missing).with_apply(Err("oops".into())),
        MockResource::new(ResourceState::Correct),
    ];
    let opts = default_opts();

    let result = process_resources(&ctx, resources, &opts).unwrap();
    assert!(is_batch_failure(&result));
}

// -----------------------------------------------------------------------
// Parallel — process_precomputed_states bail on error
// -----------------------------------------------------------------------

#[test]
fn process_precomputed_states_parallel_bail_on_error_propagates() {
    let ctx = parallel_ctx();
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

    let result = process_precomputed_states(&ctx, resource_states, &opts);
    assert!(result.is_err());
}

// -----------------------------------------------------------------------
// Parallel — process_resources_remove with state error
// -----------------------------------------------------------------------

#[test]
fn process_resources_remove_parallel_state_error_propagates() {
    let ctx = parallel_ctx();
    let resources = vec![
        MockResource::new(ResourceState::Correct).with_state_error("state failed"),
        MockResource::new(ResourceState::Correct).with_state_error("state failed"),
    ];

    let result = process_resources_remove(&ctx, resources, "unlink");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("state failed"));
}

// -----------------------------------------------------------------------
// Parallel — process_resources with state error
// -----------------------------------------------------------------------

#[test]
fn process_resources_parallel_state_error_propagates() {
    let ctx = parallel_ctx();
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
    let ctx = test_ctx();
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
    assert!(is_success(&result));
}

// -----------------------------------------------------------------------
// process_resources_remove — error during remove propagates
// -----------------------------------------------------------------------

#[test]
fn process_resources_remove_error_propagates() {
    let ctx = test_ctx();
    let resources =
        vec![MockResource::new(ResourceState::Correct).with_remove(Err("rm failed".into()))];

    let result = process_resources_remove(&ctx, resources, "unlink");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("rm failed"));
}

// -----------------------------------------------------------------------
// process_precomputed_states — dry-run
// -----------------------------------------------------------------------

#[test]
fn process_precomputed_states_dry_run() {
    let ctx = dry_ctx();
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

    let result = process_precomputed_states(&ctx, resource_states, &opts).unwrap();
    assert!(is_batch_change(&result));
}

// -----------------------------------------------------------------------
// process_precomputed_states — parallel dry-run
// -----------------------------------------------------------------------

#[test]
fn process_precomputed_states_parallel_dry_run() {
    let ctx = parallel_ctx().with_dry_run(true);
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

    let result = process_precomputed_states(&ctx, resource_states, &opts).unwrap();
    assert!(is_batch_change(&result));
}

// -----------------------------------------------------------------------
// process_resources — multiple sequential failures in lenient mode
// -----------------------------------------------------------------------

#[test]
fn process_resources_lenient_reports_multiple_apply_errors() {
    let ctx = test_ctx();
    let resources = vec![
        MockResource::new(ResourceState::Missing).with_apply(Err("error1".into())),
        MockResource::new(ResourceState::Missing).with_apply(Err("error2".into())),
        MockResource::new(ResourceState::Correct),
    ];
    let opts = default_opts();

    let result = process_resources(&ctx, resources, &opts).unwrap();
    assert!(is_batch_failure(&result));
}

// -----------------------------------------------------------------------
// process_resources_remove — parallel remove error propagation
// -----------------------------------------------------------------------

#[test]
fn process_resources_remove_parallel_error_propagates() {
    let ctx = parallel_ctx();
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
    let ctx = test_ctx();
    // Cancel before processing begins
    ctx.cancellation_token().cancel();
    // apply() would error if called — cancellation should prevent it
    let resources = vec![
        MockResource::new(ResourceState::Missing).with_apply(Err("should not call".into())),
        MockResource::new(ResourceState::Missing).with_apply(Err("should not call".into())),
    ];
    let opts = default_opts();

    let result = process_resources(&ctx, resources, &opts).unwrap();
    // Finishes with zero stats (no resources processed)
    assert!(is_success(&result));
}

// -----------------------------------------------------------------------
// Cancellation — sequential process_precomputed_states
// -----------------------------------------------------------------------

#[test]
fn process_precomputed_states_stops_on_cancellation() {
    let ctx = test_ctx();
    ctx.cancellation_token().cancel();
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

    let result = process_precomputed_states(&ctx, resource_states, &opts).unwrap();
    assert!(is_success(&result));
}

// -----------------------------------------------------------------------
// Cancellation — sequential process_resources_remove
// -----------------------------------------------------------------------

#[test]
fn process_resources_remove_stops_on_cancellation() {
    let ctx = test_ctx();
    ctx.cancellation_token().cancel();
    // remove() would error if called
    let resources = vec![
        MockResource::new(ResourceState::Correct).with_remove(Err("no remove".into())),
        MockResource::new(ResourceState::Correct).with_remove(Err("no remove".into())),
    ];

    let result = process_resources_remove(&ctx, resources, "unlink").unwrap();
    assert!(is_success(&result));
}

// -----------------------------------------------------------------------
// ProcessOpts sequential flag — forces sequential even with parallel ctx
// -----------------------------------------------------------------------

#[test]
fn sequential_opts_forces_sequential_processing() {
    let ctx = parallel_ctx();
    // Use sequential opts — should not dispatch to parallel path
    let resources = vec![
        MockResource::new(ResourceState::Correct),
        MockResource::new(ResourceState::Missing),
        MockResource::new(ResourceState::Correct),
    ];
    let opts = ProcessOpts::strict("install").sequential();

    let result = process_resources(&ctx, resources, &opts).unwrap();
    assert!(is_success(&result));
}

#[test]
fn sequential_opts_forces_sequential_for_resource_states() {
    let ctx = parallel_ctx();
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

    let result = process_precomputed_states(&ctx, resource_states, &opts).unwrap();
    assert!(is_success(&result));
}
