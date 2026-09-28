//! VS Code extension resource.
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;

use crate::engine::{Resource, ResourceChange, ResourceResult, ResourceState};
#[cfg(not(target_os = "windows"))]
use crate::infra::exec::CommandSpec;
use crate::infra::exec::{self, Executor};

#[cfg(target_os = "windows")]
const CODE_COMMANDS: [&str; 2] = ["code-insiders.cmd", "code.cmd"];
#[cfg(not(target_os = "windows"))]
const CODE_COMMANDS: [&str; 2] = ["code-insiders", "code"];

/// A VS Code extension resource that can be checked and installed.
#[derive(Debug)]
pub struct VsCodeExtensionResource {
    /// Extension identifier (e.g. "github.copilot-chat").
    pub id: String,
    /// VS Code CLI command to use (e.g. "code-insiders" or "code").
    pub code_cmd: String,
    /// Executor for running VS Code CLI commands.
    executor: Arc<dyn Executor>,
}

impl VsCodeExtensionResource {
    /// Create a new VS Code extension resource.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        code_cmd: impl Into<String>,
        executor: Arc<dyn Executor>,
    ) -> Self {
        Self {
            id: id.into(),
            code_cmd: code_cmd.into(),
            executor,
        }
    }

    /// Determine the resource state from a pre-fetched set of installed extension IDs.
    ///
    /// This avoids running `code --list-extensions` per resource when used
    /// with [`get_installed_extensions`].
    #[must_use]
    pub fn state_from_installed(&self, installed: &HashSet<String>) -> ResourceState {
        if installed.contains(&self.id.to_lowercase()) {
            ResourceState::Correct
        } else {
            ResourceState::Missing
        }
    }
}

/// Query the full set of installed VS Code extension IDs in a single command.
///
/// Returns a `HashSet` of **lower-cased** extension IDs.
///
/// # Errors
///
/// Returns an error if the VS Code command fails to execute, cannot be found,
/// or exits with a non-zero status code.
pub fn get_installed_extensions(
    code_cmd: &str,
    executor: &dyn Executor,
) -> Result<HashSet<String>> {
    let result = run_code_cmd(code_cmd, &["--list-extensions"], executor)?;
    if !result.success {
        anyhow::bail!(
            "code --list-extensions failed (exit {:?}): {}",
            result.code,
            result.stderr.trim()
        );
    }
    Ok(result
        .stdout
        .lines()
        .map(|line| line.trim().to_lowercase())
        .filter(|id| !id.is_empty())
        .collect())
}

/// `VsCodeExtensionResource` intentionally relies on an external state
/// provider instead of implementing intrinsic state checks. Its state depends
/// on a single `code --list-extensions` bulk query that is prohibitively
/// expensive to repeat for each extension individually. Callers must use
/// [`get_installed_extensions`] once and then [`Self::state_from_installed`]
/// per resource; the task (`InstallVsCodeExtensions`) already does this.
impl Resource for VsCodeExtensionResource {
    fn description(&self) -> String {
        self.id.clone()
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        let result = run_code_cmd(
            &self.code_cmd,
            &["--install-extension", &self.id, "--force"],
            &*self.executor,
        )?;
        if result.success {
            Ok(ResourceChange::Applied)
        } else {
            let exit_status = result
                .code
                .map_or_else(|| "unknown".to_string(), |code| code.to_string());
            let stdout = nonempty_output(&result.stdout);
            let stderr = nonempty_output(&result.stderr);
            Ok(ResourceChange::unusable(format!(
                "{} failed to install {} (exit {exit_status}); stdout: {stdout}; stderr: {stderr}",
                self.code_cmd, self.id
            )))
        }
    }
}

fn nonempty_output(output: &str) -> &str {
    let output = output.trim();
    if output.is_empty() { "<empty>" } else { output }
}

