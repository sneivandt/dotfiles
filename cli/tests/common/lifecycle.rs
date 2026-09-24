//! Reusable, isolated lifecycle assertions for real config-backed tasks.

use std::collections::BTreeMap;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use dotfiles_cli::testing::config::{Config, ConfigStore, profiles};
use dotfiles_cli::testing::engine::{BatchCompletion, BatchReport};
use dotfiles_cli::testing::env::MapEnv;
use dotfiles_cli::testing::exec::{CommandSpec, ExecError, ExecResult, Executor};
use dotfiles_cli::testing::logging::{Log, Logger, isolated_logger};
use dotfiles_cli::testing::platform::Platform;
use dotfiles_cli::testing::tasks::{Context, ContextOpts, Task, TaskResult, TaskStats};

/// A subprocess tripwire, not an always-successful executor.
///
/// Git settings use native libgit2 mutations. Their command count must remain
/// zero even on apply; filesystem snapshots independently observe native writes.
#[derive(Debug, Default)]
struct CommandCounter(AtomicUsize);

impl Executor for CommandCounter {
    fn execute(&self, spec: CommandSpec) -> Result<ExecResult, ExecError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("unexpected lifecycle subprocess: {spec:?}");
    }

    fn which(&self, _: &str) -> bool {
        false
    }

    fn which_path(&self, program: &str) -> anyhow::Result<PathBuf> {
        anyhow::bail!("fixture has no executable: {program}")
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Contents {
    Directory,
    File(Vec<u8>),
    Link(PathBuf),
}

#[derive(Debug, PartialEq, Eq)]
struct Entry {
    contents: Contents,
    len: u64,
    modified: Option<SystemTime>,
    readonly: bool,
    #[cfg(unix)]
    mode: u32,
    #[cfg(windows)]
    attributes: u32,
}

/// Includes empty directories, bytes, link targets, and mutation-relevant
/// metadata. Access times are deliberately excluded because reads can update them.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TreeSnapshot(BTreeMap<PathBuf, Entry>);

fn snapshot_tree(path: &Path, key: &Path, entries: &mut BTreeMap<PathBuf, Entry>) {
    let metadata = std::fs::symlink_metadata(path).expect("snapshot metadata");
    let link = std::fs::read_link(path).ok();
    let is_directory = link.is_none() && metadata.is_dir();
    let contents = link.map_or_else(
        || {
            if is_directory {
                Contents::Directory
            } else {
                assert!(
                    metadata.is_file(),
                    "unsupported fixture entry: {}",
                    path.display()
                );
                Contents::File(std::fs::read(path).expect("snapshot file"))
            }
        },
        Contents::Link,
    );
    entries.insert(
        key.to_path_buf(),
        Entry {
            contents,
            len: metadata.len(),
            modified: metadata.modified().ok(),
            readonly: metadata.permissions().readonly(),
            #[cfg(unix)]
            mode: metadata.permissions().mode(),
            #[cfg(windows)]
            attributes: metadata.file_attributes(),
        },
    );
    if is_directory {
        for child in std::fs::read_dir(path).expect("snapshot directory") {
            let child = child.expect("snapshot child");
            snapshot_tree(&child.path(), &key.join(child.file_name()), entries);
        }
    }
}

/// One logical resource action; sorting retains duplicates to test cardinality.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Action {
    verb: String,
    subject: String,
    planned: bool,
}

