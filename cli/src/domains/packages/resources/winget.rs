//! Winget package provider.

use std::collections::HashSet;

use anyhow::Result;

use super::package::PackageProvider;
use crate::engine::ResourceChange;
use crate::infra::exec::Executor;

/// Parses the column header row of `winget list` output into a list of
/// `(display-column, name)` pairs, one per column.
///
/// winget left-aligns each value at the same *display* column as its header.
/// Column starts are therefore measured in terminal cells, not bytes or
/// `char`s, so that wide characters (e.g. CJK names, which occupy two cells)
/// and multibyte-but-narrow characters (e.g. an en-dash) both keep later
/// columns aligned. Column names are detected as runs of non-space characters.
fn header_columns(line: &str) -> Vec<(usize, String)> {
    let mut cols = Vec::new();
    let mut col = 0usize;
    let mut start: Option<usize> = None;
    let mut current = String::new();

    for ch in line.chars() {
        if ch == ' ' {
            if let Some(s) = start.take() {
                cols.push((s, std::mem::take(&mut current)));
            }
        } else {
            if start.is_none() {
                start = Some(col);
            }
            current.push(ch);
        }
        col = col.saturating_add(char_display_width(ch));
    }

    if let Some(s) = start {
        cols.push((s, current));
    }

    cols
}

/// Returns the display width of `ch` in terminal cells, treating control and
/// zero-width characters as width zero.
fn char_display_width(ch: char) -> usize {
    unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0)
}

/// Returns the substring of `line` spanning the half-open *display-column*
/// range `[start, end)`. Walking by display width keeps the slice aligned with
/// winget's column layout regardless of the byte or `char` length of preceding
/// values.
fn slice_by_display_range(line: &str, start: usize, end: usize) -> String {
    let mut col = 0usize;
    let mut out = String::new();

    for ch in line.chars() {
        if col >= end {
            break;
        }
        if col >= start {
            out.push(ch);
        }
        col = col.saturating_add(char_display_width(ch));
    }

    out
}

pub(super) fn parse_winget_ids(stdout: &str) -> HashSet<String> {
    let mut ids = HashSet::new();
    let mut id_range: Option<(usize, usize)> = None;

    for line in stdout.lines() {
        if let Some((id_start, next_start)) = id_range {
            if line.bytes().all(|b| b == b'-' || b == b' ') {
                continue;
            }
            let cell = slice_by_display_range(line, id_start, next_start);
            let id = cell.trim();
            // Package IDs never contain spaces; this also filters stray
            // sentence fragments that may precede the table.
            if !id.is_empty() && !id.contains(' ') {
                ids.insert(id.to_string());
            }
        } else {
            let cols = header_columns(line);
            if let Some(pos) = cols.iter().position(|(_, name)| name == "Id")
                && let Some(&(id_start, _)) = cols.get(pos)
            {
                let next_start = cols
                    .get(pos.saturating_add(1))
                    .map_or(usize::MAX, |&(s, _)| s);
                id_range = Some((id_start, next_start));
            }
        }
    }

    ids
}

/// Winget exit codes this provider reacts to.
///
/// winget reports failures as `HRESULT`s, which arrive here as negative `i32`s.
/// Only the codes that change our behaviour are named; everything else falls
/// through to the generic failure path.
mod exit_code {
    /// `APPINSTALLER_CLI_ERROR_NO_APPLICABLE_INSTALLER` (`0x8A15_0010`).
    ///
    /// The package has no installer matching the requested scope, so a
    /// `--scope user` attempt should be retried without the scope filter.
    pub(super) const NO_APPLICABLE_INSTALLER: i32 = -1_978_335_216;

    /// `APPINSTALLER_CLI_ERROR_COMMAND_REQUIRES_ADMIN` (`0x8A15_0019`).
    pub(super) const COMMAND_REQUIRES_ADMIN: i32 = -1_978_335_207;

    /// `APPINSTALLER_CLI_ERROR_INSTALL_CANCELLED_BY_USER` (`0x8A15_010C`).
    pub(super) const INSTALL_CANCELLED_BY_USER: i32 = -1_978_334_964;

    /// `APPINSTALLER_CLI_ERROR_INSTALL_BLOCKED_BY_POLICY` (`0x8A15_010F`).
    pub(super) const INSTALL_BLOCKED_BY_POLICY: i32 = -1_978_334_961;

