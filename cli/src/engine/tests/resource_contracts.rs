use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use crate::engine::{
    BatchCompletion, BatchReport, IntrinsicState, RemovableResource, Resource, ResourceChange,
    ResourceResult, ResourceState,
};
use crate::engine::{ProcessOpts, TaskResult, process_resources, process_resources_remove};

use super::{bail_opts, test_context};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureStage {
    Discovery,
    Apply,
    Remove,
}

#[derive(Clone)]
struct ContractResource {
    state: Arc<Mutex<ResourceState>>,
    failure: Arc<Mutex<Option<FailureStage>>>,
    state_calls: Arc<AtomicUsize>,
    apply_calls: Arc<AtomicUsize>,
    remove_calls: Arc<AtomicUsize>,
}

impl ContractResource {
    fn new(state: ResourceState) -> Self {
        Self {
            state: Arc::new(Mutex::new(state)),
            failure: Arc::default(),
            state_calls: Arc::new(AtomicUsize::new(0)),
            apply_calls: Arc::new(AtomicUsize::new(0)),
            remove_calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn failing_once(self, stage: FailureStage) -> Self {
        *self.failure.lock().unwrap() = Some(stage);
        self
    }

    fn check_failure(&self, stage: FailureStage) -> ResourceResult<()> {
        let mut failure = self.failure.lock().unwrap();
        if *failure == Some(stage) {
            *failure = None;
            return Err(crate::engine::resource::ResourceError::permission_denied(
                "contract resource",
            ));
        }
        Ok(())
    }

    fn state(&self) -> ResourceState {
        self.state.lock().unwrap().clone()
    }

    fn apply_calls(&self) -> usize {
        self.apply_calls.load(Ordering::SeqCst)
    }

    fn remove_calls(&self) -> usize {
        self.remove_calls.load(Ordering::SeqCst)
    }
}

impl Resource for ContractResource {
    fn description(&self) -> String {
        "contract resource".to_string()
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        self.apply_calls.fetch_add(1, Ordering::SeqCst);
        self.check_failure(FailureStage::Apply)?;
        *self.state.lock().unwrap() = ResourceState::Correct;
        Ok(ResourceChange::Applied)
    }
}

impl RemovableResource for ContractResource {
    fn remove(&self) -> ResourceResult<ResourceChange> {
        self.remove_calls.fetch_add(1, Ordering::SeqCst);
        self.check_failure(FailureStage::Remove)?;
        *self.state.lock().unwrap() = ResourceState::Missing;
        Ok(ResourceChange::Applied)
    }
}

impl IntrinsicState for ContractResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        self.state_calls.fetch_add(1, Ordering::SeqCst);
        self.check_failure(FailureStage::Discovery)?;
        Ok(self.state())
    }
}

/// Expected mode behavior is declared independently of the implementation.
fn process_mode_cases() -> [(ProcessOpts, bool, bool); 4] {
    [
        (ProcessOpts::strict("install"), true, true),
        (ProcessOpts::lenient("install"), true, true),
        (ProcessOpts::install_missing("install"), true, false),
        (ProcessOpts::fix_existing("configure"), false, true),
    ]
}

fn state_cases() -> [ResourceState; 5] {
    [
        ResourceState::Missing,
        ResourceState::Correct,
        ResourceState::Incorrect {
            current: "drifted".into(),
        },
        ResourceState::Invalid {
            reason: "unsafe target".into(),
        },
        ResourceState::Unknown {
            reason: "state probe failed".into(),
        },
    ]
}

fn batch_stats(result: &TaskResult) -> &crate::engine::TaskStats {
    crate::test_helpers::task_batch(result)
}

fn assert_partial_failure_result(result: anyhow::Result<TaskResult>, continued: bool, label: &str) {
    if continued {
        let outcome = result.unwrap();
        let stats = batch_stats(&outcome);
        assert_eq!(stats.changed_count(), 2, "{label}");
        assert_eq!(stats.failed_count(), 1, "{label}");
    } else {
        let error = result.unwrap_err();
        let report = error.downcast_ref::<BatchReport>().unwrap();
        assert_eq!(report.completion(), BatchCompletion::Failed, "{label}");
        assert_eq!(report.stats().changed_count(), 1, "{label}");
        assert_eq!(report.stats().failed_count(), 1, "{label}");
        assert_eq!(report.interrupted_count(), 0, "{label}");
        assert_eq!(report.not_attempted_count(), 1, "{label}");
        assert_eq!(
            error
                .downcast_ref::<crate::engine::resource::ResourceError>()
                .unwrap()
                .category(),
            "permission_denied",
            "{label}"
        );
    }
}

