use crate::engine::mode::ProcessOpts;
use crate::engine::orchestrate::process_resources_with_state;
use crate::engine::{
    Context, IntrinsicState, RemovableResource, Resource, ResourceChange, ResourceResult,
    ResourceState, TaskResult, process_resources, process_resources_remove,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use super::{MockResource, bail_opts, default_opts, test_context};

struct ThreadBoundResource(std::thread::ThreadId);

impl Resource for ThreadBoundResource {
    fn description(&self) -> String {
        "thread-bound resource".into()
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        assert_eq!(
            std::thread::current().id(),
            self.0,
            "apply must remain on the caller"
        );
        Ok(ResourceChange::Applied)
    }
}

impl IntrinsicState for ThreadBoundResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        assert_eq!(
            std::thread::current().id(),
            self.0,
            "discovery must remain on the caller"
        );
        Ok(ResourceState::Missing)
    }
}

/// Deliberately has no `IntrinsicState` implementation: cached processing must
/// use the supplied discovery function, including its errors.
struct PrecomputedResource(MockResource);

impl Resource for PrecomputedResource {
    fn description(&self) -> String {
        self.0.description()
    }
    fn apply(&self) -> ResourceResult<ResourceChange> {
        self.0.apply()
    }
}

#[derive(Clone, Copy, Debug)]
enum Pipeline {
    Intrinsic,
    Precomputed,
    Remove,
}

impl Pipeline {
    fn run(
        self,
        ctx: &Context,
        resources: Vec<MockResource>,
        opts: &ProcessOpts,
    ) -> anyhow::Result<TaskResult> {
        match self {
            Self::Intrinsic => process_resources(ctx, resources, opts),
            Self::Precomputed => process_resources_with_state(
                ctx,
                resources.into_iter().map(PrecomputedResource),
                |resource| {
                    resource
                        .0
                        .state_result
                        .clone()
                        .map_err(|error| anyhow::anyhow!(error).into())
                },
                opts,
            ),
            Self::Remove => process_resources_remove(ctx, resources, "unlink"),
        }
    }
}

fn test_ctx() -> Context {
    test_context(PathBuf::from("/tmp"), None).0
}
fn parallel_ctx() -> Context {
    test_ctx().with_parallel(true)
}

#[track_caller]
fn assert_counts(result: &TaskResult, expected: (u32, u32, u32, u32), case: &str) {
    let TaskResult::Batch(stats) = result else {
        panic!("{case}: expected batch statistics, got {result:?}");
    };
    assert_eq!(
        (
            stats.changed_count(),
            stats.already_ok_count(),
            stats.skipped_count(),
            stats.failed_count()
        ),
        expected,
        "{case}"
    );
}

#[test]
fn pipelines_accumulate_mixed_states_in_both_execution_modes() {
    use ResourceState::{Correct, Missing};
    for parallel in [false, true] {
        for pipeline in [Pipeline::Intrinsic, Pipeline::Precomputed, Pipeline::Remove] {
            for (case, states, applied, removed) in [
                (
                    "mixed invalid",
                    vec![
                        Correct,
                        Missing,
                        ResourceState::Invalid {
                            reason: "bad".into(),
                        },
                    ],
                    (1, 1, 0, 1),
                    (1, 2, 0, 0),
                ),
                (
                    "cached accumulation",
                    vec![
                        Correct,
                        Correct,
                        Missing,
                        ResourceState::Invalid {
                            reason: "bad".into(),
                        },
                    ],
                    (1, 2, 0, 1),
                    (2, 2, 0, 0),
                ),
                (
                    "missing and correct",
                    vec![Missing, Correct],
                    (1, 1, 0, 0),
                    (1, 1, 0, 0),
                ),
                (
                    "incorrect",
                    vec![
                        Missing,
                        Correct,
                        ResourceState::Incorrect {
                            current: "old".into(),
                        },
                    ],
                    (2, 1, 0, 0),
                    (1, 2, 0, 0),
                ),
                (
                    "unmanaged",
                    vec![
                        Missing,
                        ResourceState::Incorrect {
                            current: "other".into(),
                        },
                        ResourceState::Invalid {
                            reason: "bad".into(),
                        },
                    ],
                    (2, 0, 0, 1),
                    (0, 3, 0, 0),
                ),
            ] {
                let resources = states.into_iter().map(MockResource::new).collect();
                let result = pipeline
                    .run(
                        &test_ctx().with_parallel(parallel),
                        resources,
                        &default_opts(),
                    )
                    .unwrap_or_else(|error| {
                        panic!("{case}, {pipeline:?}, parallel={parallel}: {error:#}")
                    });
                assert_counts(
                    &result,
                    if matches!(pipeline, Pipeline::Remove) {
                        removed
                    } else {
                        applied
                    },
                    &format!("{case}, {pipeline:?}, parallel={parallel}"),
                );
            }
        }
    }
}

