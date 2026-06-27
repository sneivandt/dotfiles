//! Pacman package provider.

use std::collections::HashSet;

use anyhow::Result;

use super::PackageProvider;
use crate::exec::Executor;
use crate::resources::ResourceChange;

#[derive(Clone, Copy)]
enum ParseMode {
    FirstToken,
}

fn query_names(
    executor: &dyn Executor,
    cmd: &str,
    args: &[&str],
    mode: ParseMode,
) -> Result<HashSet<String>> {
    let result = executor.run_unchecked(cmd, args)?;
    if !result.success {
        anyhow::bail!(
            "{cmd} query failed (exit {:?}): {}",
            result.code,
            result.stderr.trim()
        );
    }
    let mut set = HashSet::new();
    for line in result.stdout.lines() {
        match mode {
            ParseMode::FirstToken => {
                if let Some(name) = line.split_whitespace().next() {
                    set.insert(name.to_string());
                }
            }
        }
    }
    Ok(set)
}

/// Pacman provider for official Arch Linux packages.
#[derive(Debug, Clone, Copy)]
pub(super) struct PacmanProvider;

impl PackageProvider for PacmanProvider {
    fn name(&self) -> &'static str {
        "pacman"
    }

    fn query_installed(&self, executor: &dyn Executor) -> Result<HashSet<String>> {
        query_names(executor, "pacman", &["-Q"], ParseMode::FirstToken)
    }

    fn install(&self, name: &str, executor: &dyn Executor) -> Result<ResourceChange> {
        executor.run("sudo", &["pacman", "-Syu", "--needed", "--noconfirm", name])?;
        Ok(ResourceChange::Applied)
    }

    fn supports_batch(&self) -> bool {
        true
    }

    fn batch_install(&self, names: &[&str], executor: &dyn Executor) -> Result<()> {
        let mut args = vec!["pacman", "-Syu", "--needed", "--noconfirm"];
        args.extend(names);
        executor.run("sudo", &args)?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code uses panicking helpers")]
mod tests {
    use super::*;
    use crate::exec::{ExecResult, MockExecutor};

    fn ok_result(stdout: &str) -> ExecResult {
        ExecResult {
            stdout: stdout.to_string(),
            stderr: String::new(),
            success: true,
            code: Some(0),
        }
    }

    fn failed_result(stdout: &str, stderr: &str, code: i32) -> ExecResult {
        ExecResult {
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            success: false,
            code: Some(code),
        }
    }

    #[test]
    fn query_names_extracts_first_tokens_and_ignores_blank_lines() {
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked()
            .once()
            .withf(|program, args| program == "pacman" && args == ["-Q"])
            .returning(|_, _| Ok(ok_result("git 2.51.0\n\nbase-devel 1-2\nvim\n")));

        let names = query_names(&mock, "pacman", &["-Q"], ParseMode::FirstToken).unwrap();

        assert_eq!(names.len(), 3);
        assert!(names.contains("git"));
        assert!(names.contains("base-devel"));
        assert!(names.contains("vim"));
        assert!(!names.contains("2.51.0"));
    }

    #[test]
    fn query_names_includes_exit_code_and_stderr_on_failure() {
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked()
            .once()
            .returning(|_, _| Ok(failed_result("", "database lock held", 42)));

        let err = query_names(&mock, "pacman", &["-Q"], ParseMode::FirstToken).unwrap_err();
        let message = err.to_string();

        assert!(
            message.contains("pacman query failed"),
            "message: {message}"
        );
        assert!(message.contains("Some(42)"), "message: {message}");
        assert!(message.contains("database lock held"), "message: {message}");
    }
}