#[test]
fn every_process_mode_preserves_state_counts_and_idempotency() {
    for parallel in [false, true] {
        for dry_run in [false, true] {
            for (opts, applies_missing, applies_incorrect) in process_mode_cases() {
                for state in state_cases() {
                    let label = format!(
                        "{state:?}, {:?}, dry_run={dry_run}, parallel={parallel}",
                        opts.mode
                    );
                    let ctx = test_context(PathBuf::from("/tmp"), None)
                        .0
                        .with_parallel(parallel)
                        .with_dry_run(dry_run);
                    let resource = ContractResource::new(state.clone());
                    let would_apply = match state {
                        ResourceState::Missing => applies_missing,
                        ResourceState::Incorrect { .. } => applies_incorrect,
                        ResourceState::Correct
                        | ResourceState::Invalid { .. }
                        | ResourceState::Unknown { .. } => false,
                    };
                    let unsafe_state = matches!(
                        state,
                        ResourceState::Invalid { .. } | ResourceState::Unknown { .. }
                    );
                    let result = process_resources(&ctx, [resource.clone()], &opts).unwrap();
                    let stats = batch_stats(&result);
                    assert_eq!(
                        (stats.changed_count(), stats.failed_count()),
                        (u32::from(would_apply), u32::from(unsafe_state)),
                        "{label}"
                    );
                    assert_eq!(
                        (resource.apply_calls(), resource.remove_calls()),
                        (usize::from(would_apply && !dry_run), 0),
                        "{label}"
                    );
                    let expected_state = if would_apply && !dry_run {
                        ResourceState::Correct
                    } else {
                        state
                    };
                    assert_eq!(resource.state(), expected_state, "{label}");

                    let repeated = process_resources(&ctx, [resource.clone()], &opts).unwrap();
                    assert_eq!(
                        batch_stats(&repeated).changed_count(),
                        u32::from(would_apply && dry_run),
                        "repeat: {label}"
                    );
                    assert_eq!(
                        batch_stats(&repeated).failed_count(),
                        u32::from(unsafe_state),
                        "repeat: {label}"
                    );
                    assert_eq!(resource.state(), expected_state, "repeat: {label}");
                    assert_eq!(
                        (resource.apply_calls(), resource.remove_calls()),
                        (usize::from(would_apply && !dry_run), 0),
                        "repeat: {label}"
                    );
                }
            }
        }
    }
}

#[test]
fn removal_preserves_ownership_dry_run_and_idempotency() {
    for parallel in [false, true] {
        for dry_run in [false, true] {
            for state in state_cases() {
                let label = format!("{state:?}, dry_run={dry_run}, parallel={parallel}");
                let ctx = test_context(PathBuf::from("/tmp"), None)
                    .0
                    .with_parallel(parallel)
                    .with_dry_run(dry_run);
                let resource = ContractResource::new(state.clone());
                let would_remove = matches!(state, ResourceState::Correct);
                let unknown = matches!(state, ResourceState::Unknown { .. });
                let expected_state = if would_remove && !dry_run {
                    ResourceState::Missing
                } else {
                    state
                };
                for repeat in [false, true] {
                    let result =
                        process_resources_remove(&ctx, [resource.clone()], "remove").unwrap();
                    let stats = batch_stats(&result);
                    assert_eq!(
                        stats.changed_count(),
                        u32::from(would_remove && (!repeat || dry_run)),
                        "repeat={repeat}: {label}"
                    );
                    assert_eq!(stats.failed_count(), u32::from(unknown), "{label}");
                    assert_eq!(
                        (resource.apply_calls(), resource.remove_calls()),
                        (0, usize::from(would_remove && !dry_run)),
                        "{label}"
                    );
                    assert_eq!(resource.state(), expected_state, "{label}");
                }
            }
        }
    }
}

