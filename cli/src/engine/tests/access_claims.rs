//! Exclusive access across complete resource lifecycles and execution pools.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::time::Duration;

use crate::engine::{
    AccessClaim, BatchCompletion, BatchReport, Context, IntrinsicState, ProcessOpts,
    RemovableResource, Resource, ResourceChange, ResourceResult, ResourceState, process_resources,
    process_resources_remove, process_resources_with_cache,
};
use crate::test_helpers::make_linux_context;

fn claim(key: &str) -> AccessClaim {
    AccessClaim::exclusive("fixture", key)
}

fn context() -> Context {
    make_linux_context("/fixture".into(), None).with_parallel(true)
}

struct Probe {
    claims: Vec<AccessClaim>,
    active: Arc<AtomicUsize>,
    overlap: Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>,
    fail: Option<&'static str>,
    removal: bool,
}

impl Probe {
    fn new(key: &str, active: &Arc<AtomicUsize>) -> Self {
        Self {
            claims: vec![claim(key)],
            active: Arc::clone(active),
            overlap: None,
            fail: None,
            removal: false,
        }
    }

    fn finish(&self, stage: &str) -> ResourceResult<ResourceChange> {
        assert_eq!(self.active.fetch_sub(1, Ordering::SeqCst), 1);
        if self.fail == Some(stage) {
            return Err(anyhow::anyhow!("fixture {stage} failure").into());
        }
        Ok(ResourceChange::Applied)
    }
}

impl Resource for Probe {
    fn access_claims(&self) -> Vec<AccessClaim> {
        self.claims.clone()
    }

    fn description(&self) -> String {
        "claimed fixture".into()
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        self.finish("apply")
    }
}

impl IntrinsicState for Probe {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        if self.fail == Some("discovery") {
            return Err(anyhow::anyhow!("fixture discovery failure").into());
        }
        assert_eq!(
            self.active.fetch_add(1, Ordering::SeqCst),
            0,
            "discovery must not overlap a conflicting lifecycle"
        );
        if let Some((entered, peer)) = &self.overlap {
            entered.send(()).map_err(anyhow::Error::from)?;
            peer.recv_timeout(Duration::from_secs(5))
                .map_err(anyhow::Error::from)?;
        }
        Ok(if self.removal {
            ResourceState::Correct
        } else {
            ResourceState::Missing
        })
    }
}

impl RemovableResource for Probe {
    fn remove(&self) -> ResourceResult<ResourceChange> {
        self.finish("remove")
    }
}

#[test]
fn conflicting_resources_hold_claims_across_discovery_and_mutation_in_all_pipelines() {
    for parallel in [false, true] {
        for pipeline in ["intrinsic", "cached", "remove"] {
            let ctx = context().with_parallel(parallel);
            let active = Arc::new(AtomicUsize::new(0));
            let resources = (0..32)
                .map(|_| {
                    let mut probe = Probe::new("document", &active);
                    probe.removal = pipeline == "remove";
                    probe
                })
                .collect::<Vec<_>>();
            let result = match pipeline {
                "cached" => process_resources_with_cache(
                    &ctx,
                    resources,
                    &(),
                    |resource, ()| resource.current_state(),
                    &ProcessOpts::strict("configure"),
                ),
                "remove" => process_resources_remove(&ctx, resources, "remove"),
                _ => process_resources(&ctx, resources, &ProcessOpts::strict("configure")),
            };
            assert!(
                result.is_ok(),
                "{pipeline}, parallel={parallel}: {result:?}"
            );
            assert_eq!(active.load(Ordering::SeqCst), 0);
            assert!(ctx.try_access(&[claim("document")]).is_some());
        }
    }
}

#[test]
fn independent_resources_can_discover_concurrently() {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap();
    for remove in [false, true] {
        let (left_tx, left_rx) = mpsc::channel();
        let (right_tx, right_rx) = mpsc::channel();
        let resources = [
            ("copilot-document", (left_tx, right_rx)),
            ("codex-document", (right_tx, left_rx)),
        ]
        .map(|(key, overlap)| {
            let mut probe = Probe::new(key, &Arc::new(AtomicUsize::new(0)));
            probe.overlap = Some(overlap);
            probe.removal = remove;
            probe
        });
        let _result = pool.install(|| {
            if remove {
                process_resources_remove(&context(), resources, "remove")
            } else {
                process_resources(&context(), resources, &ProcessOpts::strict("configure"))
            }
            .unwrap()
        });
    }
}

