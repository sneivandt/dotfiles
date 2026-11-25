//! Update and preview tests for native APM ownership.

use super::*;

#[test]
fn update_then_install_reports_exact_lock_changes() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_and_lock(dir.path());
    std::fs::write(
        dir.path().join(".apm").join("apm.lock.yaml"),
        "dependencies:\n- repo_url: example/plugin\n  name: example-plugin\n  \
         resolved_commit: 1111111aaaa\n",
    )
    .expect("seed lock");
    let mut run = ApmRun::new(dir.path(), 1);
    run.command(UPDATE, |home| {
        std::fs::write(
            home.join(".apm/apm.lock.yaml"),
            "dependencies:\n- repo_url: example/plugin\n  name: example-plugin\n  \
                 resolved_commit: 2222222bbbb\n  deployed_files: [new/SKILL.md]\n",
        )
        .expect("advance lock");
        Ok(ExecResult::success("updated\n"))
    });
    run.ok(INSTALL, "installed\n");
    let (log, _tmp, _guard) = logging::isolated_logger();
    let log = Arc::new(log);
    let log_output: Arc<dyn Log> = Arc::<Logger>::clone(&log);
    let ctx = linux_context(dir.path(), run.mock).with_log(log_output);

    let result = update_task().run(&ctx).expect("run update");

    assert_task_changed(&result);
    assert_eq!(
        task_batch(&result).message(),
        Some("updated 1 agent package")
    );
    let contents = std::fs::read_to_string(log.log_path().expect("log path")).expect("read log");
    assert!(
        contents.lines().any(|line| {
            let Some(logging::records::StoredRecord {
                record:
                    logging::records::Record::Action {
                        verb,
                        subject,
                        planned,
                        message,
                        ..
                    },
                ..
            }) = logging::records::StoredRecord::from_line(line)
            else {
                return false;
            };
            verb == "update"
                && !planned
                && subject == "example-plugin · 1111111 -> 2222222"
                && message == "update example-plugin · 1111111 -> 2222222"
        }),
        "agent package change should be emitted as task detail: {contents}"
    );
    assert!(
        !contents.contains("APM lock state"),
        "dependency-level detail should stand on its own: {contents}"
    );
}

#[test]
fn update_writes_generated_manifest_then_updates_and_installs() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_default_home_fragment(dir.path());
    let mut run = ApmRun::new(dir.path(), 1);
    run.command(UPDATE, |home| {
        let manifest = std::fs::read_to_string(home.join(".apm/apm.yml"))
            .expect("read generated manifest before update");
        assert!(manifest.contains("example/plugin"));
        Ok(ExecResult::success("updated\n"))
    });
    run.ok(INSTALL, "installed\n");
    let ctx = linux_context(dir.path(), run.mock);

    assert_task_changed(&update_task().run(&ctx).expect("run update"));
}

#[test]
fn update_reconciles_renamed_local_skills_when_remote_refs_are_current() {
    let dir = tempfile::tempdir().unwrap();
    write_current_manifest_and_lock(dir.path());
    let source = dir
        .path()
        .join("plugin/.apm/skills/agent-customization/SKILL.md");
    let old = dir.path().join(".agents/skills/apm-dotfiles/SKILL.md");
    let deployed = dir
        .path()
        .join(".agents/skills/agent-customization/SKILL.md");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    std::fs::write(&source, "renamed local skill").unwrap();
    std::fs::write(&old, "old local skill").unwrap();
    let mut run = ApmRun::new(dir.path(), 2);
    for _ in 0..2 {
        run.ok(UPDATE, REFS_CURRENT);
        let source = source.clone();
        let old = old.clone();
        let deployed = deployed.clone();
        run.command(INSTALL, move |home| {
            assert_eq!(source.strip_prefix(home).unwrap(), Path::new("plugin/.apm/skills/agent-customization/SKILL.md"));
            // Model native install reconciling local content and stale ownership.
            std::fs::create_dir_all(deployed.parent().unwrap()).unwrap();
            std::fs::copy(&source, &deployed).unwrap();
            if old.exists() {
                std::fs::remove_file(&old).unwrap();
            }
            std::fs::write(home.join(".apm/apm.lock.yaml"),
                "dependencies:\n- name: dot-skill\n  deployed_files: [.agents/skills/agent-customization/SKILL.md]\n").unwrap();
            Ok(ExecResult::success("installed\n"))
        });
    }
    let ctx = linux_context(dir.path(), run.mock);
    assert_task_changed(&update_task().run(&ctx).unwrap());
    assert!(!old.exists());
    assert_eq!(
        std::fs::read(&deployed).unwrap(),
        std::fs::read(&source).unwrap()
    );
    assert_task_ok(&update_task().run(&ctx).unwrap());
}

