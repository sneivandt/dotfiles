use super::*;

#[test]
fn workflow_repair_reports_a_typed_action_only_when_state_changed() {
    use crate::test_helpers::CapturingOutput;
    use std::sync::Arc;

    for outcome in [
        FixupOutcome::NoWorkflows,
        FixupOutcome::Quiet,
        FixupOutcome::Unparsed,
        FixupOutcome::Set(3),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let output = Arc::new(CapturingOutput::default());
        let ctx = make_home_context_with_executor(dir.path(), MockExecutor::new());
        let ctx = ctx.with_log(Arc::<CapturingOutput>::clone(&output));
        super::super::outcome::report_fixup_outcome(&ctx, outcome, "");
        let actions = output.actions();
        assert_eq!(actions.len(), usize::from(outcome == FixupOutcome::Set(3)));
        if let Some(action) = actions.first() {
            assert_eq!(action.verb, "restore");
            assert_eq!(action.subject, "3 apm workflow(s)");
            assert!(!action.planned);
            assert_eq!(
                action.message,
                "restore 3 apm workflow(s) to desired automation state"
            );
        }
    }
}

/// Fragment that forces the changed-manifest install path in autopilot tests.
const AUTOPILOT_FIXTURE_FRAGMENT: &str = "name: base\nversion: 1.0.0\ndependencies:\n  apm:\n    - github/awesome-copilot/plugins/project-planning\n";

/// Write a `<home>/.apm/apm.lock.yaml` whose `deployed_files` record `ids` as
/// dotfiles-managed Copilot App workflows, so the autopilot fixup is scoped to
/// exactly those ids.
fn write_workflow_lock(home: &Path, ids: &[&str]) {
    let apm_dir = home.join(".apm");
    std::fs::create_dir_all(&apm_dir).expect("create .apm dir");
    let mut yaml = String::from("dependencies:\n- repo_url: test/pkg\n  deployed_files:\n");
    for id in ids {
        yaml.push_str("  - copilot-app-db://workflows/");
        yaml.push_str(id);
        yaml.push('\n');
    }
    std::fs::write(apm_dir.join("apm.lock.yaml"), yaml).expect("write workflow lock");
}

fn write_autopilot_fixture(home: &Path, ids: &[&str]) -> PathBuf {
    write_home_fragment(home, "base.yml", AUTOPILOT_FIXTURE_FRAGMENT);
    let db_path = write_copilot_app_db(home);
    write_workflow_lock(home, ids);
    db_path
}

fn write_current_workflows(home: &Path, ids: &[&str]) {
    write_current_manifest_and_lock(home);
    write_workflow_lock(home, ids);
    write_copilot_app_db(home);
}

#[test]
fn run_sets_apm_workflows_to_autopilot_after_install() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let ids = ["apm--a", "apm--b", "apm--c"];
    write_autopilot_fixture(dir.path(), &ids);
    assert_install_repairs(dir.path(), &ids, "3 3\napm--a\napm--b\napm--c\n");
}

fn assert_install_repairs(home: &Path, ids: &[&str], repaired: &'static str) {
    let mut run = ApmRun::snapshot(home, ids, ExecResult::success(""));
    run.copilot_install();
    run.workflow_script(
        WORKFLOW_AUTOPILOT_SCRIPT,
        ids,
        ExecResult::success(repaired),
    );
    let ctx = make_home_context_with_executor(home, run.mock);
    let result = install_task().run(&ctx).expect("run should not error");
    assert_task_changed(&result);
}

#[test]
fn run_restores_workflows_when_cowork_reconciliation_fails() {
    for update in [false, true] {
        let dir = tempfile::tempdir_in(".").expect("create fixture directory");
        let home = dir.path().canonicalize().expect("resolve fixture home");
        write_current_manifest_and_lock(&home);
        write_copilot_app_db(&home);
        std::fs::write(
            home.join(".apm").join("apm.lock.yaml"),
            "dependencies:\n- deployed_files:\n  - copilot-app-db://workflows/apm--a\n  \
             - .agents/skills/missing/SKILL.md\n",
        )
        .unwrap();

        let mut run = ApmRun::snapshot(&home, &["apm--a"], ExecResult::success("apm--a\n"));
        if update {
            run.copilot_update("updated\n");
        } else {
            run.copilot_install();
        }
        run.enable("copilot-cowork");
        run.workflow_script(
            WORKFLOW_AUTOPILOT_SCRIPT,
            &["apm--a"],
            ExecResult::success("1 1\napm--a\n"),
        );
        let ctx = make_windows_cowork_context(&home, run.mock);
        let task = if update {
            update_task()
        } else {
            install_task()
        };

        let error = task
            .run(&ctx)
            .expect_err("Cowork failure must still propagate");
        let message = format!("{error:#}");
        assert!(
            message.contains("APM shared skill") && message.contains("missing"),
            "{message}"
        );
    }
}

