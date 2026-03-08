//! Package installation resource.
//!
//! The [`PackageProvider`] trait abstracts over different package managers
//! (pacman, paru, winget). Adding support for a new manager requires only a
//! new implementation of `PackageProvider` and a corresponding variant in
//! [`PackageManager`].
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;

use super::{Applicable, Resource, ResourceChange, ResourceState};
use crate::exec::Executor;

// ---------------------------------------------------------------------------
// PackageProvider trait
// ---------------------------------------------------------------------------

/// Abstraction over package manager operations.
///
/// Each implementation encapsulates the command-line interface of a specific
/// package manager, allowing new managers to be added without modifying the
/// core resource processing logic.
///
/// See [`PacmanProvider`], [`ParuProvider`], and [`WingetProvider`] for
/// concrete implementations.
pub trait PackageProvider: std::fmt::Debug + Send + Sync {
    /// Human-readable name of this provider (e.g., `"pacman"`).
    fn name(&self) -> &'static str;

    /// Query all currently installed package names.
    ///
    /// Returns a set of names/IDs that can be matched against desired
    /// package names to determine what is already installed. Runs a
    /// **single** command regardless of how many packages need checking.
    ///
    /// # Errors
    ///
    /// Returns an error if the package manager command fails or output
    /// cannot be parsed.
    fn query_installed(&self, executor: &dyn Executor) -> Result<HashSet<String>>;

    /// Check whether a single package is currently installed.
    ///
    /// # Errors
    ///
    /// Returns an error if the package manager command fails.
    fn is_installed(&self, name: &str, executor: &dyn Executor) -> Result<bool>;

    /// Install a single package.
    ///
    /// # Errors
    ///
    /// Returns an error if the installation command fails.
    fn install(&self, name: &str, executor: &dyn Executor) -> Result<ResourceChange>;

    /// Whether this provider supports installing multiple packages in one command.
    fn supports_batch(&self) -> bool {
        false
    }

    /// Install multiple packages in a single invocation.
    ///
    /// Only called when [`supports_batch`](Self::supports_batch) returns `true`.
    ///
    /// # Errors
    ///
    /// Returns an error if the batch installation command fails.
    fn batch_install(&self, _names: &[&str], _executor: &dyn Executor) -> Result<()> {
        anyhow::bail!("batch install not supported by {}", self.name())
    }
}

// ---------------------------------------------------------------------------
// Shared query helper
// ---------------------------------------------------------------------------

/// Parse strategy for extracting package names from command output.
#[derive(Clone, Copy)]
enum ParseMode {
    /// Take only the first whitespace-delimited token per line (e.g. pacman).
    FirstToken,
}

/// Run a package manager command and collect package names from its output.
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

/// Split a padded CLI table row into logical columns.
///
/// Single spaces are preserved inside a column; a run of two or more spaces is
/// treated as a column separator.
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

            if !current.trim().is_empty() {
                cols.push(current.trim().to_string());
                current.clear();
            }
        } else {
            if spaces == 1 {
                current.push(' ');
            }
            spaces = 0;
            current.push(ch);
        }
    }

    if !current.trim().is_empty() {
        cols.push(current.trim().to_string());
    }

    cols
}

/// Parse package IDs from `winget list` output.
fn parse_winget_ids(stdout: &str) -> HashSet<String> {
    let mut ids = HashSet::new();
    let mut id_index = None;

    for line in stdout.lines() {
        let cols = split_padded_columns(line);
        if cols.is_empty() {
            continue;
        }

        if id_index.is_none() {
            id_index = cols.iter().position(|col| col == "Id");
            continue;
        }

        if cols.iter().all(|col| col.chars().all(|c| c == '-')) {
            continue;
        }

        if let Some(idx) = id_index
            && let Some(id) = cols.get(idx)
            && !id.is_empty()
        {
            ids.insert(id.clone());
        }
    }

    ids
}

// ---------------------------------------------------------------------------
// Provider implementations
// ---------------------------------------------------------------------------

