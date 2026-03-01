//! Package installation resource.
use std::collections::HashSet;

use anyhow::Result;

use super::{Applicable, Resource, ResourceChange, ResourceState};
use crate::exec::Executor;

/// Supported package managers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    /// Official Arch Linux packages (pacman).
    Pacman,
    /// AUR packages (paru).
    Paru,
    /// Windows packages (winget).
    Winget,
}

impl std::fmt::Display for PackageManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pacman => write!(f, "pacman"),
            Self::Paru => write!(f, "paru"),
            Self::Winget => write!(f, "winget"),
        }
    }
}

/// A system package resource that can be checked and installed.
#[derive(Debug)]
pub struct PackageResource<'a> {
    /// Package name (or winget ID).
    pub name: String,
    /// Package manager to use.
    pub manager: PackageManager,
    /// Executor for running package manager commands.
    executor: &'a dyn Executor,
}

impl<'a> PackageResource<'a> {
    /// Create a new package resource.
    #[must_use]
    pub const fn new(name: String, manager: PackageManager, executor: &'a dyn Executor) -> Self {
        Self {
            name,
            manager,
            executor,
        }
    }

    /// Determine the resource state from a pre-fetched set of installed package names.
    ///
    /// This avoids running a per-package query when used with
    /// [`get_installed_packages`].
    #[must_use]
    pub fn state_from_installed(&self, installed: &HashSet<String>) -> ResourceState {
        if installed.contains(&self.name) {
            ResourceState::Correct
        } else {
            ResourceState::Missing
        }
    }
}

/// Query the full set of installed package names for a given manager.
///
/// Returns a `HashSet` of package names (or winget IDs) that are currently
/// installed. This runs a **single** command regardless of how many packages
/// need to be checked — compared to one command per package when using
/// `PackageResource::current_state()` directly.
///
/// # Errors
///
/// Returns an error if the package manager command fails to execute or if
/// the output cannot be parsed.
pub fn get_installed_packages(
    manager: PackageManager,
    executor: &dyn Executor,
) -> Result<HashSet<String>> {
    match manager {
        PackageManager::Pacman | PackageManager::Paru => {
            // `pacman -Q` lists all explicitly & dependency-installed packages,
            // one per line: "name version"
            let result = executor.run_unchecked("pacman", &["-Q"])?;
            let mut set = HashSet::new();
            if result.success {
                for line in result.stdout.lines() {
                    if let Some(name) = line.split_whitespace().next() {
                        set.insert(name.to_string());
                    }
                }
            }
            Ok(set)
        }
        PackageManager::Winget => {
            // `winget list` outputs a formatted table — each line may contain
            // the package ID as a whitespace-delimited token.  Winget IDs are
            // reverse-domain names (e.g. `Git.Git`, `Microsoft.PowerShell`) so
            // collisions with version numbers or other tokens are not a concern
            // when doing exact-match lookups via `state_from_installed`.
            let result = executor.run_unchecked(
                "winget",
                &[
                    "list",
                    "--accept-source-agreements",
                    "--disable-interactivity",
                ],
            )?;
            let mut set = HashSet::new();
            if result.success {
                for line in result.stdout.lines() {
                    for token in line.split_whitespace() {
                        set.insert(token.to_string());
                    }
                }
            }
            Ok(set)
        }
    }
}