#[test]
fn run_restores_workflows_without_masking_native_failure_outcomes() {
    for (auth_failure, repair_failure) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        let dir = tempfile::tempdir_in(".").expect("create fixture directory");
        write_autopilot_fixture(dir.path(), &["apm--a"]);
        let mut run = ApmRun::snapshot(dir.path(), &["apm--a"], ExecResult::success("apm--a\n"));
        let message = if auth_failure {
            "authentication failed"
        } else {
            "deployment failed"
        };
        run.command(INSTALL, move |_| {
            Err(ExecError::non_zero(
                "apm",
                ExecResult::failure("", message, Some(1)),
            ))
        });
        let repair = if repair_failure {
            ExecResult::failure("", "database is locked", Some(1))
        } else {
            ExecResult::success("1 1\napm--a\n")
        };
        run.workflow_script(WORKFLOW_AUTOPILOT_SCRIPT, &["apm--a"], repair);
        let ctx = make_home_context_with_executor(dir.path(), run.mock);
        let result = install_task().run(&ctx);

        if auth_failure {
            assert!(task_skipped(&result.unwrap()).contains("GitHub authentication"));
        } else {
            assert!(format!("{:#}", result.unwrap_err()).contains(message));
        }
    }
}

#[test]
fn run_warns_when_python_missing_for_autopilot_fixup() {
    for strict in [false, true] {
        let dir = tempfile::tempdir().expect("create temp dir");
        write_autopilot_fixture(dir.path(), &["apm--a"]);

        let mut run = ApmRun::new(dir.path(), 1);
        run.copilot_install();
        // Probed twice (pre-install snapshot + post-install fixup); both fall
        // back to `python` and then give up, so neither runs a query.
        expect_python3(&mut run.mock, 2, false);
        run.mock
            .expect_which()
            .with(mockall::predicate::eq("python"))
            .times(2)
            .returning(|_| false);

        let ctx =
            make_home_context_with_executor(dir.path(), run.mock).with_require_complete(strict);
        if strict {
            assert_eq!(
                crate::engine::execute(&install_task(), &ctx),
                crate::infra::logging::TaskStatus::Failed
            );
        } else {
            let result = install_task().run(&ctx).expect("run should not error");
            assert!(matches!(
                result,
                crate::engine::TaskResult::Skipped {
                    kind: crate::engine::SkipKind::UnmetWork,
                    ..
                }
            ));
        }
    }
}

#[test]
fn run_fails_on_degraded_workflow_db_cases() {
    for message in ["database is locked", "no such column: mode"] {
        let dir = tempfile::tempdir().expect("create temp dir");
        write_autopilot_fixture(dir.path(), &["apm--a"]);

        let failure = || ExecResult::failure("", message, Some(1));
        let mut run = ApmRun::snapshot(dir.path(), &["apm--a"], failure());
        run.copilot_install();
        run.workflow_script(WORKFLOW_AUTOPILOT_SCRIPT, &["apm--a"], failure());
        let ctx = make_home_context_with_executor(dir.path(), run.mock);
        let error = install_task()
            .run(&ctx)
            .expect_err("failed repair must fail the task");
        assert!(
            format!("{error:#}").contains("workflow repair failed"),
            "{error:#}"
        );
    }
}

#[test]
fn run_skips_autopilot_fixup_when_lock_lists_no_workflows() {
    // The common case: the deployed deps ship only agents/skills, so the
    // lockfile records no `copilot-app-db://workflows/` entries. The fixup must
    // scope to zero ids and skip entirely -- never probing python or touching
    // the database -- even though `~/.copilot/data.db` exists.
    let dir = tempfile::tempdir().expect("create temp dir");
    write_home_fragment(dir.path(), "base.yml", AUTOPILOT_FIXTURE_FRAGMENT);
    write_copilot_app_db(dir.path());
    std::fs::write(
        dir.path().join(".apm").join("apm.lock.yaml"),
        "dependencies:\n- repo_url: test/pkg\n  deployed_files:\n  - .agents/skills/foo\n",
    )
    .expect("write lock without workflows");

    // Only the apm install runs; no python probe is queued, so the mock would
    // panic on any unexpected `which("python3")`/`execute` call.
    let mut run = ApmRun::new(dir.path(), 1);
    run.copilot_install();

    let ctx = make_home_context_with_executor(dir.path(), run.mock);
    let result = install_task().run(&ctx).expect("run should not error");
    assert_task_changed(&result);
}

#[test]
fn current_install_delegates_to_apm_and_repairs_autopilot_drift() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_workflows(dir.path(), &["apm--a"]);
    assert_install_repairs(dir.path(), &["apm--a"], "1 1\napm--a\n");
}

#[test]
fn current_install_previews_autopilot_drift_without_repairing_it() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_workflows(dir.path(), &["apm--a"]);

    let ctx = make_home_context_with_executor(dir.path(), MockExecutor::new()).with_dry_run(true);
    let result = install_task().run(&ctx).expect("run should not error");

    assert_task_changed(&result);
}