#[test]
fn mixed_batches_keep_unsafe_resources_untouched() {
    for parallel in [false, true] {
        for dry_run in [false, true] {
            let ctx = test_context(PathBuf::from("/tmp"), None)
                .0
                .with_parallel(parallel)
                .with_dry_run(dry_run);
            let resources: Vec<_> = state_cases()
                .into_iter()
                .map(ContractResource::new)
                .collect();
            let result = process_resources(&ctx, resources.clone(), &bail_opts()).unwrap();
            assert_eq!(
                (
                    batch_stats(&result).changed_count(),
                    batch_stats(&result).failed_count()
                ),
                (2, 2)
            );
            for (resource, initial) in resources.iter().zip(state_cases()) {
                let changes = matches!(
                    initial,
                    ResourceState::Missing | ResourceState::Incorrect { .. }
                ) && !dry_run;
                assert_eq!(resource.apply_calls(), usize::from(changes));
                assert_eq!(resource.remove_calls(), 0);
                assert_eq!(
                    resource.state(),
                    if changes {
                        ResourceState::Correct
                    } else {
                        initial
                    }
                );
            }

            let removals: Vec<_> = state_cases()
                .into_iter()
                .map(ContractResource::new)
                .collect();
            let summary = process_resources_remove(&ctx, removals.clone(), "remove").unwrap();
            assert_eq!(
                (
                    batch_stats(&summary).changed_count(),
                    batch_stats(&summary).failed_count()
                ),
                (1, 1)
            );
            for (resource, initial) in removals.iter().zip(state_cases()) {
                let removes = matches!(initial, ResourceState::Correct) && !dry_run;
                assert_eq!(resource.remove_calls(), usize::from(removes));
                assert_eq!(resource.apply_calls(), 0);
                assert_eq!(
                    resource.state(),
                    if removes {
                        ResourceState::Missing
                    } else {
                        initial
                    }
                );
            }
        }
    }
}

#[test]
fn retry_after_partial_failure_rediscovers_state_without_repeating_completed_mutations() {
    for remove in [false, true] {
        for lenient in [false, true] {
            if remove && lenient {
                continue;
            }
            for discovery_fails in [false, true] {
                let label = format!(
                    "remove={remove}, lenient={lenient}, discovery_fails={discovery_fails}"
                );
                let ctx = test_context(PathBuf::from("/fixture"), None).0;
                let initial = if remove {
                    ResourceState::Correct
                } else {
                    ResourceState::Missing
                };
                let converged = if remove {
                    ResourceState::Missing
                } else {
                    ResourceState::Correct
                };
                let stage = if discovery_fails {
                    FailureStage::Discovery
                } else if remove {
                    FailureStage::Remove
                } else {
                    FailureStage::Apply
                };
                let resources = [
                    ContractResource::new(initial.clone()),
                    ContractResource::new(initial.clone()).failing_once(stage),
                    ContractResource::new(initial.clone()),
                ];
                let opts = if lenient {
                    ProcessOpts::lenient("install")
                } else {
                    ProcessOpts::strict("install")
                };
                let run = || {
                    if remove {
                        process_resources_remove(&ctx, resources.clone(), "remove")
                    } else {
                        process_resources(&ctx, resources.clone(), &opts)
                    }
                };
                let continued = lenient && !discovery_fails;
                assert_partial_failure_result(run(), continued, &label);
                for (index, resource) in resources.iter().enumerate() {
                    let finished = index == 0 || (index == 2 && continued);
                    assert_eq!(
                        &resource.state(),
                        if finished { &converged } else { &initial },
                        "first run, item {index}: {label}"
                    );
                    let attempted = usize::from(finished || (index == 1 && !discovery_fails));
                    assert_eq!(
                        (resource.apply_calls(), resource.remove_calls()),
                        if remove {
                            (0, attempted)
                        } else {
                            (attempted, 0)
                        },
                        "first run, item {index}: {label}"
                    );
                }

                for repeat in [1, 2] {
                    let retry_result = run().unwrap();
                    let stats = batch_stats(&retry_result);
                    let changes = if repeat == 1 {
                        2 - u32::from(continued)
                    } else {
                        0
                    };
                    assert_eq!(stats.changed_count(), changes, "retry {repeat}: {label}");
                    assert_eq!(stats.already_ok_count(), 3 - changes, "{label}");
                    assert_eq!(stats.skipped_count(), 0, "{label}");
                    assert_eq!(stats.failed_count(), 0, "{label}");
                    for (index, resource) in resources.iter().enumerate() {
                        assert_eq!(resource.state(), converged, "{label}");
                        let mutations = 1 + usize::from(index == 1 && !discovery_fails);
                        assert_eq!(
                            (resource.apply_calls(), resource.remove_calls()),
                            if remove {
                                (0, mutations)
                            } else {
                                (mutations, 0)
                            },
                            "retry {repeat}, item {index}: {label}"
                        );
                        assert_eq!(
                            resource.state_calls.load(Ordering::SeqCst),
                            repeat + usize::from(index != 2 || continued),
                            "retry {repeat}, item {index}: {label}"
                        );
                    }
                }
            }
        }
    }
}