#[test]
fn update_does_not_report_current_when_followup_install_fails() {
    for message in ["archive extraction failed", "fatal: Authentication failed"] {
        let dir = tempfile::tempdir_in(".").unwrap();
        write_current_manifest_and_lock(dir.path());
        let mut run = ApmRun::new(dir.path(), 1);
        run.ok(UPDATE, REFS_CURRENT);
        run.command(INSTALL, move |_| Err(command_failure(message)));
        let ctx = make_windows_cowork_context(dir.path(), run.mock);
        let result = update_task().run(&ctx);
        if message.contains("Authentication") {
            assert!(matches!(
                result.unwrap(),
                TaskResult::Skipped {
                    reason,
                    kind: crate::engine::SkipKind::UnmetWork,
                } if reason.contains("GitHub")
            ));
        } else {
            assert!(format!("{:#}", result.unwrap_err()).contains(message));
        }
        assert!(
            !dir.path().join("OneDrive - Test").exists(),
            "failed follow-up install must not start Cowork reconciliation"
        );
    }
}

#[test]
fn update_reports_ok_when_lock_is_byte_identical() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_and_lock(dir.path());
    let mut run = ApmRun::new(dir.path(), 1);
    run.ok(UPDATE, REFS_CURRENT);
    run.ok(INSTALL, "installed\n");
    let ctx = linux_context(dir.path(), run.mock);

    assert_task_ok(&update_task().run(&ctx).expect("run update"));
}

#[test]
fn single_target_previews_restore_workflows_only_for_copilot_app() {
    use super::commands::ApmCommand;
    use super::managed_targets::ManagedTargets;

    for command in [ApmCommand::Install, ApmCommand::Update] {
        for copilot_app in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = if copilot_app {
                write_copilot_app_db(dir.path());
                linux_context(dir.path(), MockExecutor::new())
            } else {
                make_windows_cowork_context(dir.path(), MockExecutor::new())
            };
            let output = Arc::new(CapturingOutput::default());
            let ctx = ctx
                .with_dry_run(true)
                .with_log(Arc::<CapturingOutput>::clone(&output));

            ManagedTargets::detect(&ctx).unwrap().preview(&ctx, command);

            let actions = output.actions();
            let verbs: Vec<_> = actions.iter().map(|action| action.verb.as_str()).collect();
            let expected = if copilot_app {
                vec!["run", "restore"]
            } else if matches!(command, ApmCommand::Install) {
                vec!["sync"]
            } else {
                vec!["redeploy"]
            };
            assert_eq!(verbs, expected, "{command:?}, copilot_app={copilot_app}");
            assert!(actions.iter().all(|action| action.planned));
        }
    }
}

#[test]
fn update_dry_run_uses_native_apm_plan() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_current_manifest_and_lock(dir.path());
    let manifest_path = dir.path().join(".apm/apm.yml");
    let lock_path = dir.path().join(".apm/apm.lock.yaml");
    let manifest_before = std::fs::read(&manifest_path).unwrap();
    let lock_before = std::fs::read(&lock_path).unwrap();
    let mut run = ApmRun::new(dir.path(), 1);
    run.ok(PREVIEW_UPDATE, "would update example/plugin\n");
    let output = Arc::new(CapturingOutput::default());
    let ctx = linux_context(dir.path(), run.mock)
        .with_dry_run(true)
        .with_log(Arc::<CapturingOutput>::clone(&output));

    assert!(matches!(
        update_task().run(&ctx).expect("preview update"),
        TaskResult::DryRun
    ));
    assert_eq!(std::fs::read(manifest_path).unwrap(), manifest_before);
    assert_eq!(std::fs::read(lock_path).unwrap(), lock_before);
    let actions = output.actions();
    assert_eq!(
        actions
            .iter()
            .map(|action| (
                action.verb.as_str(),
                action.subject.as_str(),
                action.planned
            ))
            .collect::<Vec<_>>(),
        [
            ("use", "APM's update plan", true),
            ("run", "apm install -g", true)
        ]
    );
    for action in &actions {
        assert!(
            action
                .message
                .starts_with(&format!("{} {} ", action.verb, action.subject))
        );
    }
}

