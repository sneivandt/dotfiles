//! Unit tests for the repository update task.
use super::RepositoryUpdateSignal as UpdateSignal;
use super::*;
use crate::infra::exec::{CommandSpec, ExecError, ExecResult, Executor, MockExecutor};
use crate::infra::platform::{Os, Platform};
use crate::test_helpers::{
    CapturingOutput, ScriptedExecutor, empty_config, make_context, make_linux_context,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn git_error(message: &str) -> ExecError {
    ExecError::spawn("git", std::io::Error::other(message.to_string()))
}

fn git_non_zero(message: &str) -> ExecError {
    ExecError::non_zero("git", ExecResult::failure("", message, Some(1)))
}

fn cancelled_git_error() -> ExecError {
    ExecError::Cancelled {
        command: "git".to_string(),
        result: ExecResult::failure("", "", None),
    }
}

fn make_update_context(
    root: &Path,
    overlay: Option<&Path>,
    executor: impl Executor + 'static,
) -> Context {
    git2::Repository::init(root).unwrap();
    if let Some(path) = overlay {
        git2::Repository::init(path).unwrap();
    }
    make_context(
        root.into(),
        overlay.map(Path::to_path_buf),
        Platform::new(Os::Linux, false),
        Arc::new(executor),
    )
}

fn run_update(ctx: &Context) -> (Result<TaskResult>, UpdateSignal) {
    let signal = UpdateSignal::new();
    (UpdateRepository::new(signal.clone()).run(ctx), signal)
}

const FETCH_MAIN: &[&str] = &[
    "fetch",
    "--porcelain",
    "--verbose",
    "--no-tags",
    "--no-recurse-submodules",
    "--",
    "origin",
    "refs/heads/main",
];
const HEAD_SNAPSHOT: &[&str] = &["rev-parse", "HEAD", "--symbolic-full-name", "HEAD"];
const SYMBOLIC_HEAD: &[&str] = &["symbolic-ref", "--quiet", "HEAD"];
const TRACKED_STATUS: &[&str] = &["status", "--porcelain", "--untracked-files=no"];
const UNMERGED_INDEX: &[&str] = &["ls-files", "--unmerged"];
const REMOTE_MAIN: &[&str] = &["config", "--get", "branch.main.remote"];
const MERGE_MAIN: &[&str] = &["config", "--get", "branch.main.merge"];
const MAIN_NEW: &str = fixture_oid(&[b'1'; 40]);
const OVERLAY_NEW: &str = fixture_oid(&[b'2'; 40]);

const fn merge_args(commit: &str) -> [&str; 6] {
    [
        "merge",
        "--ff-only",
        "--no-squash",
        "--no-autostash",
        "--no-overwrite-ignore",
        commit,
    ]
}

const fn fixture_oid(hex: &'static [u8; 40]) -> &'static str {
    match std::str::from_utf8(hex) {
        Ok(oid) => oid,
        Err(_) => panic!("fixture OID must be ASCII"),
    }
}

fn fetch_output(commit: &str) -> String {
    format!("* {} {commit} FETCH_HEAD\n", "0".repeat(40))
}

fn verify_fetched(commit: &str) -> String {
    format!("{commit}^{{commit}}")
}

fn snapshot(oid: &str) -> String {
    format!("{oid}\nrefs/heads/main\n")
}

struct GitScript(ScriptedExecutor);

impl GitScript {
    fn new() -> Self {
        Self(ScriptedExecutor::new())
    }

    fn reply(self, root: &Path, args: &[&str], result: Result<ExecResult, ExecError>) -> Self {
        Self(
            self.0
                .command(CommandSpec::new("git").args(args).current_dir(root), result),
        )
    }

    fn git(self, root: &Path, args: &[&str], stdout: impl Into<String>) -> Self {
        Self(self.0.git(root, args, stdout))
    }

    fn context(self, root: &Path, overlay: Option<&Path>) -> Context {
        let mut mock = MockExecutor::new();
        mock.expect_execute().returning(move |spec| {
            assert!(
                spec.environment()
                    .iter()
                    .all(|(key, _)| key != "GIT_CONFIG_NOSYSTEM"),
                "{spec:?}"
            );
            self.0.execute(spec)
        });
        make_update_context(root, overlay, mock)
    }

    fn run(self, root: &Path, overlay: Option<&Path>) -> (Result<TaskResult>, UpdateSignal) {
        run_update(&self.context(root, overlay))
    }

    fn branch_status(self, root: &Path, branch: &str, status: &str) -> Self {
        self.git(root, SYMBOLIC_HEAD, branch)
            .git(root, TRACKED_STATUS, status)
    }

    fn tracking(self, root: &Path) -> Self {
        self.git(root, REMOTE_MAIN, "origin")
            .git(root, MERGE_MAIN, "refs/heads/main")
    }

    fn before_fetch(self, root: &Path, head: &str) -> Self {
        self.branch_status(root, "refs/heads/main", "")
            .tracking(root)
            .git(root, HEAD_SNAPSHOT, snapshot(head))
    }

    fn fetched_object(self, root: &Path, head: &str, fetched: &str) -> Self {
        self.git(root, HEAD_SNAPSHOT, snapshot(head)).git(
            root,
            &["rev-parse", "--verify", &verify_fetched(fetched)],
            fetched,
        )
    }

    fn ancestor(self, root: &Path, older: &str, newer: &str, is_ancestor: bool) -> Self {
        let result = if is_ancestor {
            Ok(ExecResult::success(""))
        } else {
            Err(git_non_zero(""))
        };
        self.reply(root, &["merge-base", "--is-ancestor", older, newer], result)
    }

    fn checkout_status(self, root: &Path, head: &str, status: &str) -> Self {
        self.git(root, HEAD_SNAPSHOT, snapshot(head))
            .git(root, TRACKED_STATUS, status)
    }

    fn merged_head(self, root: &Path, head: &str) -> Self {
        self.git(root, HEAD_SNAPSHOT, snapshot(head))
            .git(root, UNMERGED_INDEX, "")
    }
}

mod checkout;
mod fetch;
mod multi_repository;