#[test]
fn lifecycle_errors_release_claims_without_poisoning_later_work() {
    for stage in ["discovery", "apply", "remove"] {
        let ctx = context();
        let active = Arc::new(AtomicUsize::new(0));
        let mut resource = Probe::new("document", &active);
        resource.fail = Some(stage);
        resource.removal = stage == "remove";
        let result = if resource.removal {
            process_resources_remove(&ctx, [resource], "remove")
        } else {
            process_resources(&ctx, [resource], &ProcessOpts::strict("configure"))
        };
        assert!(result.is_err(), "{stage}");
        let _result = process_resources(
            &ctx,
            [Probe::new("document", &active)],
            &ProcessOpts::strict("configure"),
        )
        .unwrap();
    }
}

#[test]
fn task_scope_preserves_inner_exclusion_and_rejects_undeclared_nested_claims() {
    let ctx = context();
    let (scoped, guard) = ctx.scoped_access(vec![claim("document")]).unwrap();
    let active = Arc::new(AtomicUsize::new(0));
    let _result = process_resources(
        &scoped,
        (0..8).map(|_| Probe::new("document", &active)),
        &ProcessOpts::strict("configure"),
    )
    .unwrap();
    let error = process_resources(
        &scoped,
        [Probe::new("undeclared", &active)],
        &ProcessOpts::strict("configure"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("subset"));
    assert!(ctx.try_access(&[claim("document")]).is_none());
    drop(guard);
    assert!(ctx.try_access(&[claim("document")]).is_some());
}

#[test]
fn cancellation_stops_a_claim_wait_without_discovery_or_releasing_the_holder() {
    let ctx = context();
    let holder = ctx.try_access(&[claim("document")]).unwrap();
    let (tx, rx) = mpsc::channel();
    let active = Arc::new(AtomicUsize::new(0));
    std::thread::scope(|scope| {
        let worker_ctx = &ctx;
        let active = &active;
        scope.spawn(move || {
            tx.send(process_resources(
                worker_ctx,
                [Probe::new("document", active)],
                &ProcessOpts::strict("configure"),
            ))
            .unwrap();
        });
        ctx.cancellation_token().cancel();
        let error = rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap_err();
        let report = error.downcast_ref::<BatchReport>().unwrap();
        assert_eq!(report.completion(), BatchCompletion::Interrupted);
        assert_eq!(report.not_attempted_count(), 1);
    });
    assert_eq!(active.load(Ordering::SeqCst), 0);
    assert!(
        ctx.wait_for_access().is_err(),
        "context must propagate cancellation rather than busy-spin"
    );
    assert!(ctx.try_access(&[claim("document")]).is_none());
    drop(holder);
    assert!(ctx.try_access(&[claim("document")]).is_some());
}

#[test]
fn waiting_resource_dispatch_cannot_starve_a_claim_holders_single_worker_pool() {
    let ctx = context();
    let (holder_ctx, guard) = ctx.scoped_access(vec![claim("x"), claim("y")]).unwrap();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let active = Arc::new(AtomicUsize::new(0));
    std::thread::scope(|scope| {
        let ctx = &ctx;
        scope.spawn(move || {
            started_tx.send(()).unwrap();
            let result = process_resources(
                ctx,
                [Probe::new("x", &active)],
                &ProcessOpts::strict("configure"),
            );
            done_tx.send(result).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let _result = pool.install(|| {
            process_resources(
                &holder_ctx,
                ["x", "y"].map(|key| Probe::new(key, &Arc::new(AtomicUsize::new(0)))),
                &ProcessOpts::strict("configure"),
            )
            .unwrap()
        });
        assert!(
            done_rx.try_recv().is_err(),
            "contender must still wait for the task claim"
        );
        drop(guard);
        assert!(
            done_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .is_ok()
        );
    });
}
