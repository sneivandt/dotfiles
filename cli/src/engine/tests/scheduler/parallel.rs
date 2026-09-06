//! Concurrency-specific contracts; no sleeps or assumed thread start order.

use std::sync::Mutex;
use std::time::Duration;

use super::*;

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