/// Install a batch of packages in a single command, grouped by package manager.
///
/// Groups the given resources by their [`PackageManager`] and runs one
/// installation command per group, using each resource's own executor.
/// For Pacman packages the command is
/// `sudo pacman -S --needed --noconfirm <names…>`; for Paru packages it is
/// `paru -S --needed --noconfirm <names…>`.  Winget packages are installed
/// individually (winget does not support multi-package installs in one call);
/// a `Skipped` result from any Winget install is treated as an error.
///
/// # Errors
///
/// Returns an error if any package manager command fails, or if a Winget
/// install is skipped (i.e. the installer reported failure).
pub fn batch_install_packages(resources: &[&PackageResource<'_>]) -> Result<()> {
    if let Some(first) = resources
        .iter()
        .find(|r| r.manager == PackageManager::Pacman)
    {
        let mut args = vec!["pacman", "-S", "--needed", "--noconfirm"];
        args.extend(
            resources
                .iter()
                .filter(|r| r.manager == PackageManager::Pacman)
                .map(|r| r.name.as_str()),
        );
        first.executor.run("sudo", &args)?;
    }

    if let Some(first) = resources.iter().find(|r| r.manager == PackageManager::Paru) {
        let mut args = vec!["-S", "--needed", "--noconfirm"];
        args.extend(
            resources
                .iter()
                .filter(|r| r.manager == PackageManager::Paru)
                .map(|r| r.name.as_str()),
        );
        first.executor.run("paru", &args)?;
    }

    // Winget does not support batch installs; delegate to individual apply()
    // and propagate skipped installations as errors.
    // Note: PackageResource::apply() for Winget only returns Skipped when the
    // process exits non-zero, so Skipped always indicates an install failure.
    for resource in resources
        .iter()
        .filter(|r| r.manager == PackageManager::Winget)
    {
        let change = resource.apply()?;
        if let ResourceChange::Skipped { reason } = change {
            anyhow::bail!("winget install failed for '{}': {reason}", resource.name);
        }
    }

    Ok(())
}

impl Applicable for PackageResource<'_> {
    fn description(&self) -> String {
        format!("{} ({})", self.name, self.manager)
    }

    fn apply(&self) -> Result<ResourceChange> {
        match self.manager {
            PackageManager::Pacman => {
                self.executor.run(
                    "sudo",
                    &["pacman", "-S", "--needed", "--noconfirm", &self.name],
                )?;
                Ok(ResourceChange::Applied)
            }
            PackageManager::Paru => {
                self.executor
                    .run("paru", &["-S", "--needed", "--noconfirm", &self.name])?;
                Ok(ResourceChange::Applied)
            }
            PackageManager::Winget => {
                let result = self.executor.run_unchecked(
                    "winget",
                    &[
                        "install",
                        "--id",
                        &self.name,
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
                    // winget writes most diagnostics to stdout, not stderr.
                    // Combine both streams so the user sees useful output.
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
}

impl Resource for PackageResource<'_> {
    fn current_state(&self) -> Result<ResourceState> {
        match self.manager {
            PackageManager::Pacman | PackageManager::Paru => {
                let result = self.executor.run_unchecked("pacman", &["-Q", &self.name])?;
                if result.success {
                    Ok(ResourceState::Correct)
                } else {
                    Ok(ResourceState::Missing)
                }
            }
            PackageManager::Winget => {
                let result = self.executor.run_unchecked(
                    "winget",
                    &[
                        "list",
                        "--id",
                        &self.name,
                        "--exact",
                        "--accept-source-agreements",
                    ],
                )?;
                if result.success && result.stdout.contains(&self.name) {
                    Ok(ResourceState::Correct)
                } else {
                    Ok(ResourceState::Missing)
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::resources::test_helpers::MockExecutor;

    #[test]
    fn description_includes_manager() {
        let executor = crate::exec::SystemExecutor;
        let resource = PackageResource::new("git".to_string(), PackageManager::Pacman, &executor);
        assert_eq!(resource.description(), "git (pacman)");

        let resource =
            PackageResource::new("paru-bin".to_string(), PackageManager::Paru, &executor);
        assert_eq!(resource.description(), "paru-bin (paru)");

        let resource =
            PackageResource::new("Git.Git".to_string(), PackageManager::Winget, &executor);
        assert_eq!(resource.description(), "Git.Git (winget)");
    }

    #[test]
    fn state_from_installed_correct() {
        let executor = crate::exec::SystemExecutor;
        let resource = PackageResource::new("git".to_string(), PackageManager::Pacman, &executor);
        let mut installed = HashSet::new();
        installed.insert("git".to_string());
        installed.insert("vim".to_string());
        assert_eq!(
            resource.state_from_installed(&installed),
            ResourceState::Correct
        );
    }

    #[test]
    fn state_from_installed_missing() {
        let executor = crate::exec::SystemExecutor;
        let resource = PackageResource::new("git".to_string(), PackageManager::Pacman, &executor);
        let installed = HashSet::new();
        assert_eq!(
            resource.state_from_installed(&installed),
            ResourceState::Missing
        );
    }

    // ------------------------------------------------------------------
    // get_installed_packages
    // ------------------------------------------------------------------

    #[test]
    fn get_installed_pacman_parses_name_version_lines() {
        let executor = MockExecutor::ok("git 2.39.0\nvim 9.0.0\nbase-devel 1.0\n");
        let installed = get_installed_packages(PackageManager::Pacman, &executor).unwrap();
        assert!(installed.contains("git"));
        assert!(installed.contains("vim"));
        assert!(installed.contains("base-devel"));
        assert!(
            !installed.contains("2.39.0"),
            "version number should not be in set"
        );
    }

    #[test]
    fn get_installed_pacman_empty_on_failure() {
        let executor = MockExecutor::fail();
        let installed = get_installed_packages(PackageManager::Pacman, &executor).unwrap();
        assert!(installed.is_empty());
    }

    #[test]
    fn get_installed_winget_parses_id_tokens() {
        let executor = MockExecutor::ok(
            "Name          Id                    Version\nGit           Git.Git               2.39.0\nPowerShell    Microsoft.PowerShell  7.3\n",
        );
        let installed = get_installed_packages(PackageManager::Winget, &executor).unwrap();
        assert!(installed.contains("Git.Git"));
        assert!(installed.contains("Microsoft.PowerShell"));
    }

    // ------------------------------------------------------------------
    // PackageResource::current_state
    // ------------------------------------------------------------------

    #[test]
    fn current_state_pacman_correct_when_query_succeeds() {
        let executor = MockExecutor::ok("git 2.39.0\n");
        let resource = PackageResource::new("git".to_string(), PackageManager::Pacman, &executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn current_state_pacman_missing_when_query_fails() {
        let executor = MockExecutor::fail();
        let resource = PackageResource::new("git".to_string(), PackageManager::Pacman, &executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    }

    #[test]
    fn current_state_winget_correct_when_id_in_output() {
        let executor = MockExecutor::ok("Git.Git  2.39.0\n");
        let resource =
            PackageResource::new("Git.Git".to_string(), PackageManager::Winget, &executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn current_state_winget_missing_when_not_in_output() {
        // success=true but ID not present in stdout
        let executor = MockExecutor::ok("No packages found.\n");
        let resource =
            PackageResource::new("Git.Git".to_string(), PackageManager::Winget, &executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    }

    // ------------------------------------------------------------------
    // PackageResource::apply
    // ------------------------------------------------------------------

    #[test]
    fn apply_pacman_returns_applied_on_success() {
        let executor = MockExecutor::ok("");
        let resource = PackageResource::new("git".to_string(), PackageManager::Pacman, &executor);
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }

    #[test]
    fn apply_paru_returns_applied_on_success() {
        let executor = MockExecutor::ok("");
        let resource =
            PackageResource::new("paru-bin".to_string(), PackageManager::Paru, &executor);
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }

    // ------------------------------------------------------------------
    // batch_install_packages — RecordingExecutor
    // ------------------------------------------------------------------

    /// A test executor that records every `run()` invocation as
    /// `(program, args)` pairs so tests can assert exact command lines.
    #[derive(Debug, Default)]
    struct RecordingExecutor {
        calls: std::sync::Mutex<Vec<(String, Vec<String>)>>,
    }

    impl RecordingExecutor {
        fn new() -> Self {
            Self::default()
        }

        fn recorded_calls(&self) -> Vec<(String, Vec<String>)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl crate::exec::Executor for RecordingExecutor {
        fn run(&self, program: &str, args: &[&str]) -> anyhow::Result<crate::exec::ExecResult> {
            self.calls.lock().unwrap().push((
                program.to_string(),
                args.iter().map(|s| (*s).to_string()).collect(),
            ));
            Ok(crate::exec::ExecResult {
                stdout: String::new(),
                stderr: String::new(),
                success: true,
                code: Some(0),
            })
        }

        fn run_in(
            &self,
            _: &std::path::Path,
            program: &str,
            args: &[&str],
        ) -> anyhow::Result<crate::exec::ExecResult> {
            self.run(program, args)
        }

        fn run_in_with_env(
            &self,
            _: &std::path::Path,
            program: &str,
            args: &[&str],
            _: &[(&str, &str)],
        ) -> anyhow::Result<crate::exec::ExecResult> {
            self.run(program, args)
        }

        fn run_unchecked(
            &self,
            program: &str,
            args: &[&str],
        ) -> anyhow::Result<crate::exec::ExecResult> {
            self.run(program, args)
        }

        fn which(&self, _: &str) -> bool {
            false
        }
    }

    // ------------------------------------------------------------------
    // batch_install_packages
    // ------------------------------------------------------------------

    #[test]
    fn batch_install_pacman_groups_into_single_command() {
        let executor = RecordingExecutor::new();
        let r1 = PackageResource::new("git".to_string(), PackageManager::Pacman, &executor);
        let r2 = PackageResource::new("vim".to_string(), PackageManager::Pacman, &executor);
        batch_install_packages(&[&r1, &r2]).unwrap();

        let calls = executor.recorded_calls();
        assert_eq!(
            calls.len(),
            1,
            "exactly one command for two pacman packages"
        );
        let (prog, args) = &calls[0];
        assert_eq!(prog, "sudo");
        assert_eq!(args[0], "pacman");
        assert_eq!(args[1], "-S");
        assert_eq!(args[2], "--needed");
        assert_eq!(args[3], "--noconfirm");
        assert!(args.contains(&"git".to_string()), "git must be in args");
        assert!(args.contains(&"vim".to_string()), "vim must be in args");
    }

    #[test]
    fn batch_install_paru_groups_into_single_command() {
        let executor = RecordingExecutor::new();
        let r1 = PackageResource::new("paru-bin".to_string(), PackageManager::Paru, &executor);
        let r2 = PackageResource::new("yay".to_string(), PackageManager::Paru, &executor);
        batch_install_packages(&[&r1, &r2]).unwrap();

        let calls = executor.recorded_calls();
        assert_eq!(calls.len(), 1, "exactly one command for two paru packages");
        let (prog, args) = &calls[0];
        assert_eq!(prog, "paru");
        assert_eq!(args[0], "-S");
        assert_eq!(args[1], "--needed");
        assert_eq!(args[2], "--noconfirm");
        assert!(args.contains(&"paru-bin".to_string()));
        assert!(args.contains(&"yay".to_string()));
    }

    #[test]
    fn batch_install_mixed_managers_sends_separate_commands() {
        let pacman_exec = RecordingExecutor::new();
        let paru_exec = RecordingExecutor::new();
        let r1 = PackageResource::new("git".to_string(), PackageManager::Pacman, &pacman_exec);
        let r2 = PackageResource::new("paru-bin".to_string(), PackageManager::Paru, &paru_exec);
        batch_install_packages(&[&r1, &r2]).unwrap();

        // Pacman batch uses pacman_exec
        let pacman_calls = pacman_exec.recorded_calls();
        assert_eq!(pacman_calls.len(), 1);
        assert_eq!(pacman_calls[0].0, "sudo");
        assert!(pacman_calls[0].1.contains(&"git".to_string()));

        // Paru batch uses paru_exec
        let paru_calls = paru_exec.recorded_calls();
        assert_eq!(paru_calls.len(), 1);
        assert_eq!(paru_calls[0].0, "paru");
        assert!(paru_calls[0].1.contains(&"paru-bin".to_string()));
    }

    #[test]
    fn batch_install_empty_list_is_noop() {
        let resources: &[&PackageResource<'_>] = &[];
        batch_install_packages(resources).unwrap();
    }

    #[test]
    fn batch_install_propagates_pacman_error() {
        let executor = MockExecutor::fail();
        let r1 = PackageResource::new("git".to_string(), PackageManager::Pacman, &executor);
        assert!(batch_install_packages(&[&r1]).is_err());
    }

    #[test]
    fn batch_install_winget_skipped_returns_error() {
        // MockExecutor::fail() makes run_unchecked return success=false.
        // PackageResource::apply() for Winget checks result.success and returns
        // ResourceChange::Skipped on failure — batch_install_packages must
        // convert that into an error.
        let executor = MockExecutor::fail();
        let r1 = PackageResource::new("Git.Git".to_string(), PackageManager::Winget, &executor);
        let err = batch_install_packages(&[&r1]).unwrap_err();
        assert!(
            err.to_string().contains("winget install failed"),
            "expected 'winget install failed' in: {err}"
        );
    }
}
