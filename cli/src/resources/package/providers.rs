//! Concrete package manager providers.

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

fn split_padded_columns(line: &str) -> Vec<String> {
    let mut cols = Vec::new();
    let mut current = String::new();
    let mut spaces = 0usize;

    for ch in line.chars() {
        if ch == ' ' {
            spaces += 1;
            if spaces < 2 {
                continue;
            }

            if !current.is_empty() {
                cols.push(std::mem::take(&mut current));
            }
        } else {
            if spaces == 1 {
                current.push(' ');
            }
            spaces = 0;
            current.push(ch);
        }
    }

    if !current.is_empty() {
        cols.push(current);
    }

    cols
}

fn byte_offset_of_col(line: &str, col_idx: usize) -> Option<usize> {
    let mut col = 0usize;
    let mut in_col = false;
    let mut spaces = 0usize;

    for (i, ch) in line.char_indices() {
        if ch == ' ' {
            spaces += 1;
            if in_col && spaces >= 2 {
                col += 1;
                in_col = false;
            }
        } else {
            spaces = 0;
            if !in_col {
                if col == col_idx {
                    return Some(i);
                }
                in_col = true;
            }
        }
    }
    None
}

pub(super) fn parse_winget_ids(stdout: &str) -> HashSet<String> {
    let mut ids = HashSet::new();
    let mut id_col_idx: Option<usize> = None;

    for line in stdout.lines() {
        if let Some(col_idx) = id_col_idx {
            if line.bytes().all(|b| b == b'-' || b == b' ') {
                continue;
            }
            if let Some(start) = byte_offset_of_col(line, col_idx)
                && let Some(slice) = line.get(start..)
            {
                let id = slice
                    .find("  ")
                    .map_or_else(|| slice.trim(), |sep_pos| slice[..sep_pos].trim());
                if !id.is_empty() {
                    ids.insert(id.to_string());
                }
            }
        } else {
            let cols = split_padded_columns(line);
            if let Some(id_idx) = cols.iter().position(|c| c == "Id") {
                id_col_idx = Some(id_idx);
            }
        }
    }

    ids
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PacmanProvider;

impl PackageProvider for PacmanProvider {
    fn name(&self) -> &'static str {
        "pacman"
    }

    fn query_installed(&self, executor: &dyn Executor) -> Result<HashSet<String>> {
        query_names(executor, "pacman", &["-Q"], ParseMode::FirstToken)
    }

    fn is_installed(&self, name: &str, executor: &dyn Executor) -> Result<bool> {
        let result = executor.run_unchecked("pacman", &["-Q", name])?;
        Ok(result.success)
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

#[derive(Debug, Clone, Copy)]
pub(super) struct ParuProvider;

impl PackageProvider for ParuProvider {
    fn name(&self) -> &'static str {
        "paru"
    }

    fn query_installed(&self, executor: &dyn Executor) -> Result<HashSet<String>> {
        PacmanProvider.query_installed(executor)
    }

    fn is_installed(&self, name: &str, executor: &dyn Executor) -> Result<bool> {
        PacmanProvider.is_installed(name, executor)
    }

    fn install(&self, name: &str, executor: &dyn Executor) -> Result<ResourceChange> {
        executor.run("paru", &["-S", "--needed", "--noconfirm", name])?;
        Ok(ResourceChange::Applied)
    }

    fn supports_batch(&self) -> bool {
        true
    }

    fn batch_install(&self, names: &[&str], executor: &dyn Executor) -> Result<()> {
        let mut args = vec!["-S", "--needed", "--noconfirm"];
        args.extend(names);
        executor.run("paru", &args)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct WingetProvider;

impl PackageProvider for WingetProvider {
    fn name(&self) -> &'static str {
        "winget"
    }

    fn query_installed(&self, executor: &dyn Executor) -> Result<HashSet<String>> {
        let result = executor.run_unchecked(
            "winget",
            &[
                "list",
                "--accept-source-agreements",
                "--disable-interactivity",
            ],
        )?;

        if !result.success {
            anyhow::bail!(
                "winget list failed (exit {:?}): {}",
                result.code,
                result.stderr.trim()
            );
        }

        Ok(parse_winget_ids(&result.stdout))
    }

    fn is_installed(&self, name: &str, executor: &dyn Executor) -> Result<bool> {
        let result = executor.run_unchecked(
            "winget",
            &[
                "list",
                "--id",
                name,
                "--exact",
                "--accept-source-agreements",
            ],
        )?;
        Ok(result.success && result.stdout.contains(name))
    }

    fn install(&self, name: &str, executor: &dyn Executor) -> Result<ResourceChange> {
        let result = executor.run_unchecked(
            "winget",
            &[
                "install",
                "--id",
                name,
                "--exact",
                "--source",
                "winget",
                "--accept-source-agreements",
                "--accept-package-agreements",
            ],
        )?;
        if result.success {
            Ok(ResourceChange::Applied)
        } else {
            let detail = if result.stderr.trim().is_empty() {
                result.stdout.trim().to_string()
            } else {
                format!("{}\n{}", result.stdout.trim(), result.stderr.trim())
            };
            Ok(ResourceChange::Skipped {
                reason: format!("winget install failed: {detail}"),
            })
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
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
    fn split_padded_columns_preserves_single_spaces_inside_columns() {
        assert_eq!(
            split_padded_columns("Windows Terminal  Microsoft.WindowsTerminal  1.22.0"),
            vec!["Windows Terminal", "Microsoft.WindowsTerminal", "1.22.0"],
        );
    }

    #[test]
    fn split_padded_columns_collapses_wide_separators() {
        assert_eq!(
            split_padded_columns("Name        Id                         Version"),
            vec!["Name", "Id", "Version"],
        );
    }

    #[test]
    fn byte_offset_of_col_finds_columns_after_unicode_names() {
        let line = "中文名称 App                   Unicode.App                  1.0.0";

        assert_eq!(byte_offset_of_col(line, 0), Some(0));
        assert_eq!(
            byte_offset_of_col(line, 1),
            Some("中文名称 App                   ".len())
        );
        assert_eq!(
            line.get(byte_offset_of_col(line, 1).unwrap()..)
                .unwrap()
                .split_whitespace()
                .next(),
            Some("Unicode.App"),
        );
    }

    #[test]
    fn byte_offset_of_col_returns_none_for_missing_column() {
        assert_eq!(byte_offset_of_col("Name  Id", 3), None);
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

    #[test]
    fn parse_winget_ids_returns_empty_without_id_header() {
        let ids = parse_winget_ids("Name  Version\nGit   2.51.0\n");

        assert!(ids.is_empty());
    }

    #[test]
    fn parse_winget_ids_ignores_blank_and_separator_rows() {
        let ids = parse_winget_ids(concat!(
            "Name             Id        Version\n",
            "----------------------------------\n",
            "\n",
            "                                  \n",
            "Git              Git.Git   2.51.0\n",
        ));

        assert_eq!(ids.len(), 1);
        assert!(ids.contains("Git.Git"));
    }

    #[test]
    fn winget_query_installed_uses_expected_noninteractive_flags() {
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked()
            .once()
            .withf(|program, args| {
                program == "winget"
                    && args
                        == [
                            "list",
                            "--accept-source-agreements",
                            "--disable-interactivity",
                        ]
            })
            .returning(|_, _| Ok(ok_result("Name  Id       Version\nGit   Git.Git  2.51.0\n")));

        let ids = WingetProvider.query_installed(&mock).unwrap();

        assert_eq!(ids.len(), 1);
        assert!(ids.contains("Git.Git"));
    }

    #[test]
    fn winget_install_failure_prefers_stdout_when_stderr_is_empty() {
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked()
            .once()
            .withf(|program, args| {
                program == "winget"
                    && args
                        == [
                            "install",
                            "--id",
                            "Git.Git",
                            "--exact",
                            "--source",
                            "winget",
                            "--accept-source-agreements",
                            "--accept-package-agreements",
                        ]
            })
            .returning(|_, _| Ok(failed_result("No package found", "", 1)));

        let change = WingetProvider.install("Git.Git", &mock).unwrap();

        assert_eq!(
            change,
            ResourceChange::Skipped {
                reason: "winget install failed: No package found".to_string(),
            },
        );
    }

    #[test]
    fn winget_install_failure_includes_stdout_and_stderr_when_both_present() {
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked()
            .once()
            .returning(|_, _| Ok(failed_result("Installer output", "Access denied", 1)));

        let change = WingetProvider.install("Git.Git", &mock).unwrap();

        assert_eq!(
            change,
            ResourceChange::Skipped {
                reason: "winget install failed: Installer output\nAccess denied".to_string(),
            },
        );
    }
}
