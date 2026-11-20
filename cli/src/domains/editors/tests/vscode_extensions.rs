use super::*;
use crate::infra::ConfigHandle;
use crate::infra::env::MapEnv;
use crate::infra::exec::{ExecError, ExecResult, MockExecutor};
use crate::infra::logging::TaskStatus;
use crate::infra::platform::{Os, Platform};
use crate::test_helpers::{CapturingOutput, make_context, make_linux_context, task_batch};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn ext() -> String {
    "github.copilot".to_string()
}

fn expect_code(
    mock: &mut MockExecutor,
    sequence: &mut mockall::Sequence,
    launcher: &Path,
    args: &[&str],
    output: &'static str,
) {
    expect_code_result(
        mock,
        sequence,
        launcher,
        args,
        Ok(ExecResult::success(output)),
    );
}

fn expect_code_result(
    mock: &mut MockExecutor,
    sequence: &mut mockall::Sequence,
    launcher: &Path,
    args: &[&str],
    result: std::result::Result<ExecResult, ExecError>,
) {
    let launcher = launcher.to_path_buf();
    let args: Vec<_> = args.iter().map(|arg| (*arg).to_string()).collect();
    mock.expect_execute()
        .once()
        .in_sequence(sequence)
        .withf(move |spec| {
            #[cfg(windows)]
            let command_matches = spec.windows_command_line()
                == Some(format!("\"\"{}\" {}\"", launcher.display(), args.join(" ")).as_str());
            #[cfg(not(windows))]
            let command_matches = spec.program() == launcher.as_os_str()
                && spec
                    .arguments()
                    .iter()
                    .map(|arg| arg.to_string_lossy())
                    .eq(args.iter().map(String::as_str));
            command_matches && !spec.is_checked() && spec.working_dir().is_none()
        })
        .return_once(move |_| result);
}

fn assert_partial_install_record(task: &InstallVsCodeExtensions, ctx: &Context) {
    let output = Arc::new(CapturingOutput::default());
    let status =
        crate::engine::execute(task, &ctx.with_log(Arc::<CapturingOutput>::clone(&output)));
    assert_eq!(status, TaskStatus::Failed);
    let records = output.records();
    let [entry] = records.as_slice() else {
        panic!("expected one task result")
    };
    assert_eq!((entry.actions.applied, entry.actions.failed), (1, 1));
    assert!(
        entry.message.as_deref().unwrap().contains(
            "VS Code did not report these extensions after installation: ms-python.python"
        )
    );
}

#[test]
fn bulk_inventory_deduplicates_case_variants_and_converges() {
    for (dry_run, verified) in [(true, false), (false, true), (false, false)] {
        let root = tempfile::tempdir_in(".").unwrap();
        let launcher = root.path().join("Code Insiders").join(if cfg!(windows) {
            "code-insiders.cmd"
        } else {
            "code-insiders"
        });
        let mut mock = MockExecutor::new();
        let command = if cfg!(windows) {
            "code-insiders.cmd"
        } else {
            "code-insiders"
        };
        let expected_launcher = launcher.clone();
        mock.expect_which()
            .times(if verified { 2 } else { 1 })
            .withf(move |program| program == command)
            .return_const(true);
        mock.expect_which_path()
            .times(if verified { 2 } else { 1 })
            .withf(move |program| program == command)
            .returning(move |_| Ok(expected_launcher.clone()));
        let mut sequence = mockall::Sequence::new();
        expect_code(
            &mut mock,
            &mut sequence,
            &launcher,
            &["--list-extensions"],
            "GITHUB.COPILOT\n",
        );
        if !dry_run {
            expect_code(
                &mut mock,
                &mut sequence,
                &launcher,
                &["--install-extension", "ms-python.python", "--force"],
                "installed",
            );
            expect_code(
                &mut mock,
                &mut sequence,
                &launcher,
                &["--install-extension", "rust-lang.rust-analyzer", "--force"],
                "installed",
            );
            expect_code(
                &mut mock,
                &mut sequence,
                &launcher,
                &["--list-extensions"],
                if verified {
                    "github.copilot\nMS-PYTHON.PYTHON\nrust-lang.rust-analyzer\n"
                } else {
                    "github.copilot\nrust-lang.rust-analyzer\n"
                },
            );
        }
        if verified {
            expect_code(
                &mut mock,
                &mut sequence,
                &launcher,
                &["--list-extensions"],
                "github.copilot\nMS-PYTHON.PYTHON\nrust-lang.rust-analyzer\n",
            );
        }
        let ctx = make_context(
            root.path().to_path_buf(),
            None,
            Platform::new(Os::Linux, false),
            Arc::new(mock),
        )
        .with_dry_run(dry_run)
        .with_parallel(dry_run);
        let task = InstallVsCodeExtensions::new(ConfigHandle::new(vec![
            "GitHub.Copilot".to_string(),
            "github.copilot".to_string(),
            "ms-python.python".to_string(),
            "MS-PYTHON.PYTHON".to_string(),
            "rust-lang.rust-analyzer".to_string(),
        ]));

        if !dry_run && !verified {
            assert_partial_install_record(&task, &ctx);
            continue;
        }
        let first = task.run(&ctx).unwrap();
        assert_eq!(task_batch(&first).changed_count(), 2, "dry_run={dry_run}");
        assert_eq!(
            task_batch(&first).already_ok_count(),
            1,
            "dry_run={dry_run}"
        );
        if verified {
            let repeated = task.run(&ctx).unwrap();
            assert_eq!(task_batch(&repeated).changed_count(), 0);
            assert_eq!(task_batch(&repeated).already_ok_count(), 3);
        }
    }
}

