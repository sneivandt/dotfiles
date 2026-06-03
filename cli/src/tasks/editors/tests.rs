//! Unit tests for the VS Code extension install task.

use super::*;
use crate::config::vscode_extensions::VsCodeExtension;
use crate::tasks::test_helpers::{empty_config, make_linux_context};
use std::path::PathBuf;

#[test]
fn should_run_false_when_no_extensions_configured() {
    let config = empty_config(PathBuf::from("/tmp"));
    let ctx = make_linux_context(config);
    assert!(!InstallVsCodeExtensions.should_run(&ctx));
}

#[test]
fn should_run_true_when_extensions_configured() {
    let mut config = empty_config(PathBuf::from("/tmp"));
    config.vscode_extensions.push(VsCodeExtension {
        id: "github.copilot".to_string(),
    });
    let ctx = make_linux_context(config);
    assert!(InstallVsCodeExtensions.should_run(&ctx));
}

#[test]
fn run_skips_when_vscode_cli_not_found() {
    let mut config = empty_config(PathBuf::from("/tmp"));
    config.vscode_extensions.push(VsCodeExtension {
        id: "github.copilot".to_string(),
    });
    // Default make_linux_context uses TestExecutor with which_result=false,
    // so find_code_command returns None for both "code-insiders" and "code".
    let ctx = make_linux_context(config);
    let result = InstallVsCodeExtensions.run(&ctx).unwrap();
    assert!(
        matches!(result, TaskResult::Skipped(ref s) if s.contains("VS Code CLI not found")),
        "expected 'VS Code CLI not found' skip, got {result:?}"
    );
}