#[test]
fn update_re_arms_apm_workflows_cases() {
    // Regardless of whether `apm update` advances the lock ("updated\n") or
    // reports no changes (cached), it can redeploy a workflow disabled, so
    // the post-update fixup must run defensively on both paths and re-arm it.
    let cases = [
        (
            "apm update advances the lock: workflow not desired pre-update",
            "",
            "updated\n",
            true,
        ),
        (
            "apm update reports no changes: workflow already desired pre-update",
            "apm--a\n",
            "  [+] github.com/example/plugin (cached)\n",
            false,
        ),
    ];
    for (case, pre_stdout, update_stdout, expected_changed) in cases {
        let dir = tempfile::tempdir().expect("create temp dir");
        write_current_workflows(dir.path(), &["apm--a"]);
        let mut run = ApmRun::snapshot(dir.path(), &["apm--a"], ExecResult::success(pre_stdout));
        run.copilot_update(update_stdout);
        run.workflow_script(
            WORKFLOW_AUTOPILOT_SCRIPT,
            &["apm--a"],
            ExecResult::success("1 1\napm--a\n"),
        );
        let ctx = make_home_context_with_executor(dir.path(), run.mock);
        let result = update_task().run(&ctx).expect(case);
        if expected_changed {
            assert_task_changed(&result);
        } else {
            assert_task_ok(&result);
        }
    }
}

#[test]
fn decide_fixup_outcome_cases() {
    let cases: Vec<(&str, &str, DesiredApmWorkflows, FixupOutcome)> = vec![
        (
            "quiet in steady state: the pre-install workflows are already \
             desired, so the install-reset set matches with no net change",
            "3 3\napm--a\napm--b\napm--c\n",
            DesiredApmWorkflows::Known(id_set(&["apm--a", "apm--b", "apm--c"])),
            FixupOutcome::Quiet,
        ),
        (
            "sets all workflows on first install",
            "3 3\napm--a\napm--b\napm--c\n",
            DesiredApmWorkflows::FirstInstall,
            FixupOutcome::Set(3),
        ),
        (
            "sets one when a workflow was newly added",
            "3 3\napm--a\napm--b\napm--c\n",
            DesiredApmWorkflows::Known(id_set(&["apm--a", "apm--b"])),
            FixupOutcome::Set(1),
        ),
        (
            "sets one when the user disabled apm--b pre-install and the fixup \
             re-enables it",
            "3 3\napm--a\napm--b\napm--c\n",
            DesiredApmWorkflows::Known(id_set(&["apm--a", "apm--c"])),
            FixupOutcome::Set(1),
        ),
        (
            "stays quiet when a pre-install workflow is gone post-install, since \
             the post set is a subset of pre and the forward diff is zero",
            "2 2\napm--a\napm--b\n",
            DesiredApmWorkflows::Known(id_set(&["apm--a", "apm--b", "apm--c"])),
            FixupOutcome::Quiet,
        ),
        (
            "reports no workflows when the header lists zero",
            "0 0\n",
            DesiredApmWorkflows::FirstInstall,
            FixupOutcome::NoWorkflows,
        ),
        (
            "stays quiet without a trustworthy pre-install snapshot, rather than \
             emit a spurious \"set N\" line",
            "3 3\napm--a\napm--b\napm--c\n",
            DesiredApmWorkflows::Unavailable,
            FixupOutcome::Quiet,
        ),
        (
            "reports unparsed on a non-numeric header",
            "not-a-number\n",
            DesiredApmWorkflows::FirstInstall,
            FixupOutcome::Unparsed,
        ),
        (
            "reports unparsed on a three-token header",
            "3 3 extra\napm--a\n",
            DesiredApmWorkflows::FirstInstall,
            FixupOutcome::Unparsed,
        ),
    ];
    for (case, stdout, pre, expected) in cases {
        assert_eq!(decide_fixup_outcome(stdout, &pre), expected, "case: {case}");
    }
}

#[test]
fn interpret_fixup_result_classifies_script_failures() {
    let pre = DesiredApmWorkflows::Unavailable;
    assert_eq!(
        interpret_fixup_result(ExecResult::failure("", "database is locked", Some(1)), &pre,),
        FixupExecution::Failed(FixupFailure::DatabaseLocked)
    );
    assert_eq!(
        interpret_fixup_result(
            ExecResult::failure("", "no such table: workflows", Some(1)),
            &pre,
        ),
        FixupExecution::Failed(FixupFailure::WorkflowsTableMissing)
    );
    assert_eq!(
        interpret_fixup_result(
            ExecResult::failure("", "no such column: mode", Some(1)),
            &pre,
        ),
        FixupExecution::Failed(FixupFailure::SchemaDrift(
            "no such column: mode".to_string()
        ))
    );
    assert_eq!(
        interpret_fixup_result(ExecResult::failure("", "permission denied", Some(1)), &pre,),
        FixupExecution::Failed(FixupFailure::Other("permission denied".to_string()))
    );
}