struct Editor {
    mock: MockExecutor,
    launcher: PathBuf,
    seq: mockall::Sequence,
}

impl Editor {
    fn new(root: &Path) -> Self {
        let command = if cfg!(windows) { "code.cmd" } else { "code" };
        let launcher = root.join(command);
        let mut mock = MockExecutor::new();
        mock.expect_which()
            .returning(move |program| program == command);
        mock.expect_which_path()
            .withf(move |p| p == command)
            .returning({
                let launcher = launcher.clone();
                move |_| Ok(launcher.clone())
            });
        Self {
            mock,
            launcher,
            seq: mockall::Sequence::new(),
        }
    }

    fn list(&mut self, result: std::result::Result<ExecResult, ExecError>) {
        expect_code_result(
            &mut self.mock,
            &mut self.seq,
            &self.launcher,
            &["--list-extensions"],
            result,
        );
    }

    fn install(&mut self, id: &str, result: std::result::Result<ExecResult, ExecError>) {
        expect_code_result(
            &mut self.mock,
            &mut self.seq,
            &self.launcher,
            &["--install-extension", id, "--force"],
            result,
        );
    }

    fn context(self, root: &Path) -> Context {
        make_context(
            root.into(),
            None,
            Platform::new(Os::Linux, false),
            Arc::new(self.mock),
        )
        .with_home(root.into())
        .with_env(MapEnv::new().into_handle())
    }
}

fn task_for(ids: &[&str]) -> InstallVsCodeExtensions {
    InstallVsCodeExtensions::new(ConfigHandle::new(
        ids.iter().map(|id| (*id).to_string()).collect(),
    ))
}

#[test]
fn mixed_install_results_report_only_verified_actions_and_retry_missing_ids() {
    for parallel in [false, true] {
        mixed_install_and_retry(parallel);
    }
}

