//! Existing user state must survive preview, convergence, conflicts and removal.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "isolated lifecycle assertions"
)]

mod common;

use std::path::Path;
use std::process::Command;

struct LifecycleCli<'a> {
    repo: &'a Path,
    home: tempfile::TempDir,
}

impl<'a> LifecycleCli<'a> {
    fn new(repo: &'a Path) -> Self {
        Self {
            repo,
            home: tempfile::tempdir().unwrap(),
        }
    }

    fn command(&self, overlay: Option<&Path>, verb: &str, selector: &str) -> Command {
        common::cli_command(self.repo, self.home.path(), overlay, verb, selector)
    }

    fn run(&self, overlay: Option<&Path>, verb: &str, selector: &str, success: bool) -> String {
        output(&mut self.command(overlay, verb, selector), success)
    }
}

fn output(command: &mut Command, success: bool) -> String {
    let result = command.output().unwrap();
    let text = common::combined_output(&result);
    assert_eq!(result.status.success(), success, "{text}");
    text
}

#[path = "audit_lifecycle/git_config.rs"]
mod git_config;
#[path = "audit_lifecycle/managed_files.rs"]
mod managed_files;
#[path = "audit_lifecycle/overlay.rs"]
mod overlay;
#[path = "audit_lifecycle/session.rs"]
mod session;