/// Pacman provider for official Arch Linux packages.
#[derive(Debug, Clone, Copy)]
pub struct PacmanProvider;

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
        executor.run("sudo", &["pacman", "-S", "--needed", "--noconfirm", name])?;
        Ok(ResourceChange::Applied)
    }

    fn supports_batch(&self) -> bool {
        true
    }

    fn batch_install(&self, names: &[&str], executor: &dyn Executor) -> Result<()> {
        let mut args = vec!["pacman", "-S", "--needed", "--noconfirm"];
        args.extend(names);
        executor.run("sudo", &args)?;
        Ok(())
    }
}

/// Paru provider for AUR packages.
#[derive(Debug, Clone, Copy)]
pub struct ParuProvider;

impl PackageProvider for ParuProvider {
    fn name(&self) -> &'static str {
        "paru"
    }

    fn query_installed(&self, executor: &dyn Executor) -> Result<HashSet<String>> {
        // Paru packages are also visible via pacman -Q.
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

/// Winget provider for Windows packages.
#[derive(Debug, Clone, Copy)]
pub struct WingetProvider;

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

// ---------------------------------------------------------------------------
// PackageManager enum
// ---------------------------------------------------------------------------

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

impl PackageManager {
    /// Return the [`PackageProvider`] implementation for this manager.
    #[must_use]
    pub fn provider(self) -> &'static dyn PackageProvider {
        match self {
            Self::Pacman => &PacmanProvider,
            Self::Paru => &ParuProvider,
            Self::Winget => &WingetProvider,
        }
    }
}

impl std::fmt::Display for PackageManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.provider().name())
    }
}

/// A system package resource that can be checked and installed.
#[derive(Debug)]
pub struct PackageResource {
    /// Package name (or winget ID).
    pub name: String,
    /// Package manager to use.
    pub manager: PackageManager,
    /// Provider implementation for this package manager.
    provider: &'static dyn PackageProvider,
    /// Executor for running package manager commands.
    executor: Arc<dyn Executor>,
}

