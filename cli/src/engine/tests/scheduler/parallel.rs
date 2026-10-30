//! Concurrency-specific contracts; no sleeps or assumed thread start order.

use std::sync::Mutex;
use std::time::Duration;

use super::*;

type ClaimBody = dyn Fn(&Context) -> Result<TaskResult> + Send + Sync;

struct ClaimedTask {
    key: &'static str,
    claims: Vec<AccessClaim>,
    body: Box<ClaimBody>,
}

impl Task for ClaimedTask {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new(self.key)
    }

    fn task_id(&self) -> TaskId {
        TaskId::dynamic::<Self>(self.key)
    }

    fn access_claims(&self, _ctx: &Context) -> Vec<AccessClaim> {
        self.claims.clone()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        (self.body)(ctx)
    }
}

#[test]
fn conflicting_task_bodies_never_overlap() {
    let (ctx, log, _root, _guard) = fixture(Mode::Parallel, false);
    let active = Arc::new(AtomicUsize::new(0));
    let tasks: Vec<_> = ["first", "second", "third"]
        .into_iter()
        .map(|key| {
            let active = Arc::clone(&active);
            ClaimedTask {
                key,
                claims: vec![AccessClaim::exclusive("fixture", "shared")],
                body: Box::new(move |_| {
                    assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
                    std::thread::yield_now();
                    assert_eq!(active.fetch_sub(1, Ordering::SeqCst), 1);
                    Ok(TaskResult::Ok)
                }),
            }
        })
        .collect();
    let refs: Vec<&dyn Task> = tasks.iter().map(|task| -> &dyn Task { task }).collect();
    assert_eq!(
        Mode::Parallel.run(&refs, &ctx, &log, None).failure_count(),
        0
    );
}

#[test]
fn independent_task_claims_allow_bodies_to_overlap() {
    let (ctx, log, _root, _guard) = fixture(Mode::Parallel, false);
    let (left_tx, left_rx) = mpsc::channel();
    let (right_tx, right_rx) = mpsc::channel();
    let left_rx = Mutex::new(left_rx);
    let right_rx = Mutex::new(right_rx);
    let left = ClaimedTask {
        key: "left",
        claims: vec![AccessClaim::exclusive("fixture", "left")],
        body: Box::new(move |_| {
            left_tx.send(())?;
            right_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))?;
            Ok(TaskResult::Ok)
        }),
    };
    let right = ClaimedTask {
        key: "right",
        claims: vec![AccessClaim::exclusive("fixture", "right")],
        body: Box::new(move |_| {
            right_tx.send(())?;
            left_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))?;
            Ok(TaskResult::Ok)
        }),
    };
    assert_eq!(
        Mode::Parallel
            .run(&[&left, &right], &ctx, &log, None)
            .failure_count(),
        0
    );
}

#[test]
fn cancellation_interrupts_task_claim_acquisition_without_running_the_body() {
    let (ctx, log, _root, _guard) = fixture(Mode::Parallel, false);
    let claim = AccessClaim::exclusive("fixture", "document");
    let holder = ctx.try_access(std::slice::from_ref(&claim)).unwrap();
    let runs = Arc::new(AtomicUsize::new(0));
    let body_runs = Arc::clone(&runs);
    let task = ClaimedTask {
        key: "waiting",
        claims: vec![claim.clone()],
        body: Box::new(move |_| {
            body_runs.fetch_add(1, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }),
    };
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    std::thread::scope(|scope| {
        let ctx = &ctx;
        scope.spawn(move || {
            started_tx.send(()).unwrap();
            done_tx.send(engine::execute(&task, ctx)).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        ctx.cancellation_token().cancel();
        assert_eq!(
            done_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            TaskStatus::Interrupted
        );
    });
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    assert_eq!(log.task_entries()[0].status, TaskStatus::Interrupted);
    assert!(ctx.try_access(std::slice::from_ref(&claim)).is_none());
    drop(holder);
    assert!(ctx.try_access(&[claim]).is_some());
}

#[test]
fn task_claims_cover_requirement_probes_and_their_consumers() {
    for dry_run in [false, true] {
        let (base_ctx, _log, _root, _guard) = fixture(Mode::Parallel, false);
        let ctx = base_ctx.with_dry_run(dry_run);
        let claim = AccessClaim::exclusive("fixture", "required-document");
        let body_claim = claim.clone();
        let observer = ctx.clone();
        let task = ClaimedTask {
            key: "claimed-requirement",
            claims: vec![claim.clone()],
            body: Box::new(move |task_ctx| {
                let requirement = engine::Requirement::new("document-input", |_, stage| {
                    assert_eq!(
                        stage,
                        if dry_run {
                            engine::RequirementStage::Preview
                        } else {
                            engine::RequirementStage::Apply
                        }
                    );
                    assert!(
                        observer
                            .try_access(std::slice::from_ref(&body_claim))
                            .is_none()
                    );
                    Ok(engine::RequirementState::Satisfied(17))
                });
                engine::process_requirement(task_ctx, &requirement, |evidence| {
                    assert_eq!(evidence, 17);
                    assert!(
                        observer
                            .try_access(std::slice::from_ref(&body_claim))
                            .is_none()
                    );
                    Ok(TaskResult::Ok)
                })
            }),
        };
        assert_eq!(engine::execute(&task, &ctx), TaskStatus::Ok);
        assert!(ctx.try_access(&[claim]).is_some());
    }
}

struct SlowTask {
    started: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl Task for SlowTask {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new("slow")
    }

    fn run(&self, _ctx: &Context) -> Result<TaskResult> {
        self.started.send(())?;
        self.release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))?;
        Ok(TaskResult::Ok)
    }
}

struct FastTask(Mutex<mpsc::Receiver<()>>);

impl Task for FastTask {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new("fast")
    }

    fn run(&self, _ctx: &Context) -> Result<TaskResult> {
        self.0
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))?;
        Ok(TaskResult::Ok)
    }
}

struct ReleaseTask(mpsc::Sender<()>);

impl Task for ReleaseTask {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new("release")
    }

    crate::engine::task_deps![FastTask];

    fn run(&self, _ctx: &Context) -> Result<TaskResult> {
        self.0.send(())?;
        Ok(TaskResult::Ok)
    }
}

#[test]
fn ready_dependents_run_without_waiting_for_unrelated_in_flight_tasks() {
    let (ctx, log, _root, _guard) = fixture(Mode::Parallel, false);
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let slow = SlowTask {
        started: started_tx,
        release: Mutex::new(release_rx),
    };
    let fast = FastTask(Mutex::new(started_rx));
    let release = ReleaseTask(release_tx);

    let summary = Mode::Parallel.run(&[&slow, &fast, &release], &ctx, &log, None);

    assert_eq!(
        summary.failure_count(),
        0,
        "slow and fast must overlap, not run in waves"
    );
    assert!(
        log.task_entries()
            .iter()
            .all(|entry| entry.status == TaskStatus::Ok)
    );
    let entries = log.task_entries();
    let fast_pos = entries
        .iter()
        .position(|entry| entry.name == "fast")
        .unwrap();
    let slow_pos = entries
        .iter()
        .position(|entry| entry.name == "slow")
        .unwrap();
    assert!(
        fast_pos < slow_pos,
        "records retain completion order, not catalog order"
    );
}