fn mixed_install_and_retry(parallel: bool) {
    let root = tempfile::tempdir_in(".").unwrap();
    let mut editor = Editor::new(root.path());
    for retry in [false, true] {
        editor.list(Ok(ExecResult::success(if retry { "one" } else { "" })));
        for id in if retry {
            &["three", "two"][..]
        } else {
            &["one", "three", "two"][..]
        } {
            let result = if !retry && *id == "three" {
                ExecResult::failure("", "Marketplace rejected three", Some(1))
            } else {
                ExecResult::success("")
            };
            editor.install(id, Ok(result));
        }
        editor.list(Ok(ExecResult::success(if retry {
            "one\ntwo\nthree"
        } else {
            "one\nunrelated"
        })));
    }
    let ctx = editor.context(root.path()).with_parallel(parallel);
    let task = task_for(&["one", "three", "two"]);
    for (retry, expected, applied, failed, names) in [
        (false, TaskStatus::Failed, 1, 2, vec!["one"]),
        (true, TaskStatus::Changed, 2, 0, vec!["three", "two"]),
    ] {
        let output = Arc::new(CapturingOutput::default());
        assert_eq!(
            crate::engine::execute(&task, &ctx.with_log(Arc::<CapturingOutput>::clone(&output))),
            expected
        );
        assert_eq!(
            (
                output.records()[0].actions.applied,
                output.records()[0].actions.failed
            ),
            (applied, failed)
        );
        assert!(
            output
                .actions()
                .iter()
                .all(|action| !action.planned && action.verb == "install")
        );
        assert_eq!(
            output
                .actions()
                .into_iter()
                .map(|action| action.subject)
                .collect::<Vec<_>>(),
            names
        );
        if !retry {
            assert!(
                output
                    .messages()
                    .iter()
                    .any(|(_, s)| s.contains("Marketplace rejected three"))
            );
            assert!(
                output.records()[0]
                    .message
                    .as_deref()
                    .unwrap()
                    .contains("two")
            );
        }
    }
}

#[test]
fn unreadable_inventory_reports_failed_verification_not_missing_ids_and_retry_rediscovers() {
    let root = tempfile::tempdir_in(".").unwrap();
    let mut editor = Editor::new(root.path());
    editor.list(Ok(ExecResult::success("existing")));
    editor.install("one", Ok(ExecResult::success("installed")));
    editor.install(
        "two",
        Ok(ExecResult::failure("", "install denied", Some(1))),
    );
    editor.list(Ok(ExecResult::failure(
        "one",
        "cannot read inventory",
        Some(2),
    )));
    editor.list(Ok(ExecResult::success("existing\none")));
    editor.install("two", Ok(ExecResult::success("")));
    editor.list(Ok(ExecResult::success("one\ntwo\nexisting")));
    let output = Arc::new(CapturingOutput::default());
    let ctx = editor
        .context(root.path())
        .with_log(Arc::<CapturingOutput>::clone(&output));
    let task = task_for(&["existing", "one", "two"]);
    assert_eq!(crate::engine::execute(&task, &ctx), TaskStatus::Failed);
    let record = &output.records()[0];
    let reason = record.message.as_deref().unwrap();
    assert!(
        reason.contains("could not verify")
            && reason.contains("cannot read inventory")
            && reason.contains("install denied"),
        "{reason}"
    );
    assert!(
        !reason.contains("did not report these extensions"),
        "{reason}"
    );
    assert_eq!((record.actions.applied, record.actions.failed), (0, 2));
    assert!(output.actions().is_empty());
    assert_eq!(crate::engine::execute(&task, &ctx), TaskStatus::Changed);
    assert_eq!(output.records()[1].actions.applied, 1);
    assert_eq!(output.actions().len(), 1);
    assert_eq!(output.actions()[0].subject, "two");
}

#[test]
fn listed_extension_does_not_hide_install_error_or_count_twice() {
    let root = tempfile::tempdir_in(".").unwrap();
    let mut editor = Editor::new(root.path());
    editor.list(Ok(ExecResult::success("previously.current")));
    editor.install(
        "new.id",
        Ok(ExecResult::failure("", "install denied", Some(1))),
    );
    editor.list(Ok(ExecResult::success("NEW.ID")));
    let output = Arc::new(CapturingOutput::default());
    let ctx = editor
        .context(root.path())
        .with_log(Arc::<CapturingOutput>::clone(&output));
    let task = task_for(&["previously.current", "new.id"]);
    assert_eq!(crate::engine::execute(&task, &ctx), TaskStatus::Failed);
    let record = &output.records()[0];
    let reason = record.message.as_deref().unwrap();
    assert!(
        reason.contains("listed as installed despite the command failure")
            && reason.contains("install denied"),
        "{reason}"
    );
    assert!(
        reason.contains("did not report these extensions after installation: previously.current"),
        "{reason}"
    );
    assert_eq!((record.actions.applied, record.actions.failed), (0, 2));
    assert!(output.actions().is_empty());
}