    /// `APPINSTALLER_CLI_ERROR_PACKAGE_ALREADY_INSTALLED` (`0x8A15_0061`).
    pub(super) const PACKAGE_ALREADY_INSTALLED: i32 = -1_978_335_135;

    /// `APPINSTALLER_CLI_ERROR_INSTALL_ALREADY_INSTALLED` (`0x8A15_010D`).
    pub(super) const INSTALL_ALREADY_INSTALLED: i32 = -1_978_334_963;

    /// `APPINSTALLER_CLI_ERROR_INSTALL_REBOOT_REQUIRED_TO_FINISH` (`0x8A15_0109`).
    pub(super) const INSTALL_REBOOT_REQUIRED_TO_FINISH: i32 = -1_978_334_967;
}

/// How the package task should treat a winget exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallVerdict {
    /// The package is installed; nothing more to do.
    Installed,
    /// The requested scope has no installer; retry without `--scope`.
    RetryWithoutScope,
    /// Expected on an unelevated run; report it rather than failing the task.
    Skipped(&'static str),
    /// Anything else, reported with winget's own output.
    Failed,
}

/// Classify a winget exit code into the action the caller should take.
///
/// Recognising "already installed" as success keeps installs idempotent when
/// `winget list` under-reports a package, and recognising the privilege codes
/// lets an unelevated run degrade to a skip instead of a hard failure.
const fn classify_install(code: Option<i32>) -> InstallVerdict {
    match code {
        Some(exit_code::NO_APPLICABLE_INSTALLER) => InstallVerdict::RetryWithoutScope,
        Some(
            exit_code::PACKAGE_ALREADY_INSTALLED
            | exit_code::INSTALL_ALREADY_INSTALLED
            | exit_code::INSTALL_REBOOT_REQUIRED_TO_FINISH,
        ) => InstallVerdict::Installed,
        Some(exit_code::COMMAND_REQUIRES_ADMIN) => InstallVerdict::Skipped(
            "requires administrator; re-run `dotfiles install --only packages` from an elevated shell",
        ),
        Some(exit_code::INSTALL_CANCELLED_BY_USER) => {
            InstallVerdict::Skipped("installer elevation was declined")
        }
        Some(exit_code::INSTALL_BLOCKED_BY_POLICY) => {
            InstallVerdict::Skipped("blocked by system policy")
        }
        _ => InstallVerdict::Failed,
    }
}

/// Build the `winget install` argument list for `name`.
///
/// `--disable-interactivity` suppresses winget's own prompts so a run cannot
/// stall on a hidden question; it does not suppress an installer's UAC prompt,
/// which Windows shows on the secure desktop.
fn install_args<'a>(name: &'a str, scope: Option<&'a str>) -> Vec<&'a str> {
    let mut args = vec![
        "install",
        "--id",
        name,
        "--exact",
        "--source",
        "winget",
        "--accept-source-agreements",
        "--accept-package-agreements",
        "--disable-interactivity",
    ];
    if let Some(scope) = scope {
        args.push("--scope");
        args.push(scope);
    }
    args
}