#[test]
fn empty_pipelines_preserve_their_result_and_never_query_state() {
    for parallel in [false, true] {
        let ctx = test_ctx().with_parallel(parallel);
        for pipeline in [Pipeline::Intrinsic, Pipeline::Precomputed, Pipeline::Remove] {
            let result = pipeline.run(&ctx, vec![], &default_opts()).unwrap();
            if matches!(pipeline, Pipeline::Remove) {
                assert_counts(
                    &result,
                    (0, 0, 0, 0),
                    &format!("empty {pipeline:?}, parallel={parallel}"),
                );
            } else {
                assert!(matches!(result, TaskResult::Ok), "{pipeline:?}");
            }
        }
        let checks = AtomicUsize::new(0);
        let result = process_resources_with_state(
            &ctx,
            Vec::<PrecomputedResource>::new(),
            |_| {
                checks.fetch_add(1, Ordering::SeqCst);
                Ok(ResourceState::Missing)
            },
            &default_opts(),
        )
        .unwrap();
        assert!(matches!(result, TaskResult::Ok));
        assert_eq!(checks.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn dry_run_and_cancellation_never_mutate_any_pipeline() {
    for parallel in [false, true] {
        for count in [1, 2] {
            for pipeline in [Pipeline::Intrinsic, Pipeline::Precomputed, Pipeline::Remove] {
                for cancelled in [false, true] {
                    let ctx = test_ctx().with_parallel(parallel).with_dry_run(!cancelled);
                    if cancelled {
                        ctx.cancellation_token().cancel();
                    }
                    let resources = (0..count)
                        .map(|_| {
                            MockResource::new(if matches!(pipeline, Pipeline::Remove) {
                                ResourceState::Correct
                            } else {
                                ResourceState::Missing
                            })
                            .with_apply(Err("must not apply".into()))
                            .with_remove(Err("must not remove".into()))
                        })
                        .collect();
                    let result = pipeline.run(&ctx, resources, &default_opts());
                    if cancelled {
                        assert_unstarted_batch(&result.unwrap_err(), count);
                    } else {
                        assert_counts(
                            &result.unwrap(),
                            (count, 0, 0, 0),
                            &format!("dry run {pipeline:?}, parallel={parallel}, count={count}"),
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn dry_run_keeps_current_resources_current() {
    for parallel in [false, true] {
        for pipeline in [Pipeline::Intrinsic, Pipeline::Precomputed] {
            let resources = vec![
                MockResource::new(ResourceState::Missing).with_apply(Err("must not apply".into())),
                MockResource::new(ResourceState::Correct).with_apply(Err("must not apply".into())),
            ];
            let result = pipeline
                .run(
                    &test_ctx().with_parallel(parallel).with_dry_run(true),
                    resources,
                    &default_opts(),
                )
                .unwrap();
            assert_counts(
                &result,
                (1, 1, 0, 0),
                &format!("dry run mixed {pipeline:?}, parallel={parallel}"),
            );
        }
    }
}

#[test]
fn state_and_mutation_errors_propagate_in_every_pipeline() {
    for parallel in [false, true] {
        for count in [1, 2] {
            for pipeline in [Pipeline::Intrinsic, Pipeline::Precomputed, Pipeline::Remove] {
                for state_error in [false, true] {
                    let resources = (0..count)
                        .map(|_| {
                            let resource =
                                MockResource::new(if matches!(pipeline, Pipeline::Remove) {
                                    ResourceState::Correct
                                } else {
                                    ResourceState::Missing
                                });
                            if state_error {
                                resource.with_state_error("state failed")
                            } else {
                                resource
                                    .with_apply(Err("mutation failed".into()))
                                    .with_remove(Err("mutation failed".into()))
                            }
                        })
                        .collect();
                    let result =
                        pipeline.run(&test_ctx().with_parallel(parallel), resources, &bail_opts());
                    let error = format!("{:#}", result.unwrap_err());
                    assert!(
                        error.contains(if state_error {
                            "state failed"
                        } else {
                            "mutation failed"
                        }),
                        "{pipeline:?}, parallel={parallel}: {error}"
                    );
                }
            }
        }
    }
}

#[test]
fn lenient_discovery_errors_still_propagate() {
    for parallel in [false, true] {
        for pipeline in [Pipeline::Intrinsic, Pipeline::Precomputed, Pipeline::Remove] {
            let resources = (0..2)
                .map(|_| MockResource::new(ResourceState::Missing).with_state_error("state failed"))
                .collect();
            let error = pipeline
                .run(
                    &test_ctx().with_parallel(parallel),
                    resources,
                    &default_opts(),
                )
                .unwrap_err();
            assert!(
                format!("{error:#}").contains("state failed"),
                "{pipeline:?}"
            );
        }
    }
}

#[test]
fn lenient_apply_errors_are_counted_and_do_not_stop_other_resources() {
    for parallel in [false, true] {
        for pipeline in [Pipeline::Intrinsic, Pipeline::Precomputed] {
            for failures in [1, 2] {
                let mut resources: Vec<_> = (0..failures)
                    .map(|index| {
                        MockResource::new(ResourceState::Missing)
                            .with_apply(Err(format!("error{index}")))
                    })
                    .collect();
                resources.push(MockResource::new(ResourceState::Correct));
                let result = pipeline
                    .run(
                        &test_ctx().with_parallel(parallel),
                        resources,
                        &default_opts(),
                    )
                    .unwrap();
                assert_counts(
                    &result,
                    (0, 1, 0, failures),
                    &format!("lenient {pipeline:?}, parallel={parallel}, failures={failures}"),
                );
            }
        }
    }
}

#[test]
fn a_single_resource_stays_on_the_calling_thread_in_parallel_mode() {
    let resources = vec![ThreadBoundResource(std::thread::current().id())];
    let result = process_resources(&parallel_ctx(), resources, &default_opts()).unwrap();
    assert_counts(&result, (1, 0, 0, 0), "one resource on caller");
}

fn assert_unstarted_batch(error: &anyhow::Error, count: u32) {
    let report = error.downcast_ref::<crate::engine::BatchReport>().unwrap();
    assert_eq!(
        report.completion(),
        crate::engine::BatchCompletion::Interrupted
    );
    assert_eq!(report.stats().changed_count(), 0);
    assert_eq!(report.stats().failed_count(), 0);
    assert_eq!(report.interrupted_count(), 0);
    assert_eq!(report.not_attempted_count(), count);
}

#[test]
fn sequential_opts_forces_sequential_processing() {
    let ctx = parallel_ctx();
    let caller = std::thread::current().id();
    let resources = (0..3).map(|_| ThreadBoundResource(caller));
    let opts = ProcessOpts::strict("install").sequential();

    let result = process_resources(&ctx, resources, &opts).unwrap();
    assert_counts(&result, (3, 0, 0, 0), "forced sequential processing");
}

#[test]
fn sequential_opts_forces_sequential_for_resource_states() {
    let ctx = parallel_ctx();
    let caller = std::thread::current().id();
    let resources = (0..3).map(|_| ThreadBoundResource(caller));
    let opts = ProcessOpts::strict("install").sequential();

    let result = process_resources_with_state(
        &ctx,
        resources,
        |_| {
            assert_eq!(std::thread::current().id(), caller);
            Ok(ResourceState::Missing)
        },
        &opts,
    )
    .unwrap();
    assert_counts(&result, (3, 0, 0, 0), "forced sequential processing");
}

#[test]
fn batch_discovery_loads_once_and_uses_the_shared_cache_in_both_modes() {
    for parallel in [false, true] {
        for dry_run in [false, true] {
            let ctx = test_ctx().with_parallel(parallel).with_dry_run(dry_run);
            let loads = AtomicUsize::new(0);
            let checks = AtomicUsize::new(0);
            let resources = ["missing", "correct"]
                .into_iter()
                .map(|name| {
                    let resource = MockResource::new(ResourceState::Unknown {
                        reason: "intrinsic state must not be used".into(),
                    })
                    .with_desc(name);
                    if dry_run || name == "correct" {
                        resource.with_apply(Err("must not mutate this resource".into()))
                    } else {
                        resource
                    }
                })
                .collect();
            let result = crate::engine::run_batch_resource_task(
                &ctx,
                resources,
                |resources, _| {
                    loads.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(resources.len(), 2);
                    Ok(std::collections::HashMap::from([
                        ("missing".to_string(), ResourceState::Missing),
                        ("correct".to_string(), ResourceState::Correct),
                    ]))
                },
                |resource, cache| {
                    checks.fetch_add(1, Ordering::SeqCst);
                    Ok(cache.get(&resource.description()).unwrap().clone())
                },
                &ProcessOpts::strict("install"),
            )
            .unwrap();
            let TaskResult::Batch(stats) = result else {
                panic!("expected batch statistics");
            };
            assert_eq!(stats.changed_count(), 1);
            assert_eq!(stats.already_ok_count(), 1);
            assert_eq!(stats.failed_count(), 0);
            assert_eq!(loads.load(Ordering::SeqCst), 1);
            assert_eq!(checks.load(Ordering::SeqCst), 2);
        }
    }
}

#[test]
fn state_discovery_error_prevents_mutation_in_both_modes() {
    for parallel in [false, true] {
        let ctx = test_ctx().with_parallel(parallel);
        let resources = (0..2).map(|_| {
            MockResource::new(ResourceState::Missing)
                .with_apply(Err("apply must not run after a failed state query".into()))
        });
        let result = process_resources_with_state(
            &ctx,
            resources,
            |_| Err(anyhow::anyhow!("state query failed").into()),
            &ProcessOpts::strict("install"),
        );
        assert!(format!("{:#}", result.unwrap_err()).contains("state query failed"));
    }
}

#[test]
fn discovery_errors_identify_the_resource_and_preserve_the_typed_cause() {
    struct UnreadableResource;

    impl Resource for UnreadableResource {
        fn description(&self) -> String {
            "unreadable resource".into()
        }

        fn apply(&self) -> ResourceResult<ResourceChange> {
            panic!("failed discovery must never authorize apply")
        }
    }

    impl IntrinsicState for UnreadableResource {
        fn current_state(&self) -> ResourceResult<ResourceState> {
            Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "probe denied").into())
        }
    }

    impl RemovableResource for UnreadableResource {
        fn remove(&self) -> ResourceResult<ResourceChange> {
            panic!("failed discovery must never authorize removal")
        }
    }

    for parallel in [false, true] {
        for dry_run in [false, true] {
            for pipeline in [Pipeline::Intrinsic, Pipeline::Precomputed, Pipeline::Remove] {
                let ctx = test_ctx().with_parallel(parallel).with_dry_run(dry_run);
                let resources = [UnreadableResource, UnreadableResource];
                let error = match pipeline {
                    Pipeline::Intrinsic => process_resources(&ctx, resources, &bail_opts()),
                    Pipeline::Precomputed => process_resources_with_state(
                        &ctx,
                        resources,
                        IntrinsicState::current_state,
                        &bail_opts(),
                    ),
                    Pipeline::Remove => process_resources_remove(&ctx, resources, "unlink"),
                }
                .unwrap_err();
                let message = format!("{error:#}");
                let expected = if matches!(pipeline, Pipeline::Remove) {
                    "failed to check unreadable resource for removal"
                } else {
                    "failed to check unreadable resource"
                };
                assert!(message.contains(expected), "{pipeline:?}: {message}");
                let cause = error
                    .downcast_ref::<crate::engine::resource::ResourceError>()
                    .unwrap();
                assert_eq!(cause.category(), "io");
                assert!(matches!(
                    cause,
                    crate::engine::resource::ResourceError::Io { source }
                        if source.kind() == std::io::ErrorKind::PermissionDenied
                ));
            }
        }
    }
}
