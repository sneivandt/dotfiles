#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration tests use panicking assertions for clear failures"
)]
//! Behavioral CI coverage.
//!
//! These tests focus on contracts that are easy for CI smoke tests to miss:
//! profile/platform filtering, complete filesystem outcomes, idempotency, and
//! command lines emitted to external tools through fake executors.

mod common;

use dotfiles_cli::testing as test_api;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use test_api::exec::{ExecResult, Executor};
use test_api::logging::{Log, Logger};
use test_api::platform::{Os, Platform};
use test_api::tasks::{Context, ContextOpts, Task, TaskResult};

fn log_arc(log: &Arc<Logger>) -> Arc<dyn Log> {
    Arc::<Logger>::clone(log)
}

fn executor_arc<T: Executor + 'static>(executor: &Arc<T>) -> Arc<dyn Executor> {
    Arc::<T>::clone(executor)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallKind {
    Run,
    RunUnchecked,
}

#[derive(Debug)]
struct ExpectedCall {
    kind: CallKind,
    program: String,
    args: Vec<String>,
    result: ExecResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordedCall {
    kind: CallKind,
    program: String,
    args: Vec<String>,
}

#[derive(Debug)]
struct RecordingExecutor {
    available: HashSet<String>,
    expected: Mutex<VecDeque<ExpectedCall>>,
    calls: Mutex<Vec<RecordedCall>>,
}

impl RecordingExecutor {
    fn new(available: &[&str], expected: Vec<ExpectedCall>) -> Self {
        Self {
            available: available
                .iter()
                .map(|program| (*program).to_string())
                .collect(),
            expected: Mutex::new(VecDeque::from(expected)),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().unwrap().clone()
    }

    fn assert_complete(&self) {
        let guard = self.expected.lock().unwrap();
        if guard.is_empty() {
            return;
        }
        let remaining = format!("{guard:?}");
        drop(guard);
        panic!("executor had unconsumed expectations: {remaining}");
    }

    fn next(&self, kind: CallKind, program: &str, args: &[&str]) -> ExecResult {
        let recorded = RecordedCall {
            kind,
            program: program.to_string(),
            args: args.iter().map(|arg| (*arg).to_string()).collect(),
        };
        self.calls.lock().unwrap().push(recorded.clone());

        let expected = self
            .expected
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| panic!("unexpected executor call: {recorded:?}"));

        assert_eq!(expected.kind, recorded.kind, "executor call kind mismatch");
        assert_eq!(
            expected.program, recorded.program,
            "executor program mismatch"
        );
        assert_eq!(expected.args, recorded.args, "executor args mismatch");
        expected.result
    }
}

impl Executor for RecordingExecutor {
    fn run(&self, program: &str, args: &[&str]) -> anyhow::Result<ExecResult> {
        Ok(self.next(CallKind::Run, program, args))
    }

    fn run_in_with_env(
        &self,
        _: &Path,
        program: &str,
        args: &[&str],
        _: &[(&str, &str)],
    ) -> anyhow::Result<ExecResult> {
        Ok(self.next(CallKind::Run, program, args))
    }

    fn run_unchecked(&self, program: &str, args: &[&str]) -> anyhow::Result<ExecResult> {
        Ok(self.next(CallKind::RunUnchecked, program, args))
    }

    fn which(&self, program: &str) -> bool {
        self.available.contains(program)
    }

