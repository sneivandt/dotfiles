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

struct FastTask {
    slow_started: Mutex<mpsc::Receiver<()>>,
    worker: mpsc::Sender<std::thread::ThreadId>,
}

impl Task for FastTask {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new("fast")
    }

    fn run(&self, _ctx: &Context) -> Result<TaskResult> {
        self.slow_started
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))?;
        self.worker.send(std::thread::current().id())?;
        Ok(TaskResult::Ok)
    }
}

struct ReleaseTask {
    release: mpsc::Sender<()>,
    fast_worker: Mutex<mpsc::Receiver<std::thread::ThreadId>>,
}

impl Task for ReleaseTask {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new("release")
    }

    crate::engine::task_deps![FastTask];

    fn run(&self, _ctx: &Context) -> Result<TaskResult> {
        assert_eq!(
            self.fast_worker
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))?,
            std::thread::current().id(),
            "the dependent must use the worker freed by fast while slow still holds the other slot"
        );
        self.release.send(())?;
        Ok(TaskResult::Ok)
    }
}

#[test]
fn ready_dependents_reuse_a_free_worker_without_waiting_for_the_phase() {
    let (ctx, log, _root, _guard) = fixture(Mode::Parallel, false);
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let slow = SlowTask {
        started: started_tx,
        release: Mutex::new(release_rx),
    };
    let (worker_tx, worker_rx) = mpsc::channel();
    let fast = FastTask {
        slow_started: Mutex::new(started_rx),
        worker: worker_tx,
    };
    let release = ReleaseTask {
        release: release_tx,
        fast_worker: Mutex::new(worker_rx),
    };
    let tasks: [&dyn Task; 3] = [&release, &slow, &fast];
    let assessments = tasks
        .iter()
        .map(|task| (task.task_id(), task.assess(&ctx)))
        .collect();
    let graph = ResolvedTaskGraph::resolve_assessed(&tasks, &assessments).unwrap();

    let summary = run_tasks_with_limit(&tasks, &graph, &assessments, &ctx, &log, None, 2);

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

#[test]
fn no_parallel_uses_one_worker_for_independent_tasks() {
    let (ctx, log, _root, _guard) = fixture(Mode::Sequential, false);
    let execution = Arc::new(Mutex::new(Vec::new()));
    let tasks: Vec<_> = ["first", "second", "third"]
        .into_iter()
        .map(|key| {
            let execution = Arc::clone(&execution);
            ClaimedTask {
                key,
                claims: Vec::new(),
                body: Box::new(move |_| {
                    execution
                        .lock()
                        .unwrap()
                        .push((key, std::thread::current().id()));
                    Ok(TaskResult::Ok)
                }),
            }
        })
        .collect();
    let refs: Vec<&dyn Task> = tasks.iter().map(|task| -> &dyn Task { task }).collect();
    assert_eq!(
        Mode::Sequential
            .run(&refs, &ctx, &log, None)
            .failure_count(),
        0
    );
    let execution = execution.lock().unwrap();
    let worker = execution.first().expect("first task ran").1;
    assert_eq!(
        *execution,
        [("first", worker), ("second", worker), ("third", worker)]
    );
}

#[test]
#[should_panic(expected = "fixture dispatch panic")]
fn a_dispatch_panic_reaches_the_scheduler_caller() {
    struct PanickingLogKey;

    impl Task for PanickingLogKey {
        fn meta(&self) -> TaskMeta<'_> {
            TaskMeta::new("panicking-log-key")
        }

        #[allow(clippy::panic, reason = "exercise the scheduler's worker unwind path")]
        fn log_key(&self) -> String {
            panic!("fixture dispatch panic");
        }

        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            Ok(TaskResult::Ok)
        }
    }

    let (ctx, log, _root, _guard) = fixture(Mode::Sequential, false);
    Mode::Sequential.run(&[&PanickingLogKey], &ctx, &log, None);
}