#[test]
fn stale_or_missing_manifest_previews_preserve_all_managed_target_state() {
    for update in [false, true] {
        let previous_owned = format!("{GENERATED_HEADER}name: previous\n");
        for manifest in [None, Some(previous_owned.as_str())] {
            let dir = tempfile::tempdir_in(".").unwrap();
            write_default_home_fragment(dir.path());
            let manifest_path = dir.path().join(".apm/apm.yml");
            if let Some(manifest) = manifest {
                std::fs::write(&manifest_path, manifest).unwrap();
            }
            let db_path = write_copilot_app_db(dir.path());
            let lock_path = dir.path().join(".apm/apm.lock.yaml");
            std::fs::write(
                &lock_path,
                "# retain legacy evidence\n\
                 dependencies:\n- deployed_files: [cowork://skills/example/SKILL.md]\n",
            )
            .unwrap();
            let target = dir.path().join("OneDrive - Test/Documents/Cowork/skills");
            std::fs::create_dir_all(target.join("example")).unwrap();
            let skill_path = target.join("example/SKILL.md");
            std::fs::write(&skill_path, "stale deployment\n").unwrap();
            let ownership_path = target.join(".dotfiles-apm-skills.json");
            std::fs::write(&ownership_path, "[ \"example\" ]\n").unwrap();
            let snapshots: Vec<_> = [
                dir.path().join(".apm/config/base.yml"),
                lock_path,
                db_path,
                skill_path,
                ownership_path,
            ]
            .into_iter()
            .map(|path| {
                let bytes = std::fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect();
            let mut mock = MockExecutor::new();
            if update {
                expect_which_apm(&mut mock, true);
            }
            let output = Arc::new(CapturingOutput::default());
            let ctx = make_windows_cowork_context(dir.path(), mock)
                .with_dry_run(true)
                .with_log(Arc::<CapturingOutput>::clone(&output));
            let task = if update {
                update_task()
            } else {
                install_task()
            };

            assert_task_changed(&task.run(&ctx).expect("preview stale or missing manifest"));
            let actions = output.actions();
            let mut verbs: Vec<_> = actions.iter().map(|action| action.verb.as_str()).collect();
            verbs.sort_unstable();
            assert_eq!(
                verbs,
                if update {
                    ["merge", "redeploy", "restore", "run", "run"]
                } else {
                    ["merge", "restore", "run", "run", "sync"]
                }
            );
            assert_eq!(
                actions.len(),
                output.messages_of(logging::MsgKind::DryRun).len()
            );
            for action in &actions {
                assert!(action.planned);
                assert!(
                    action
                        .message
                        .starts_with(&format!("{} {} ", action.verb, action.subject))
                );
            }

            if let Some(manifest) = manifest {
                assert_eq!(std::fs::read(&manifest_path).unwrap(), manifest.as_bytes());
            } else {
                assert!(!manifest_path.try_exists().unwrap());
            }
            for (path, before) in snapshots {
                assert_eq!(std::fs::read(&path).unwrap(), before, "{}", path.display());
            }
        }
    }
}

#[test]
fn update_preview_failure_preserves_files_and_failure_classification() {
    for failure in [
        "authentication failed",
        "archive extraction failed",
        "cancelled",
    ] {
        let dir = tempfile::tempdir_in(".").unwrap();
        write_current_manifest_and_lock(dir.path());
        let manifest_path = dir.path().join(".apm/apm.yml");
        let lock_path = dir.path().join(".apm/apm.lock.yaml");
        let manifest_before = std::fs::read(&manifest_path).unwrap();
        let lock_before = std::fs::read(&lock_path).unwrap();
        let mut run = ApmRun::new(dir.path(), 1);
        run.command(PREVIEW_UPDATE, move |_| {
            Err(if failure == "cancelled" {
                ExecError::Cancelled {
                    command: "apm".into(),
                    result: ExecResult::failure("", "authentication required", None),
                }
            } else {
                command_failure(failure)
            })
        });
        let ctx = linux_context(dir.path(), run.mock).with_dry_run(true);

        let result = update_task().run(&ctx);

        match failure {
            "authentication failed" => assert!(matches!(
                result.unwrap(),
                TaskResult::Skipped {
                    reason,
                    kind: crate::engine::SkipKind::UnmetWork,
                } if reason.contains("apm update requires GitHub authentication")
            )),
            "cancelled" => assert!(
                result
                    .unwrap_err()
                    .downcast_ref::<ExecError>()
                    .is_some_and(ExecError::is_cancelled)
            ),
            _ => assert!(format!("{:#}", result.unwrap_err()).contains(failure)),
        }
        assert_eq!(std::fs::read(&manifest_path).unwrap(), manifest_before);
        assert_eq!(std::fs::read(&lock_path).unwrap(), lock_before);
    }
}