impl PackageResource {
    /// Create a new package resource.
    #[must_use]
    pub fn new(name: String, manager: PackageManager, executor: Arc<dyn Executor>) -> Self {
        Self {
            name,
            manager,
            provider: manager.provider(),
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
/// Delegates to the manager's [`PackageProvider::query_installed`]
/// implementation, running a **single** command regardless of how many
/// packages need to be checked.
///
/// # Errors
///
/// Returns an error if the package manager command fails to execute or if
/// the output cannot be parsed.
pub fn get_installed_packages(
    manager: PackageManager,
    executor: &dyn Executor,
) -> Result<HashSet<String>> {
    manager.provider().query_installed(executor)
}

/// Install a batch of packages in a single command, grouped by package manager.
///
/// Groups the given resources by their [`PackageManager`] and delegates to
/// each provider's batch or individual install method. Providers that support
/// batch installation (pacman, paru) install all missing packages in one
/// command; providers that do not (winget) install individually.
///
/// # Errors
///
/// Returns an error if any package manager command fails, or if a Winget
/// install is skipped (i.e. the installer reported failure).
pub fn batch_install_packages(resources: &[&PackageResource]) -> Result<()> {
    for &manager in &[
        PackageManager::Pacman,
        PackageManager::Paru,
        PackageManager::Winget,
    ] {
        let group: Vec<_> = resources.iter().filter(|r| r.manager == manager).collect();
        if group.is_empty() {
            continue;
        }

        let provider = manager.provider();
        let Some(first) = group.first() else {
            continue;
        };
        let executor = &*first.executor;

        if provider.supports_batch() {
            let names: Vec<&str> = group.iter().map(|r| r.name.as_str()).collect();
            provider.batch_install(&names, executor)?;
        } else {
            // Individual install — propagate skipped installations as errors.
            for resource in &group {
                let change = resource.apply()?;
                if let ResourceChange::Skipped { reason } = change {
                    return Err(crate::error::ResourceError::command_failed(
                        provider.name(),
                        format!("install failed for '{}': {reason}", resource.name),
                    )
                    .into());
                }
            }
        }
    }

    Ok(())
}

impl Applicable for PackageResource {
    fn description(&self) -> String {
        format!("{} ({})", self.name, self.manager)
    }

    fn apply(&self) -> Result<ResourceChange> {
        self.provider.install(&self.name, &*self.executor)
    }
}

impl Resource for PackageResource {
    fn current_state(&self) -> Result<ResourceState> {
        let installed = self.provider.is_installed(&self.name, &*self.executor)?;
        if installed {
            Ok(ResourceState::Correct)
        } else {
            Ok(ResourceState::Missing)
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::exec::test_helpers::TestExecutor;

    #[test]
    fn description_includes_manager() {
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let resource = PackageResource::new(
            "git".to_string(),
            PackageManager::Pacman,
            Arc::clone(&executor),
        );
        assert_eq!(resource.description(), "git (pacman)");

        let resource = PackageResource::new(
            "paru-bin".to_string(),
            PackageManager::Paru,
            Arc::clone(&executor),
        );
        assert_eq!(resource.description(), "paru-bin (paru)");

        let resource = PackageResource::new(
            "Git.Git".to_string(),
            PackageManager::Winget,
            Arc::clone(&executor),
        );
        assert_eq!(resource.description(), "Git.Git (winget)");
    }

    #[test]
    fn state_from_installed_correct() {
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let resource = PackageResource::new(
            "git".to_string(),
            PackageManager::Pacman,
            Arc::clone(&executor),
        );
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
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let resource = PackageResource::new(
            "git".to_string(),
            PackageManager::Pacman,
            Arc::clone(&executor),
        );
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
        let executor = TestExecutor::ok("git 2.39.0\nvim 9.0.0\nbase-devel 1.0\n");
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
    fn get_installed_pacman_returns_error_on_failure() {
        let executor = TestExecutor::fail();
        let result = get_installed_packages(PackageManager::Pacman, &executor);
        assert!(
            result.is_err(),
            "should return an error when the command fails"
        );
    }

    #[test]
    fn get_installed_winget_parses_id_tokens() {
        let executor = TestExecutor::ok(
            "Name          Id                    Version\nGit           Git.Git               2.39.0\nPowerShell    Microsoft.PowerShell  7.3\n",
        );
        let installed = get_installed_packages(PackageManager::Winget, &executor).unwrap();
        assert!(installed.contains("Git.Git"));
        assert!(installed.contains("Microsoft.PowerShell"));
        assert!(
            !installed.contains("Git"),
            "display names should not be included"
        );
    }

    #[test]
    fn get_installed_winget_ignores_separator_and_extra_columns() {
        let stdout = concat!(
            "Name                          Id                           Version        Available Source\n",
            "-------------------------------------------------------------------------------------\n",
            "Git                           Git.Git                      2.45.1         2.46.0   winget\n",
            "Windows Terminal              Microsoft.WindowsTerminal    1.21.2361.0             winget\n",
        );

        let installed = parse_winget_ids(stdout);
        assert!(installed.contains("Git.Git"));
        assert!(installed.contains("Microsoft.WindowsTerminal"));
        assert_eq!(installed.len(), 2, "only package IDs should be collected");
    }

    // ------------------------------------------------------------------
    // PackageResource::current_state
    // ------------------------------------------------------------------

    #[test]
    fn current_state_pacman_correct_when_query_succeeds() {
        let executor: Arc<dyn Executor> = Arc::new(TestExecutor::ok("git 2.39.0\n"));
        let resource = PackageResource::new(
            "git".to_string(),
            PackageManager::Pacman,
            Arc::clone(&executor),
        );
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn current_state_pacman_missing_when_query_fails() {
        let executor: Arc<dyn Executor> = Arc::new(TestExecutor::fail());
        let resource = PackageResource::new(
            "git".to_string(),
            PackageManager::Pacman,
            Arc::clone(&executor),
        );
        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    }

    #[test]
    fn current_state_winget_correct_when_id_in_output() {
        let executor: Arc<dyn Executor> = Arc::new(TestExecutor::ok("Git.Git  2.39.0\n"));
        let resource = PackageResource::new(
            "Git.Git".to_string(),
            PackageManager::Winget,
            Arc::clone(&executor),
        );
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn current_state_winget_missing_when_not_in_output() {
        // success=true but ID not present in stdout
        let executor: Arc<dyn Executor> = Arc::new(TestExecutor::ok("No packages found.\n"));
        let resource = PackageResource::new(
            "Git.Git".to_string(),
            PackageManager::Winget,
            Arc::clone(&executor),
        );
        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    }

    // ------------------------------------------------------------------
    // PackageResource::apply
    // ------------------------------------------------------------------

    #[test]
    fn apply_pacman_returns_applied_on_success() {
        let executor: Arc<dyn Executor> = Arc::new(TestExecutor::ok(""));
        let resource = PackageResource::new(
            "git".to_string(),
            PackageManager::Pacman,
            Arc::clone(&executor),
        );
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }

    #[test]
    fn apply_paru_returns_applied_on_success() {
        let executor: Arc<dyn Executor> = Arc::new(TestExecutor::ok(""));
        let resource = PackageResource::new(
            "paru-bin".to_string(),
            PackageManager::Paru,
            Arc::clone(&executor),
        );
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

        fn which_path(&self, program: &str) -> anyhow::Result<std::path::PathBuf> {
            anyhow::bail!("{program} not found on PATH")
        }
    }

    // ------------------------------------------------------------------
    // batch_install_packages
    // ------------------------------------------------------------------

    #[test]
    fn batch_install_pacman_groups_into_single_command() {
        let executor = Arc::new(RecordingExecutor::new());
        let r1 = PackageResource::new(
            "git".to_string(),
            PackageManager::Pacman,
            Arc::clone(&executor) as Arc<dyn Executor>,
        );
        let r2 = PackageResource::new(
            "vim".to_string(),
            PackageManager::Pacman,
            Arc::clone(&executor) as Arc<dyn Executor>,
        );
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
        let executor = Arc::new(RecordingExecutor::new());
        let r1 = PackageResource::new(
            "paru-bin".to_string(),
            PackageManager::Paru,
            Arc::clone(&executor) as Arc<dyn Executor>,
        );
        let r2 = PackageResource::new(
            "yay".to_string(),
            PackageManager::Paru,
            Arc::clone(&executor) as Arc<dyn Executor>,
        );
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
        let pacman_exec = Arc::new(RecordingExecutor::new());
        let paru_exec = Arc::new(RecordingExecutor::new());
        let r1 = PackageResource::new(
            "git".to_string(),
            PackageManager::Pacman,
            Arc::clone(&pacman_exec) as Arc<dyn Executor>,
        );
        let r2 = PackageResource::new(
            "paru-bin".to_string(),
            PackageManager::Paru,
            Arc::clone(&paru_exec) as Arc<dyn Executor>,
        );
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
        let resources: &[&PackageResource] = &[];
        batch_install_packages(resources).unwrap();
    }

    #[test]
    fn batch_install_propagates_pacman_error() {
        let executor: Arc<dyn Executor> = Arc::new(TestExecutor::fail());
        let r1 = PackageResource::new(
            "git".to_string(),
            PackageManager::Pacman,
            Arc::clone(&executor),
        );
        assert!(batch_install_packages(&[&r1]).is_err());
    }

    #[test]
    fn batch_install_winget_skipped_returns_error() {
        // TestExecutor::fail() makes run_unchecked return success=false.
        // PackageResource::apply() for Winget checks result.success and returns
        // ResourceChange::Skipped on failure — batch_install_packages must
        // convert that into an error.
        let executor: Arc<dyn Executor> = Arc::new(TestExecutor::fail());
        let r1 = PackageResource::new(
            "Git.Git".to_string(),
            PackageManager::Winget,
            Arc::clone(&executor),
        );
        let err = batch_install_packages(&[&r1]).unwrap_err();
        assert!(
            err.to_string().contains("winget install failed"),
            "expected 'winget install failed' in: {err}"
        );
    }
}