#[test]
fn cancellation_keeps_unverified_work_and_unattempted_requests_distinct_from_failures() {
    use crate::engine::{BatchCompletion, BatchReport};
    for during_list in [false, true] {
        for prior_failure in [false, true] {
            let root = tempfile::tempdir_in(".").unwrap();
            let mut editor = Editor::new(root.path());
            let cancel = Err(ExecError::Cancelled {
                command: "code".into(),
                result: ExecResult::failure("", "", None),
            });
            editor.list(Ok(ExecResult::success("existing")));
            editor.install(
                "first",
                Ok(if prior_failure {
                    ExecResult::failure("", "denied", Some(1))
                } else {
                    ExecResult::success("")
                }),
            );
            if during_list {
                editor.install("second", Ok(ExecResult::success("")));
                editor.install("third", Ok(ExecResult::success("")));
                editor.list(cancel);
            } else {
                editor.install("second", cancel);
            }
            let output = Arc::new(CapturingOutput::default());
            let ctx = editor
                .context(root.path())
                .with_log(Arc::<CapturingOutput>::clone(&output));
            let error = task_for(&["existing", "first", "second", "third"])
                .run(&ctx)
                .unwrap_err();
            assert!(error.downcast_ref::<ExecError>().unwrap().is_cancelled());
            let report = error.downcast_ref::<BatchReport>().unwrap();
            let failed = u32::from(prior_failure);
            let unattempted = u32::from(!during_list);
            assert_eq!(
                report.completion(),
                if prior_failure {
                    BatchCompletion::Failed
                } else {
                    BatchCompletion::Interrupted
                }
            );
            assert_eq!(
                (
                    report.stats().already_ok_count(),
                    report.stats().changed_count(),
                    report.stats().failed_count()
                ),
                (1, 0, failed)
            );
            assert_eq!(
                (report.interrupted_count(), report.not_attempted_count()),
                (3 - failed - unattempted, unattempted)
            );
            assert_eq!(format!("{error:#}").contains("denied"), prior_failure);
            assert!(output.actions().is_empty());
        }
    }
}

#[test]
fn should_run_false_when_no_extensions_configured() {
    let ctx = make_linux_context(PathBuf::from("/tmp"), None);
    assert!(!InstallVsCodeExtensions::new(ConfigHandle::new(vec![])).should_run(&ctx));
}

#[test]
fn should_run_true_when_extensions_configured() {
    let ctx = make_linux_context(PathBuf::from("/tmp"), None);
    let task = InstallVsCodeExtensions::new(ConfigHandle::new(vec![ext()]));
    assert!(task.should_run(&ctx));
}

#[test]
fn run_skips_when_vscode_cli_not_found() {
    // Default make_linux_context uses TestExecutor with which_result=false,
    // so find_code_command returns None for both "code-insiders" and "code".
    let ctx = make_linux_context(PathBuf::from("/tmp"), None);
    let task = InstallVsCodeExtensions::new(ConfigHandle::new(vec![ext()]));
    let result = task.run(&ctx).unwrap();
    assert!(
        matches!(
            result,
            TaskResult::Skipped { ref reason, .. }
                if reason.contains("VS Code CLI not found")
        ),
        "expected 'VS Code CLI not found' skip, got {result:?}"
    );
}