/// Find the VS Code CLI command, preferring Code Insiders.
///
/// Windows installations include both an extensionless POSIX shell script and
/// a `.cmd` launcher in the same directory. The Windows command must include
/// the extension and be resolved to an absolute path because the launcher uses
/// `%~dp0` to locate the adjacent VS Code executable.
#[must_use]
pub fn find_code_command(executor: &dyn Executor) -> Option<String> {
    CODE_COMMANDS
        .iter()
        .find_map(|cmd| executor.which_path(cmd).ok())
        .map(|path| path.to_string_lossy().into_owned())
}

/// Run a VS Code CLI command. On Windows, `.cmd` wrappers need `cmd.exe /C`.
///
/// # Errors
///
/// Returns an error if the command execution fails or if the command cannot be found.
fn run_code_cmd(cmd: &str, args: &[&str], executor: &dyn Executor) -> Result<exec::ExecResult> {
    #[cfg(target_os = "windows")]
    {
        exec::windows::CmdCommand::new(cmd)
            .args(args)
            .run_unchecked(executor)
    }

    #[cfg(not(target_os = "windows"))]
    {
        Ok(executor.execute(CommandSpec::new(cmd).args(args).unchecked())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::resource::SkipKind;
    use crate::infra::exec::{ExecError, ExecResult, MockExecutor};

    fn expect_code_command(
        mock: &mut MockExecutor,
        args: &'static [&'static str],
        result: std::result::Result<ExecResult, ExecError>,
    ) {
        #[cfg(target_os = "windows")]
        {
            let command_line = format!(
                "\"\"code\" {}\"",
                args.iter()
                    .map(|arg| format!("\"{arg}\""))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            mock.expect_execute()
                .once()
                .withf(move |spec| {
                    spec.windows_command_line() == Some(command_line.as_str())
                        && !spec.is_checked()
                        && spec.working_dir().is_none()
                })
                .return_once(|_| result);
        }

        #[cfg(not(target_os = "windows"))]
        mock.expect_execute()
            .once()
            .withf(move |spec| {
                spec.program() == "code"
                    && spec.arguments() == args
                    && !spec.is_checked()
                    && spec.working_dir().is_none()
            })
            .return_once(|_| result);
    }

    fn expect_list_extensions(mock: &mut MockExecutor, result: ExecResult) {
        expect_code_command(mock, &["--list-extensions"], Ok(result));
    }

    fn expect_install_extension(mock: &mut MockExecutor, result: ExecResult) {
        expect_code_command(
            mock,
            &["--install-extension", "ms-python.python", "--force"],
            Ok(result),
        );
    }

    #[test]
    fn find_code_command_prefers_platform_specific_insiders_launcher() {
        let mut mock = MockExecutor::new();
        let expected = std::path::PathBuf::from(CODE_COMMANDS[0]);
        mock.expect_which_path()
            .once()
            .with(mockall::predicate::eq(CODE_COMMANDS[0]))
            .return_once({
                let expected = expected.clone();
                |_| Ok(expected)
            });

        assert_eq!(
            find_code_command(&mock).as_deref(),
            expected.to_str(),
            "the platform-specific Code Insiders launcher should be preferred"
        );
    }

    #[test]
    fn find_code_command_falls_back_to_platform_specific_stable_launcher() {
        let mut sequence = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        let expected = std::path::PathBuf::from(CODE_COMMANDS[1]);
        mock.expect_which_path()
            .once()
            .with(mockall::predicate::eq(CODE_COMMANDS[0]))
            .in_sequence(&mut sequence)
            .returning(|cmd| anyhow::bail!("{cmd} not found"));
        mock.expect_which_path()
            .once()
            .with(mockall::predicate::eq(CODE_COMMANDS[1]))
            .in_sequence(&mut sequence)
            .return_once({
                let expected = expected.clone();
                |_| Ok(expected)
            });

        assert_eq!(
            find_code_command(&mock).as_deref(),
            expected.to_str(),
            "the platform-specific stable launcher should be used as fallback"
        );
    }

    #[test]
    fn state_from_installed_matches_ids_case_insensitively() {
        for (id, installed, expected) in [
            ("github.copilot-chat", true, ResourceState::Correct),
            ("GitHub.Copilot-Chat", true, ResourceState::Correct),
            ("github.copilot-chat", false, ResourceState::Missing),
        ] {
            let resource = VsCodeExtensionResource::new(id, "code", Arc::new(MockExecutor::new()));
            let installed_ids = if installed {
                HashSet::from(["github.copilot-chat".to_string()])
            } else {
                HashSet::new()
            };
            assert_eq!(
                resource.state_from_installed(&installed_ids),
                expected,
                "{id}, installed={installed}"
            );
        }
    }

    #[test]
    fn apply_failure_preserves_cli_diagnostics() {
        let mut mock = MockExecutor::new();
        expect_install_extension(
            &mut mock,
            ExecResult::failure(
                "Installing extensions...",
                "Signature verification failed",
                Some(1),
            ),
        );
        let resource = VsCodeExtensionResource::new("ms-python.python", "code", Arc::new(mock));

        let change = resource.apply().unwrap();

        assert_eq!(
            change,
            ResourceChange::Skipped {
                reason: "code failed to install ms-python.python (exit 1); stdout: Installing extensions...; stderr: Signature verification failed".to_string(),
                kind: SkipKind::UnmetWork,
            }
        );
    }

    #[test]
    fn apply_success_and_empty_failure_output_are_distinguished() {
        for (result, expected) in [
            (ExecResult::success("installed"), ResourceChange::Applied),
            (
                ExecResult::failure(" \n", "\n ", None),
                ResourceChange::unusable(
                    "code failed to install ms-python.python (exit unknown); stdout: <empty>; stderr: <empty>",
                ),
            ),
        ] {
            let mut mock = MockExecutor::new();
            expect_install_extension(&mut mock, result);
            let resource = VsCodeExtensionResource::new("ms-python.python", "code", Arc::new(mock));

            assert_eq!(resource.apply().unwrap(), expected);
        }
    }

    #[test]
    fn spawn_failure_is_not_reported_as_an_extension_skip_or_empty_inventory() {
        for install in [false, true] {
            let mut mock = MockExecutor::new();
            let args: &'static [&'static str] = if install {
                &["--install-extension", "ms-python.python", "--force"]
            } else {
                &["--list-extensions"]
            };
            expect_code_command(
                &mut mock,
                args,
                Err(ExecError::spawn(
                    "code",
                    std::io::Error::other("launcher unavailable"),
                )),
            );
            let error = if install {
                VsCodeExtensionResource::new("ms-python.python", "code", Arc::new(mock))
                    .apply()
                    .unwrap_err()
                    .to_string()
            } else {
                get_installed_extensions("code", &mock)
                    .unwrap_err()
                    .to_string()
            };
            assert!(error.contains("launcher unavailable"), "{error}");
        }
    }

    // ------------------------------------------------------------------
    // get_installed_extensions
    // ------------------------------------------------------------------

    #[test]
    fn get_installed_extensions_parses_and_lowercases() {
        let mut mock = MockExecutor::new();
        expect_list_extensions(
            &mut mock,
            ExecResult::success(
                " GitHub.Copilot \r\n\nms-python.python\nRust-lang.Rust-analyzer\nGITHUB.COPILOT\n \t\n",
            ),
        );
        let installed = get_installed_extensions("code", &mock).unwrap();
        assert_eq!(
            installed,
            HashSet::from([
                "github.copilot".to_string(),
                "ms-python.python".to_string(),
                "rust-lang.rust-analyzer".to_string(),
            ])
        );
    }

    #[test]
    fn get_installed_extensions_returns_error_when_command_fails() {
        let mut mock = MockExecutor::new();
        expect_list_extensions(
            &mut mock,
            ExecResult::failure("partial.extension", " inventory failed \n", Some(2)),
        );
        let error = get_installed_extensions("code", &mock)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("exit Some(2)") && error.contains("inventory failed"),
            "{error}"
        );
    }
}