/// Winget provider for Windows packages.
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

    /// Install `name`, preferring a per-user installer.
    ///
    /// Trying `--scope user` first means the common case installs without any
    /// UAC prompt at all. Packages that only ship a machine-scope installer
    /// report `NO_APPLICABLE_INSTALLER`, and are retried unscoped so winget can
    /// raise its own elevation prompt for just that package.
    fn install(&self, name: &str, executor: &dyn Executor) -> Result<ResourceChange> {
        let mut result = executor.run_unchecked("winget", &install_args(name, Some("user")))?;

        if !result.success && classify_install(result.code) == InstallVerdict::RetryWithoutScope {
            result = executor.run_unchecked("winget", &install_args(name, None))?;
        }

        if result.success {
            return Ok(ResourceChange::Applied);
        }

        match classify_install(result.code) {
            InstallVerdict::Installed => Ok(ResourceChange::Applied),
            InstallVerdict::Skipped(reason) => Ok(ResourceChange::Skipped {
                reason: format!("{name}: {reason}"),
            }),
            InstallVerdict::RetryWithoutScope | InstallVerdict::Failed => {
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
    use crate::infra::exec::{ExecResult, MockExecutor};

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
    fn parse_winget_ids_handles_truncated_name_with_single_space_separator() {
        // A 28-char name padded to width 29 leaves exactly ONE trailing space
        // before the Id, reproducing the winget truncation that broke the old
        // 2+-space column splitter.
        let header = format!("{:<29}{:<37}{}", "Name", "Id", "Version");
        let row = format!(
            "{:<29}{:<37}{}",
            "Visual Studio Code Insiders…", "Microsoft.VisualStudioCode.Insiders", "1.125.0"
        );
        let stdout = format!("{header}\n{}\n{row}\n", "-".repeat(73));

        let ids = parse_winget_ids(&stdout);

        assert!(ids.contains("Microsoft.VisualStudioCode.Insiders"));
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn parse_winget_ids_handles_en_dash_name() {
        // The en-dash is one char but three bytes; display-range slicing must
        // keep the Id column aligned.
        let header = format!("{:<44}{:<20}{}", "Name", "Id", "Version");
        let row = format!(
            "{:<44}{:<20}{}",
            "APM – Agent Package Manager", "Microsoft.APM", "1.0.0"
        );
        let stdout = format!("{header}\n{}\n{row}\n", "-".repeat(70));

        let ids = parse_winget_ids(&stdout);

        assert!(ids.contains("Microsoft.APM"));
        assert_eq!(ids.len(), 1);
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
                            "--disable-interactivity",
                            "--scope",
                            "user",
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

    #[test]
    fn winget_install_prefers_user_scope() {
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked()
            .once()
            .withf(|_, args| args.contains(&"--scope") && args.contains(&"user"))
            .returning(|_, _| Ok(ok_result("")));

        assert_eq!(
            WingetProvider.install("Git.Git", &mock).unwrap(),
            ResourceChange::Applied,
        );
    }

    #[test]
    fn winget_install_retries_without_scope_when_no_user_installer_exists() {
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked()
            .once()
            .withf(|_, args| args.contains(&"--scope"))
            .returning(|_, _| {
                Ok(failed_result(
                    "",
                    "no applicable installer",
                    exit_code::NO_APPLICABLE_INSTALLER,
                ))
            });
        mock.expect_run_unchecked()
            .once()
            .withf(|_, args| !args.contains(&"--scope"))
            .returning(|_, _| Ok(ok_result("")));

        assert_eq!(
            WingetProvider.install("Git.Git", &mock).unwrap(),
            ResourceChange::Applied,
        );
    }

    #[test]
    fn winget_install_skips_rather_than_fails_when_elevation_is_required() {
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked().times(2).returning(|_, args| {
            if args.contains(&"--scope") {
                Ok(failed_result("", "", exit_code::NO_APPLICABLE_INSTALLER))
            } else {
                Ok(failed_result("", "", exit_code::COMMAND_REQUIRES_ADMIN))
            }
        });

        let ResourceChange::Skipped { reason } = WingetProvider.install("Git.Git", &mock).unwrap()
        else {
            panic!("expected a skip");
        };
        assert!(
            reason.starts_with("Git.Git: requires administrator"),
            "{reason}"
        );
        assert!(reason.contains("--only packages"), "{reason}");
    }

    #[test]
    fn winget_install_treats_already_installed_as_success() {
        for code in [
            exit_code::PACKAGE_ALREADY_INSTALLED,
            exit_code::INSTALL_ALREADY_INSTALLED,
            exit_code::INSTALL_REBOOT_REQUIRED_TO_FINISH,
        ] {
            let mut mock = MockExecutor::new();
            mock.expect_run_unchecked()
                .once()
                .returning(move |_, _| Ok(failed_result("", "", code)));

            assert_eq!(
                WingetProvider.install("Git.Git", &mock).unwrap(),
                ResourceChange::Applied,
                "exit code {code} should count as installed",
            );
        }
    }

    #[test]
    fn winget_install_skips_when_the_user_declines_the_installer_prompt() {
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked()
            .once()
            .returning(|_, _| Ok(failed_result("", "", exit_code::INSTALL_CANCELLED_BY_USER)));

        let ResourceChange::Skipped { reason } = WingetProvider.install("Git.Git", &mock).unwrap()
        else {
            panic!("expected a skip");
        };
        assert!(reason.contains("elevation was declined"), "{reason}");
    }

    #[test]
    fn winget_install_args_omit_scope_when_unscoped() {
        assert!(!install_args("Git.Git", None).contains(&"--scope"));
        assert!(install_args("Git.Git", None).contains(&"--disable-interactivity"));
    }
}