#[test]
fn arch_chroot_provisioning_creates_then_preserves_the_first_session_marker() {
    let home = tempfile::tempdir().unwrap();
    let env = MapEnv::new()
        .with(
            crate::infra::provisioning::ENV_VAR,
            crate::infra::provisioning::ARCH_CHROOT,
        )
        .into_handle();
    let ctx = make_context(
        home.path().to_path_buf(),
        None,
        Platform::new(Os::Linux, false),
        Arc::new(MockExecutor::new()),
    )
    .with_home(home.path().to_path_buf())
    .with_env(env);
    let task = InstallVsCodeExtensions::new(ConfigHandle::new(vec![ext()]));
    assert!(
        task.run(&ctx)
            .unwrap_err()
            .to_string()
            .contains("no configured")
    );
    let task = task.with_first_login_profile(Some("base".to_string()));
    assert!(
        task.run(&ctx)
            .unwrap_err()
            .to_string()
            .contains("install the dotfiles-first-login")
    );
    let unit = home
        .path()
        .join(crate::infra::provisioning::FIRST_LOGIN_UNIT_PATH);
    let wants = unit
        .parent()
        .unwrap()
        .join("graphical-session.target.wants");
    std::fs::create_dir_all(&wants).unwrap();
    std::fs::write(&unit, "unit fixture").unwrap();
    crate::infra::fs::create_native_symlink(&unit, &wants.join(unit.file_name().unwrap()), false)
        .unwrap();
    let first = task.run(&ctx).unwrap();
    assert!(
        matches!(first, TaskResult::Batch(ref stats) if stats.changed_count() == 1),
        "first provisioning run should create the marker: {first:?}"
    );
    let marker = home
        .path()
        .join(crate::infra::provisioning::DEFERRED_EXTENSIONS_MARKER);
    assert!(
        std::fs::read_to_string(&marker)
            .unwrap()
            .contains("dotfiles-first-login"),
        "the durable marker must identify its consumer"
    );
    let preserved_content = "pending extensions; preserve first-session notes\n";
    std::fs::write(&marker, preserved_content).unwrap();

    assert_eq!(task_batch(&task.run(&ctx).unwrap()).changed_count(), 1);
    let result = task.run(&ctx).unwrap();

    assert!(
        matches!(
            result,
            TaskResult::Batch(ref stats)
                if stats.already_ok_count() == 1 && stats.changed_count() == 0
        ),
        "rerunning provisioning should preserve the existing deferral: {result:?}"
    );
    let resumed = std::fs::read_to_string(marker).unwrap();
    assert!(resumed.starts_with(preserved_content));
    assert!(resumed.contains("DOTFILES_PROFILE=\"base\"\n"));
}

#[test]
fn arch_chroot_preview_neither_probes_cli_nor_creates_a_marker() {
    let home = tempfile::tempdir_in(".").unwrap();
    let ctx = make_context(
        home.path().to_path_buf(),
        None,
        Platform::new(Os::Linux, false),
        Arc::new(MockExecutor::new()),
    )
    .with_home(home.path().to_path_buf())
    .with_dry_run(true)
    .with_env(
        MapEnv::new()
            .with(
                crate::infra::provisioning::ENV_VAR,
                crate::infra::provisioning::ARCH_CHROOT,
            )
            .into_handle(),
    );
    let task = InstallVsCodeExtensions::new(ConfigHandle::new(vec![ext()]))
        .with_first_login_profile(Some("desktop".to_string()));

    assert_eq!(task_batch(&task.run(&ctx).unwrap()).changed_count(), 1);
    assert!(
        !home.path().join(".local").exists(),
        "preview must not create marker parents"
    );
}
#[test]
fn early_extension_readiness_requires_a_launcher_and_real_session() {
    for (chroot, available) in [(false, false), (false, true), (true, true)] {
        let root = tempfile::tempdir().unwrap();
        let mut mock = MockExecutor::new();
        mock.expect_which()
            .times(if chroot {
                0
            } else if available {
                1
            } else {
                2
            })
            .return_const(available);
        mock.expect_which_path()
            .times(usize::from(!chroot && available))
            .returning(|program| Ok(PathBuf::from(program)));
        let env = MapEnv::new().with(
            crate::infra::provisioning::ENV_VAR,
            if chroot {
                crate::infra::provisioning::ARCH_CHROOT
            } else {
                ""
            },
        );
        let ctx = make_context(
            root.path().into(),
            None,
            Platform::new(Os::Linux, false),
            Arc::new(mock),
        )
        .with_env(env.into_handle());
        let task = InstallVsCodeExtensions::new(ConfigHandle::new(vec![ext()]));
        assert_eq!(
            crate::engine::requirement::ordering_requirements_satisfied(
                &ctx,
                &task.ordering_requirements(),
            ),
            !chroot && available,
            "chroot={chroot}, available={available}"
        );
    }
}
