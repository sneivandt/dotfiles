//! Unit tests for the VS Code extension install task.

use super::*;
use crate::infra::ConfigHandle;
use crate::test_helpers::{empty_config, make_linux_context};
use std::path::PathBuf;

fn ext() -> String {
    "github.copilot".to_string()
}

#[test]
fn should_run_false_when_no_extensions_configured() {
    let ctx = make_linux_context(empty_config(PathBuf::from("/tmp")));
    assert!(!InstallVsCodeExtensions::new(ConfigHandle::new(vec![])).should_run(&ctx));
}

#[test]
fn should_run_true_when_extensions_configured() {
    let ctx = make_linux_context(empty_config(PathBuf::from("/tmp")));
    let task = InstallVsCodeExtensions::new(ConfigHandle::new(vec![ext()]));
    assert!(task.should_run(&ctx));
}

#[test]
fn run_skips_when_vscode_cli_not_found() {
    // Default make_linux_context uses TestExecutor with which_result=false,
    // so find_code_command returns None for both "code-insiders" and "code".
    let ctx = make_linux_context(empty_config(PathBuf::from("/tmp")));
    let task = InstallVsCodeExtensions::new(ConfigHandle::new(vec![ext()]));
    let result = task.run(&ctx).unwrap();
    assert!(
        matches!(result, TaskResult::Skipped(ref s) if s.contains("VS Code CLI not found")),
        "expected 'VS Code CLI not found' skip, got {result:?}"
    );
}