    fn which_path(&self, program: &str) -> anyhow::Result<PathBuf> {
        if self.which(program) {
            return Ok(PathBuf::from(format!("/fake/bin/{program}")));
        }
        anyhow::bail!("{program} not found on PATH")
    }
}

fn ok(stdout: &str) -> ExecResult {
    ExecResult {
        stdout: stdout.to_string(),
        stderr: String::new(),
        success: true,
        code: Some(0),
    }
}

const fn platform(os: Os, is_arch: bool) -> Platform {
    Platform {
        os,
        is_arch,
        is_wsl: false,
    }
}

fn disabled(stdout: &str) -> ExecResult {
    ExecResult {
        stdout: stdout.to_string(),
        stderr: String::new(),
        success: false,
        code: Some(1),
    }
}

fn expect(kind: CallKind, program: &str, args: &[&str], result: ExecResult) -> ExpectedCall {
    ExpectedCall {
        kind,
        program: program.to_string(),
        args: args.iter().map(|arg| (*arg).to_string()).collect(),
        result,
    }
}

fn expect_code_cmd(cmd: &str, args: &[&str], result: ExecResult) -> ExpectedCall {
    #[cfg(target_os = "windows")]
    {
        let mut full_args = vec!["/C".to_string(), cmd.to_string()];
        full_args.extend(args.iter().map(|arg| (*arg).to_string()));
        ExpectedCall {
            kind: CallKind::RunUnchecked,
            program: "cmd".to_string(),
            args: full_args,
            result,
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        expect(CallKind::RunUnchecked, cmd, args, result)
    }
}

fn make_context(
    repo: &common::IntegrationTestContext,
    profile: &str,
    platform: Platform,
    executor: Arc<dyn Executor>,
) -> (Context, Arc<Logger>, tempfile::TempDir) {
    let config = repo.load_config_for_platform(profile, platform);
    let home = tempfile::tempdir().expect("create temp home");
    let log = Arc::new(Logger::new("behavioral-ci"));
    let ctx = Context::from_raw(
        Arc::new(std::sync::RwLock::new(Arc::new(config))),
        platform,
        log_arc(&log),
        executor,
        home.path().to_path_buf(),
        ContextOpts {
            dry_run: false,
            parallel: false,
            advance_versions: false,
            is_ci: Some(false),
        },
    );
    (ctx, log, home)
}

#[cfg(unix)]
fn symlink_target(home: &Path, source: &str, explicit_target: Option<&str>) -> PathBuf {
    explicit_target.map_or_else(
        || home.join(format!(".{source}")),
        |target| home.join(target),
    )
}

#[test]
fn profile_and_platform_filtering_selects_the_declared_state() {
    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "symlinks.toml",
            r#"[base]
symlinks = ["base-file"]

[linux]
symlinks = ["linux-file"]

[desktop]
symlinks = ["desktop-file"]

[linux-desktop]
symlinks = ["linux-desktop-file"]

[windows]
symlinks = ["windows-file"]

[windows-desktop]
symlinks = ["windows-desktop-file"]
"#,
        )
        .with_config_file(
            "packages.toml",
            r#"[arch]
packages = ["git", { name = "paru-bin", aur = true }]

[arch-desktop]
packages = ["alacritty", { name = "visual-studio-code-insiders-bin", aur = true }]

[windows]
packages = ["Git.Git"]
"#,
        )
        .with_symlink_source("base-file")
        .with_symlink_source("linux-file")
        .with_symlink_source("desktop-file")
        .with_symlink_source("linux-desktop-file")
        .with_symlink_source("windows-file")
        .with_symlink_source("windows-desktop-file")
        .build();

    let linux_desktop = repo.load_config_for_platform(
        "desktop",
        Platform {
            os: Os::Linux,
            is_arch: false,
            is_wsl: false,
        },
    );
    let sources: Vec<&str> = linux_desktop
        .symlinks
        .iter()
        .map(|symlink| symlink.source.as_str())
        .collect();

    assert_eq!(
        sources,
        vec![
            "base-file",
            "desktop-file",
            "linux-file",
            "linux-desktop-file"
        ],
        "Linux desktop profile should include only base, desktop, linux, and linux-desktop"
    );

    let arch_desktop = repo.load_config_for_platform(
        "desktop",
        Platform {
            os: Os::Linux,
            is_arch: true,
            is_wsl: false,
        },
    );
    let packages: Vec<(&str, bool)> = arch_desktop
        .packages
        .iter()
        .map(|package| (package.name.as_str(), package.is_aur))
        .collect();

    assert_eq!(
        packages,
        vec![
            ("git", false),
            ("paru-bin", true),
            ("alacritty", false),
            ("visual-studio-code-insiders-bin", true)
        ],
        "Arch desktop profile should include native and AUR packages from both arch layers"
    );
}

#[cfg(unix)]
#[test]
fn symlink_round_trip_verifies_every_declared_target() {
    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "symlinks.toml",
            r#"[base]
symlinks = [
  "bashrc",
  "config/git/config",
  { source = "AppData/Roaming/Code/User/settings.json", target = "AppData/Roaming/Code/User/settings.json" },
]
"#,
        )
        .with_symlink_source_content("bashrc", "# bashrc\n")
        .with_symlink_source_content("config/git/config", "[init]\n")
        .with_symlink_source_content(
            "AppData/Roaming/Code/User/settings.json",
            "{ \"editor.fontSize\": 14 }\n",
        )
        .build();
    let (ctx, log, _home) = make_context(
        &repo,
        "base",
        platform(Os::Linux, false),
        Arc::new(common::StubExecutor),
    );
    let config = ctx.config_read();
    let expected: Vec<_> = config
        .symlinks
        .iter()
        .map(|symlink| {
            (
                symlink.source.clone(),
                symlink_target(&ctx.home, &symlink.source, symlink.target.as_deref()),
            )
        })
        .collect();
    drop(config);

