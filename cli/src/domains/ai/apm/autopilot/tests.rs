use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use crate::engine::Task;
use crate::infra::exec::{ExecError, ExecResult, MockExecutor};
use crate::test_helpers::{assert_task_changed, assert_task_ok, task_skipped};

use super::super::test_fixture::{
    ApmRun, INSTALL, install_task, make_context_with_home, make_home_context_with_executor,
    make_windows_cowork_context, update_task, write_copilot_app_db,
    write_current_manifest_and_lock, write_home_fragment,
};
use super::DesiredApmWorkflows;
use crate::engine::Context;

use super::lockfile::parse_deployed_workflow_ids;
use super::outcome::{
    FixupExecution, FixupFailure, FixupOutcome, decide_fixup_outcome, interpret_fixup_result,
};
use super::scripts::{
    WORKFLOW_AUTOPILOT_SCRIPT, WORKFLOW_DESIRED_IDS_SCRIPT, WORKFLOW_PRESENT_IDS_SCRIPT,
    build_workflow_script_args, parse_autopilot_result, parse_desired_ids,
};

fn id_set(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|s| (*s).to_string()).collect()
}

fn expect_python3(mock: &mut MockExecutor, times: usize, found: bool) {
    mock.expect_which()
        .with(mockall::predicate::eq("python3"))
        .times(times)
        .returning(move |_| found);
}

fn expect_python3_without_scope_probe(mock: &mut MockExecutor, times: usize) {
    expect_python3(mock, times, true);
    mock.expect_execute()
        .withf(|spec| {
            spec.arguments()
                .get(1)
                .is_some_and(|script| script == WORKFLOW_PRESENT_IDS_SCRIPT)
        })
        .never();
}

impl<'a> ApmRun<'a> {
    fn snapshot(home: &'a Path, ids: &[&str], result: ExecResult) -> Self {
        let mut run = Self::new(home, 1);
        expect_python3_without_scope_probe(&mut run.mock, 2);
        run.workflow_script(WORKFLOW_DESIRED_IDS_SCRIPT, ids, result);
        run
    }

    fn workflow_script(&mut self, script: &str, ids: &[&str], result: ExecResult) {
        let home = self.home.to_owned();
        let db = home.join(".copilot").join("data.db");
        let args: Vec<_> = ["-c", script, db.to_str().unwrap()]
            .into_iter()
            .chain(ids.iter().copied())
            .map(std::ffi::OsString::from)
            .collect();
        self.mock
            .expect_execute()
            .once()
            .in_sequence(&mut self.seq)
            .return_once(move |spec| {
                assert_eq!(spec.program(), "python3");
                assert_eq!(spec.working_dir(), Some(home.as_path()));
                assert!(!spec.is_checked());
                assert_eq!(spec.arguments(), args);
                Ok(result)
            });
    }
}

type AutopilotResultCase = (&'static str, &'static str, Option<(u64, HashSet<String>)>);

fn python_for_script_tests() -> Option<&'static str> {
    ["python3", "python"].into_iter().find(|candidate| {
        std::process::Command::new(candidate)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    })
}

fn run_python_script_unchecked(python: &str, script: &str, args: &[&str]) -> std::process::Output {
    std::process::Command::new(python)
        .arg("-c")
        .arg(script)
        .args(args)
        .output()
        .expect("run python script")
}

