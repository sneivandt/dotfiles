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
    let mut set = HashSet::new();
    for line in result.stdout.lines() {
        let id = line.trim().to_lowercase();
        if !id.is_empty() {
            set.insert(id);
        }
    }
    Ok(set)
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
            Ok(ResourceChange::unusable(format!(
                "failed to install: {}",
                result.stderr.trim()
            )))
        }
    }
}

/// Find the VS Code CLI command, preferring Code Insiders.
///
/// Windows installations include both an extensionless POSIX shell script and
/// a `.cmd` launcher in the same directory. The Windows command must include
/// the extension and be resolved to an absolute path because the launcher uses
/// `%~dp0` to locate the adjacent VS Code executable.
#[must_use]
pub fn find_code_command(executor: &dyn Executor) -> Option<String> {
    for cmd in CODE_COMMANDS {
        if let Ok(path) = executor.which_path(cmd) {
            return Some(path.to_string_lossy().into_owned());
        }
    }
    None
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
    use crate::infra::exec::{ExecResult, MockExecutor};

    fn expect_list_extensions(mock: &mut MockExecutor, result: ExecResult) {
        #[cfg(target_os = "windows")]
        mock.expect_execute()
            .once()
            .withf(|spec| spec.windows_command_line() == Some(r#"""code" "--list-extensions"""#))
            .return_once(|_| Ok(result));

        #[cfg(not(target_os = "windows"))]
        mock.expect_execute()
            .once()
            .withf(|spec| {
                spec.program() == "code"
                    && spec.arguments() == ["--list-extensions"]
                    && !spec.is_checked()
            })
            .return_once(|_| Ok(result));
    }

    #[test]
    fn description_returns_extension_id() {
        let executor: Arc<dyn Executor> = Arc::new(exec::ProcessExecutor::system());
        let resource = VsCodeExtensionResource::new(
            "github.copilot-chat".to_string(),
            "code".to_string(),
            Arc::clone(&executor),
        );
        assert_eq!(resource.description(), "github.copilot-chat");
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
    fn state_from_installed_correct() {
        let executor: Arc<dyn Executor> = Arc::new(exec::ProcessExecutor::system());
        let resource = VsCodeExtensionResource::new(
            "github.copilot-chat".to_string(),
            "code".to_string(),
            Arc::clone(&executor),
        );
        let mut installed = HashSet::new();
        installed.insert("github.copilot-chat".to_string());
        assert_eq!(
            resource.state_from_installed(&installed),
            ResourceState::Correct
        );
    }

    #[test]
    fn state_from_installed_case_insensitive() {
        let executor: Arc<dyn Executor> = Arc::new(exec::ProcessExecutor::system());
        let resource = VsCodeExtensionResource::new(
            "GitHub.Copilot-Chat".to_string(),
            "code".to_string(),
            Arc::clone(&executor),
        );
        let mut installed = HashSet::new();
        installed.insert("github.copilot-chat".to_string()); // lowercase in set
        assert_eq!(
            resource.state_from_installed(&installed),
            ResourceState::Correct
        );
    }

    #[test]
    fn state_from_installed_missing() {
        let executor: Arc<dyn Executor> = Arc::new(exec::ProcessExecutor::system());
        let resource = VsCodeExtensionResource::new(
            "github.copilot-chat".to_string(),
            "code".to_string(),
            Arc::clone(&executor),
        );
        let installed = HashSet::new();
        assert_eq!(
            resource.state_from_installed(&installed),
            ResourceState::Missing
        );
    }

    // ------------------------------------------------------------------
    // get_installed_extensions
    // ------------------------------------------------------------------

    #[test]
    fn get_installed_extensions_parses_and_lowercases() {
        let mut mock = MockExecutor::new();
        expect_list_extensions(
            &mut mock,
            ExecResult::success("GitHub.Copilot\nms-python.python\nRust-lang.Rust-analyzer\n"),
        );
        let installed = get_installed_extensions("code", &mock).unwrap();
        assert!(installed.contains("github.copilot"));
        assert!(installed.contains("ms-python.python"));
        assert!(installed.contains("rust-lang.rust-analyzer"));
    }

    #[test]
    fn get_installed_extensions_returns_error_when_command_fails() {
        let mut mock = MockExecutor::new();
        expect_list_extensions(&mut mock, ExecResult::failure("", "", Some(1)));
        let result = get_installed_extensions("code", &mock);
        assert!(
            result.is_err(),
            "should return an error when the command fails"
        );
    }

    #[test]
    fn get_installed_extensions_uses_single_bulk_query() {
        let mut mock = MockExecutor::new();
        expect_list_extensions(&mut mock, ExecResult::success("github.copilot-chat\n"));
        let installed = get_installed_extensions("code", &mock).unwrap();
        assert!(
            installed.contains("github.copilot-chat"),
            "extension should be found"
        );
    }
}