    let first = test_api::tasks::files::symlinks::InstallSymlinks
        .run(&ctx)
        .expect("install symlinks");
    assert!(matches!(first, TaskResult::Ok));

    for (source, target) in &expected {
        let metadata = std::fs::symlink_metadata(target)
            .unwrap_or_else(|_| panic!("missing installed target: {}", target.display()));
        assert!(
            metadata.is_symlink(),
            "{} should be a symlink",
            target.display()
        );
        assert_eq!(
            std::fs::read_link(target).expect("read symlink"),
            repo.root_path().join("symlinks").join(source),
            "{} should point at its configured source",
            target.display()
        );
    }

    let second = test_api::tasks::files::symlinks::InstallSymlinks
        .run(&ctx)
        .expect("second install symlinks");
    assert!(matches!(second, TaskResult::Ok));

    let uninstall = test_api::tasks::files::symlinks::UninstallSymlinks
        .run(&ctx)
        .expect("uninstall symlinks");
    assert!(matches!(uninstall, TaskResult::Ok));

    for (source, target) in &expected {
        let metadata = std::fs::symlink_metadata(target)
            .unwrap_or_else(|_| panic!("missing materialized target: {}", target.display()));
        assert!(
            !metadata.is_symlink(),
            "{} should be materialized after uninstall",
            target.display()
        );
        let source_content =
            std::fs::read_to_string(repo.root_path().join("symlinks").join(source))
                .expect("read source");
        let target_content = std::fs::read_to_string(target).expect("read target");
        assert_eq!(target_content, source_content);
    }

    let second_uninstall = test_api::tasks::files::symlinks::UninstallSymlinks
        .run(&ctx)
        .expect("second uninstall symlinks");
    assert!(matches!(second_uninstall, TaskResult::Ok));
    assert!(!log.has_failures(), "round trip should not record failures");
}

#[test]
fn negative_validation_fixture_reports_all_independent_failures() {
    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "symlinks.toml",
            r#"[base]
symlinks = [
  "missing-source",
  "/absolute-source",
  { source = "safe", target = "../escape" },
]
"#,
        )
        .with_config_file(
            "chmod.toml",
            r#"[base]
permissions = [
  { path = "/absolute", mode = "600" },
  { path = ".ssh/config", mode = "999" },
]
"#,
        )
        .with_symlink_source("safe")
        .build();
    let config = repo.load_config("base");
    let warnings = config.validate(platform(Os::Linux, false));
    let messages = warnings
        .iter()
        .map(|warning| format!("{}: {}", warning.source, warning.message))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        messages.contains("does not exist"),
        "missing source should be reported:\n{messages}"
    );
    assert!(
        messages.contains("should be relative"),
        "absolute paths should be reported:\n{messages}"
    );
    assert!(
        messages.contains("must not contain '..'"),
        "target path traversal should be reported:\n{messages}"
    );
    assert!(
        messages.contains("octal"),
        "invalid chmod mode should be reported:\n{messages}"
    );
}

#[test]
fn pacman_task_installs_only_missing_native_packages_in_one_batch() {
    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "packages.toml",
            "[arch]\npackages = [\"git\", \"ripgrep\"]\n",
        )
        .build();
    let executor = Arc::new(RecordingExecutor::new(
        &["pacman"],
        vec![
            expect(
                CallKind::RunUnchecked,
                "pacman",
                &["-Q"],
                ok("git 2.51.0\n"),
            ),
            expect(
                CallKind::Run,
                "sudo",
                &["pacman", "-Syu", "--needed", "--noconfirm", "ripgrep"],
                ok(""),
            ),
        ],
    ));
    let (ctx, _log, _home) = make_context(
        &repo,
        "base",
        Platform {
            os: Os::Linux,
            is_arch: true,
            is_wsl: false,
        },
        executor_arc(&executor),
    );

    let result = test_api::tasks::packages::InstallPackages
        .run(&ctx)
        .expect("install packages");

    assert!(matches!(result, TaskResult::OkWithMessage(_)));
    executor.assert_complete();
    assert_eq!(
        executor.calls().len(),
        2,
        "package state should be queried once"
    );
}