impl Action {
    pub(crate) fn new(verb: &str, subject: &str, planned: bool) -> Self {
        Self {
            verb: verb.into(),
            subject: subject.replace('\\', "/"),
            planned,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Run {
    result: anyhow::Result<TaskResult>,
    actions: Vec<Action>,
}

impl Run {
    pub(crate) fn assert_actions(&self, verb: &str, subjects: &[&str], planned: bool) {
        let mut expected: Vec<_> = subjects
            .iter()
            .map(|subject| Action::new(verb, subject, planned))
            .collect();
        expected.sort();
        assert_eq!(
            self.actions, expected,
            "exact logical action multiset (including duplicate detection)"
        );
    }

    pub(crate) fn assert_batch(&self, changed: u32, current: u32) {
        let TaskResult::Batch(stats) = self.result.as_ref().expect("task must complete") else {
            panic!("expected a resource batch, got {:?}", self.result);
        };
        assert_counts(stats, changed, current, 0);
    }

    pub(crate) fn assert_no_resources(&self) {
        match self.result.as_ref().expect("task must complete") {
            TaskResult::Ok => {}
            TaskResult::Batch(stats) => assert_counts(stats, 0, 0, 0),
            result @ (TaskResult::DryRun
            | TaskResult::CheckPassed
            | TaskResult::NotApplicable(_)
            | TaskResult::Skipped { .. }
            | TaskResult::Failed(_)) => {
                panic!("expected an empty successful task, got {result:?}");
            }
        }
    }

    pub(crate) fn assert_partial_failure(&self, changed: u32, not_attempted: u32) {
        let error = self.result.as_ref().expect_err("Nth resource must fail");
        let report = error
            .downcast_ref::<BatchReport>()
            .expect("strict failure must preserve its partial batch");
        assert_eq!(
            report.completion(),
            BatchCompletion::Failed,
            "a resource failure is not cancellation"
        );
        assert_counts(report.stats(), changed, 0, 1);
        assert_eq!(
            report.not_attempted_count(),
            not_attempted,
            "later resources must not be dispatched"
        );
        assert_eq!(report.interrupted_count(), 0, "no cancellation injected");
    }
}

fn assert_counts(stats: &TaskStats, changed: u32, current: u32, failed: u32) {
    assert_eq!(stats.changed_count(), changed, "changed resources");
    assert_eq!(
        stats.already_ok_count(),
        current,
        "already-correct resources"
    );
    assert_eq!(stats.skipped_count(), 0, "unexpected skipped resources");
    assert_eq!(stats.failed_count(), failed, "failed resources");
}

/// Owns every writable path used by a lifecycle, including persistent run logs.
pub(crate) struct Fixture {
    sandbox: tempfile::TempDir,
    pub(crate) repo: PathBuf,
    pub(crate) home: PathBuf,
    platform: Platform,
    commands: Arc<CommandCounter>,
    run_number: AtomicUsize,
}

impl Fixture {
    pub(crate) fn new(platform: Platform) -> Self {
        let sandbox = tempfile::Builder::new()
            .prefix("real-task-")
            .tempdir()
            .expect("create native temporary fixture");
        let repo = sandbox.path().join("repo with spaces");
        let home = sandbox.path().join("home with spaces");
        std::fs::create_dir_all(&home).expect("create fixture home");
        super::common::setup_minimal_repo(&repo);
        let repository = git2::Repository::init(&repo).expect("initialize fixture Git repository");
        repository
            .config()
            .expect("repository config")
            .set_str("core.hooksPath", ".git/hooks")
            .expect("override inherited hook path");
        Self {
            sandbox,
            repo,
            home,
            platform,
            commands: Arc::default(),
            run_number: AtomicUsize::new(0),
        }
    }

    pub(crate) fn config(&self, file: &str, content: &str) {
        write(&self.repo.join("conf").join(file), content);
    }

    pub(crate) fn snapshot(&self) -> TreeSnapshot {
        let mut entries = BTreeMap::new();
        snapshot_tree(&self.repo, Path::new("repo"), &mut entries);
        snapshot_tree(&self.home, Path::new("home"), &mut entries);
        TreeSnapshot(entries)
    }

    pub(crate) fn command_count(&self) -> usize {
        self.commands.0.load(Ordering::SeqCst)
    }

    /// Reload config and construct a new context/logger for every invocation,
    /// so idempotency cannot accidentally depend on cached state.
    pub(crate) fn run(
        &self,
        dry_run: bool,
        parallel: bool,
        task: impl FnOnce(ConfigStore) -> Box<dyn Task>,
    ) -> Run {
        let profile = profiles::resolve("base", self.platform).expect("fixture profile");
        let config =
            Config::load(&self.repo, &profile, self.platform, None).expect("fixture config");
        let run_number = self.run_number.fetch_add(1, Ordering::SeqCst);
        let log_dir = self.sandbox.path().join(format!("log-{run_number}"));
        let mut logger = isolated_logger("lifecycle", &log_dir);
        logger.set_dry_run(dry_run);
        let log = Arc::new(logger);
        let log_path = log.log_path().expect("isolated persistent run log");
        assert!(
            log_path.starts_with(self.sandbox.path()),
            "logs must remain within the fixture"
        );
        let sink: Arc<dyn Log> = Arc::<Logger>::clone(&log);
        let executor: Arc<dyn Executor> = Arc::<CommandCounter>::clone(&self.commands);
        let env = MapEnv::new()
            .with("HOME", &self.home)
            .with("USERPROFILE", &self.home)
            .with("XDG_CONFIG_HOME", self.home.join(".config"))
            .with("XDG_STATE_HOME", self.home.join(".state"))
            .with("GIT_CONFIG_GLOBAL", self.home.join(".gitconfig"))
            .with("GIT_CONFIG_NOSYSTEM", "1")
            .into_handle();
        let ctx = Context::new(
            self.repo.clone(),
            None,
            self.platform,
            sink,
            executor,
            env,
            ContextOpts {
                dry_run,
                parallel,
                is_ci: Some(false),
            },
        )
        .expect("isolated task context");
        let task = task(ConfigStore::from_config(config));
        assert!(task.should_run(&ctx), "fixture task must be applicable");
        let result = task.run(&ctx);
        let content = std::fs::read_to_string(log_path).expect("read task run log");
        let mut actions = Vec::new();
        for line in content.lines() {
            let Some((_, json)) = line.split_once("] [record] ") else {
                continue;
            };
            let record: serde_json::Value =
                serde_json::from_str(json).expect("valid structured run-log JSON");
            assert_eq!(
                record.get("schema").and_then(serde_json::Value::as_u64),
                Some(1),
                "known record schema"
            );
            if record.get("type").and_then(serde_json::Value::as_str) == Some("action") {
                actions.push(Action::new(
                    record
                        .get("verb")
                        .and_then(serde_json::Value::as_str)
                        .expect("action verb"),
                    record
                        .get("subject")
                        .and_then(serde_json::Value::as_str)
                        .expect("action subject"),
                    record
                        .get("planned")
                        .and_then(serde_json::Value::as_bool)
                        .expect("action planned flag"),
                ));
            }
        }
        actions.sort();
        Run { result, actions }
    }
}

pub(crate) fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().expect("fixture path parent"))
        .expect("create fixture parent");
    std::fs::write(path, content).expect("write fixture");
}

/// Shared preview → apply → fresh-context repeat contract. Domain assertions
/// inspect real desired state after apply and after the no-op repeat.
pub(crate) fn assert_lifecycle(
    fixture: &Fixture,
    parallel: bool,
    task: impl Fn(ConfigStore) -> Box<dyn Task>,
    verb: &str,
    subjects: &[&str],
    resource_count: u32,
    assert_converged: impl Fn(),
) {
    let expected = u32::try_from(subjects.len()).expect("fixture action count");
    let unchanged = resource_count
        .checked_sub(expected)
        .expect("one action per changed resource");
    let before = fixture.snapshot();
    let commands_before_preview = fixture.command_count();
    let preview = fixture.run(true, parallel, &task);
    preview.assert_batch(expected, unchanged);
    preview.assert_actions(verb, subjects, true);
    assert_eq!(
        fixture.snapshot(),
        before,
        "preview must not mutate the tree"
    );
    assert_eq!(
        fixture.command_count(),
        commands_before_preview,
        "preview subprocess mutations"
    );

    let apply = fixture.run(false, parallel, &task);
    apply.assert_batch(expected, unchanged);
    apply.assert_actions(verb, subjects, false);
    assert_converged();

    let installed = fixture.snapshot();
    let commands_before_repeat = fixture.command_count();
    let repeat = fixture.run(false, parallel, task);
    repeat.assert_batch(0, resource_count);
    repeat.assert_actions(verb, &[], false);
    assert_eq!(
        fixture.snapshot(),
        installed,
        "repeat must not mutate the tree"
    );
    assert_eq!(
        fixture.command_count(),
        commands_before_repeat,
        "repeat subprocess mutations"
    );
    assert_converged();
}