fn run_python_script(python: &str, script: &str, args: &[&str]) -> std::process::Output {
    let output = run_python_script_unchecked(python, script, args);
    assert!(
        output.status.success(),
        "Python fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn write_local_cron_prompt(prompt: &Path, cron: Option<&str>) {
    let metadata = cron.map_or_else(String::new, |cron| format!("cron_expression: \"{cron}\"\n"));
    std::fs::write(
        prompt,
        format!("---\ninterval: manual\n{metadata}---\nFixture review prompt"),
    )
    .unwrap();
}

struct LocalCron {
    home: tempfile::TempDir,
    python: &'static str,
    prompt: PathBuf,
    db: PathBuf,
}

#[derive(Clone, Copy, Debug)]
enum WorkflowStep {
    Scope,
    Snapshot,
    Repair,
    InvalidRepair,
}

impl LocalCron {
    fn new() -> Option<Self> {
        let python = python_for_script_tests()?;
        let home = tempfile::tempdir().expect("create cron fixture home");
        let prompts = home
            .path()
            .join(".apm")
            .join("plugins")
            .join("fixture")
            .join(".apm")
            .join("prompts");
        std::fs::create_dir_all(&prompts).unwrap();
        let prompt = prompts.join("review.prompt.md");
        write_local_cron_prompt(&prompt, Some("15 7,11,15 * * 1-5"));
        std::fs::write(
            home.path().join(".apm").join("apm.lock.yaml"),
            "dependencies:\n- local_path: ~/.apm/plugins/fixture\n  deployed_files:\n  \
         - copilot-app-db://workflows/apm--_local--fixture--review\n  \
         - copilot-app-db://workflows/apm--_local--fixture--removed\n",
        )
        .unwrap();
        std::fs::create_dir(home.path().join(".copilot")).unwrap();
        let db_path = home.path().join(".copilot").join("data.db");
        let db = db_path.to_str().unwrap();
        run_python_script(
            python,
            r#"
import sqlite3, sys
con = sqlite3.connect(sys.argv[1])
con.execute("CREATE TABLE workflows (id TEXT PRIMARY KEY, name TEXT, prompt TEXT, mode TEXT, enabled INTEGER, interval TEXT, schedule_hour INTEGER, schedule_minute INTEGER, schedule_day INTEGER, next_run_at TEXT, cron_expression TEXT)")
con.executemany(
    "INSERT INTO workflows VALUES (?, 'Review', 'prompt', 'interactive', 0, 'manual', 9, 0, 1, NULL, NULL)",
    [("apm--_local--fixture--review",), ("apm--foreign--fixture--review",)],
)
con.commit()
"#,
            &[db],
        );

        Some(Self {
            home,
            python,
            prompt,
            db: db_path,
        })
    }

    fn row(&self) -> serde_json::Value {
        let output = run_python_script(
            self.python,
            r#"
import json, sqlite3, sys
from datetime import datetime
con = sqlite3.connect(sys.argv[1])
row = con.execute(
    "SELECT mode, enabled, interval, cron_expression, next_run_at FROM workflows WHERE id=?",
    ("apm--_local--fixture--review",),
).fetchone()
if row[3]:
    next_local = datetime.fromisoformat(row[4].replace("Z", "+00:00")).astimezone()
    fields = row[3].split()
    assert next_local > datetime.now().astimezone(), row
    assert next_local.minute == int(fields[0]), row
    assert next_local.hour in {int(hour) for hour in fields[1].split(",")}, row
    assert next_local.isoweekday() <= 5, row
print(json.dumps(row))
assert con.execute(
    "SELECT mode, enabled, cron_expression, next_run_at FROM workflows WHERE id=?",
    ("apm--foreign--fixture--review",),
).fetchone() == ("interactive", 0, None, None)
"#,
            &[self.db.to_str().unwrap()],
        );

        serde_json::from_slice(&output.stdout).unwrap()
    }

    /// Each operation looks up Python once. With stale sources it also probes
    /// the surviving rows before running the requested snapshot or repair.
    fn context(&self, scope_first: bool, steps: &[WorkflowStep]) -> Context {
        use WorkflowStep::{InvalidRepair, Repair, Scope, Snapshot};
        let mut mock = MockExecutor::new();
        expect_python3(&mut mock, steps.len(), true);
        let mut seq = mockall::Sequence::new();
        for &operation in steps {
            for step in scope_first.then_some(Scope).into_iter().chain([operation]) {
                let script = match step {
                    Scope => WORKFLOW_PRESENT_IDS_SCRIPT,
                    Snapshot => WORKFLOW_DESIRED_IDS_SCRIPT,
                    Repair | InvalidRepair => WORKFLOW_AUTOPILOT_SCRIPT,
                };
                let python = self.python;
                let home = self.home.path().to_owned();
                let db = self.db.clone();
                mock.expect_execute()
                    .once()
                    .in_sequence(&mut seq)
                    .return_once(move |spec| {
                        assert_eq!(spec.program(), "python3");
                        assert_eq!(spec.working_dir(), Some(home.as_path()));
                        assert!(!spec.is_checked());
                        let args = spec.arguments();
                        assert_eq!(args[0], "-c");
                        assert_eq!(args[1], script, "unexpected workflow script for {step:?}");
                        assert_eq!(args[2], db.as_os_str());
                        let output = std::process::Command::new(python)
                            .args(args)
                            .output()
                            .unwrap();
                        assert_eq!(
                            output.status.success(),
                            !matches!(step, InvalidRepair),
                            "{step:?}: {output:?}"
                        );
                        let stdout = String::from_utf8(output.stdout).unwrap();
                        let stderr = String::from_utf8(output.stderr).unwrap();
                        Ok(if output.status.success() {
                            ExecResult::success(stdout)
                        } else {
                            ExecResult::failure(stdout, stderr, output.status.code())
                        })
                    });
            }
        }
        make_context_with_home(
            self.home.path(),
            crate::infra::platform::Platform::new(crate::infra::platform::Os::Windows, false),
            mock,
        )
    }
}

mod lock;
mod repair;
mod schedules;
mod scripts;