#[test]
fn paru_task_installs_only_missing_aur_packages_without_sudo_wrapper() {
    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "packages.toml",
            "[arch]\npackages = [{ name = \"apm-bin\", aur = true }, { name = \"powershell-bin\", aur = true }]\n",
        )
        .build();
    let executor = Arc::new(RecordingExecutor::new(
        &["paru"],
        vec![
            expect(
                CallKind::RunUnchecked,
                "pacman",
                &["-Q"],
                ok("apm-bin 1.0.0\n"),
            ),
            expect(
                CallKind::Run,
                "paru",
                &["-S", "--needed", "--noconfirm", "powershell-bin"],
                ok(""),
            ),
        ],
    ));
    let (ctx, _log, _home) = make_context(
        &repo,
        "base",
        Platform {
            os: Os::Linux,
            is_arch: true,
            is_wsl: false,
        },
        executor_arc(&executor),
    );

    let result = test_api::tasks::packages::InstallAurPackages
        .run(&ctx)
        .expect("install aur packages");

    assert!(matches!(result, TaskResult::OkWithMessage(_)));
    executor.assert_complete();
}

#[test]
fn winget_task_uses_exact_ids_and_installs_each_missing_package() {
    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "packages.toml",
            "[windows]\npackages = [\"Git.Git\", \"Microsoft.PowerShell\"]\n",
        )
        .build();
    let executor = Arc::new(RecordingExecutor::new(
        &["winget"],
        vec![
            expect(
                CallKind::RunUnchecked,
                "winget",
                &[
                    "list",
                    "--accept-source-agreements",
                    "--disable-interactivity",
                ],
                ok("Name             Id        Version\nGit              Git.Git   2.51.0\n"),
            ),
            expect(
                CallKind::RunUnchecked,
                "winget",
                &[
                    "install",
                    "--id",
                    "Microsoft.PowerShell",
                    "--exact",
                    "--source",
                    "winget",
                    "--accept-source-agreements",
                    "--accept-package-agreements",
                ],
                ok(""),
            ),
        ],
    ));
    let (ctx, _log, _home) = make_context(
        &repo,
        "base",
        platform(Os::Windows, false),
        executor_arc(&executor),
    );

    let result = test_api::tasks::packages::InstallPackages
        .run(&ctx)
        .expect("install winget packages");

    assert!(matches!(result, TaskResult::OkWithMessage(_)));
    executor.assert_complete();
}

#[test]
fn vscode_task_queries_once_and_installs_only_missing_extensions() {
    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "vscode-extensions.toml",
            "[desktop]\nextensions = [\"github.copilot-chat\", \"ms-python.python\"]\n",
        )
        .build();
    let executor = Arc::new(RecordingExecutor::new(
        &["code-insiders"],
        vec![
            expect_code_cmd(
                "code-insiders",
                &["--list-extensions"],
                ok("GitHub.Copilot-Chat\n"),
            ),
            expect_code_cmd(
                "code-insiders",
                &["--install-extension", "ms-python.python", "--force"],
                ok(""),
            ),
        ],
    ));
    let (ctx, _log, _home) = make_context(
        &repo,
        "desktop",
        platform(Os::Linux, false),
        executor_arc(&executor),
    );

    let result = test_api::tasks::editors::vscode_extensions::InstallVsCodeExtensions
        .run(&ctx)
        .expect("install vscode extensions");

    assert!(matches!(result, TaskResult::Ok));
    executor.assert_complete();
}

#[test]
fn systemd_task_reloads_then_enables_user_and_system_units() {
    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "systemd-units.toml",
            "[linux]\nunits = [\"clean-home-tmp.timer\", { name = \"sshd.service\", scope = \"system\" }]\n",
        )
        .build();
    let executor = Arc::new(RecordingExecutor::new(
        &["systemctl"],
        vec![
            expect(
                CallKind::Run,
                "systemctl",
                &["--user", "daemon-reload"],
                ok(""),
            ),
            expect(
                CallKind::Run,
                "sudo",
                &["systemctl", "daemon-reload"],
                ok(""),
            ),
            expect(
                CallKind::RunUnchecked,
                "systemctl",
                &["--user", "is-enabled", "clean-home-tmp.timer"],
                disabled("disabled\n"),
            ),
            expect(
                CallKind::RunUnchecked,
                "systemctl",
                &["--user", "enable", "--now", "clean-home-tmp.timer"],
                ok(""),
            ),
            expect(
                CallKind::RunUnchecked,
                "systemctl",
                &["is-enabled", "sshd.service"],
                disabled("disabled\n"),
            ),
            expect(
                CallKind::RunUnchecked,
                "sudo",
                &["systemctl", "enable", "--now", "sshd.service"],
                ok(""),
            ),
        ],
    ));
    let (ctx, _log, _home) = make_context(
        &repo,
        "base",
        platform(Os::Linux, false),
        executor_arc(&executor),
    );

    let result = test_api::tasks::system::systemd_units::ConfigureSystemd
        .run(&ctx)
        .expect("configure systemd");

    assert!(matches!(result, TaskResult::Ok));
    executor.assert_complete();
}
