use super::*;
use crate::exec::{ExecResult, MockExecutor};

fn executor_arc<T: Executor + 'static>(executor: &Arc<T>) -> Arc<dyn Executor> {
    Arc::<T>::clone(executor)
}

fn ok_result(stdout: &str) -> ExecResult {
    ExecResult {
        stdout: stdout.to_string(),
        stderr: String::new(),
        success: true,
        code: Some(0),
    }
}

fn fail_result() -> ExecResult {
    ExecResult {
        stdout: String::new(),
        stderr: String::new(),
        success: false,
        code: Some(1),
    }
}

#[test]
fn description_includes_manager() {
    let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
    let pacman_resource = PackageResource::new(
        "git".to_string(),
        PackageManager::Pacman,
        Arc::clone(&executor),
    );
    assert_eq!(pacman_resource.description(), "git (pacman)");

    let paru_resource = PackageResource::new(
        "paru-bin".to_string(),
        PackageManager::Paru,
        Arc::clone(&executor),
    );
    assert_eq!(paru_resource.description(), "paru-bin (paru)");

    let winget_resource = PackageResource::new(
        "Git.Git".to_string(),
        PackageManager::Winget,
        Arc::clone(&executor),
    );
    assert_eq!(winget_resource.description(), "Git.Git (winget)");
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
    let mut mock = MockExecutor::new();
    mock.expect_run_unchecked()
        .once()
        .returning(|_, _| Ok(ok_result("git 2.39.0\nvim 9.0.0\nbase-devel 1.0\n")));
    let installed = get_installed_packages(PackageManager::Pacman, &mock).unwrap();
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
    let mut mock = MockExecutor::new();
    mock.expect_run_unchecked()
        .once()
        .returning(|_, _| Err(anyhow::anyhow!("simulated failure")));
    let result = get_installed_packages(PackageManager::Pacman, &mock);
    assert!(
        result.is_err(),
        "should return an error when the command fails"
    );
}

#[test]
fn get_installed_winget_parses_id_tokens() {
    let mut mock = MockExecutor::new();
    mock.expect_run_unchecked().once().returning(|_, _| {
            Ok(ok_result(
                "Name          Id                    Version\nGit           Git.Git               2.39.0\nPowerShell    Microsoft.PowerShell  7.3\n",
            ))
        });
    let installed = get_installed_packages(PackageManager::Winget, &mock).unwrap();
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

#[test]
fn get_installed_winget_handles_unicode_in_name_column() {
    // The Name column contains multi-byte and multi-width Unicode characters
    // (accented letters, an em dash, and wide CJK characters). winget aligns the
    // Id column by display width, not byte or char offset, so the parser must
    // slice columns by display column to extract IDs from every row.
    let stdout = concat!(
        "Name                          Id                           Version\n",
        "-------------------------------------------------------------------\n",
        // CJK characters (display width 2 each) in Name
        "中文名称 App                  Unicode.App                  1.0.0\n",
        // em dash (display width 1) in Name
        "App \u{2014} Edition                 App.Edition                  2.0.0\n",
        // accented characters (display width 1 each) in Name
        "Ünïcödé App                   Unicode.Accented             3.0.0\n",
    );

    let installed = parse_winget_ids(stdout);
    assert!(
        installed.contains("Unicode.App"),
        "should extract ID from row with CJK characters in Name"
    );
    assert!(
        installed.contains("App.Edition"),
        "should extract ID from row with em dash in Name"
    );
    assert!(
        installed.contains("Unicode.Accented"),
        "should extract ID from row with accented characters in Name"
    );
    assert_eq!(
        installed.len(),
        3,
        "all three package IDs should be collected"
    );
}

// ------------------------------------------------------------------
// PackageResource::apply
// ------------------------------------------------------------------

#[test]
fn apply_pacman_returns_applied_on_success() {
    let mut mock = MockExecutor::new();
    mock.expect_run().once().returning(|_, _| Ok(ok_result("")));
    let executor: Arc<dyn Executor> = Arc::new(mock);
    let resource = PackageResource::new(
        "git".to_string(),
        PackageManager::Pacman,
        Arc::clone(&executor),
    );
    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
}

#[test]
fn apply_paru_returns_applied_on_success() {
    let mut mock = MockExecutor::new();
    mock.expect_run().once().returning(|_, _| Ok(ok_result("")));
    let executor: Arc<dyn Executor> = Arc::new(mock);
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

impl Executor for RecordingExecutor {
    fn run(&self, program: &str, args: &[&str]) -> Result<ExecResult> {
        self.calls.lock().unwrap().push((
            program.to_string(),
            args.iter().map(|s| (*s).to_string()).collect(),
        ));
        Ok(ExecResult {
            stdout: String::new(),
            stderr: String::new(),
            success: true,
            code: Some(0),
        })
    }

    fn run_in_with_env(
        &self,
        _: &std::path::Path,
        program: &str,
        args: &[&str],
        _: &[(&str, &str)],
    ) -> Result<ExecResult> {
        self.run(program, args)
    }

    fn run_unchecked(&self, program: &str, args: &[&str]) -> Result<ExecResult> {
        self.run(program, args)
    }

    fn which(&self, _: &str) -> bool {
        false
    }

    fn which_path(&self, program: &str) -> Result<std::path::PathBuf> {
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
        executor_arc(&executor),
    );
    let r2 = PackageResource::new(
        "vim".to_string(),
        PackageManager::Pacman,
        executor_arc(&executor),
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
    assert_eq!(args[1], "-Syu");
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
        executor_arc(&executor),
    );
    let r2 = PackageResource::new(
        "yay".to_string(),
        PackageManager::Paru,
        executor_arc(&executor),
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
        executor_arc(&pacman_exec),
    );
    let r2 = PackageResource::new(
        "paru-bin".to_string(),
        PackageManager::Paru,
        executor_arc(&paru_exec),
    );
    batch_install_packages(&[&r1, &r2]).unwrap();

    // Pacman batch uses pacman_exec
    let pacman_calls = pacman_exec.recorded_calls();
    assert_eq!(pacman_calls.len(), 1);
    assert_eq!(pacman_calls[0].0, "sudo");
    assert!(pacman_calls[0].1.contains(&"-Syu".to_string()));
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
    let mut mock = MockExecutor::new();
    mock.expect_run()
        .once()
        .returning(|_, _| Err(anyhow::anyhow!("simulated failure")));
    let executor: Arc<dyn Executor> = Arc::new(mock);
    let r1 = PackageResource::new(
        "git".to_string(),
        PackageManager::Pacman,
        Arc::clone(&executor),
    );
    assert!(batch_install_packages(&[&r1]).is_err());
}

#[test]
fn batch_install_winget_skipped_returns_error() {
    // MockExecutor::run_unchecked returns success=false.
    // PackageResource::apply() for Winget checks result.success and returns
    // ResourceChange::Skipped on failure — batch_install_packages must
    // convert that into an error.
    let mut mock = MockExecutor::new();
    mock.expect_run_unchecked()
        .once()
        .returning(|_, _| Ok(fail_result()));
    let executor: Arc<dyn Executor> = Arc::new(mock);
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
