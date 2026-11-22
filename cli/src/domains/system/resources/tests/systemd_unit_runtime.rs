use super::*;

#[test]
fn runtime_state_handles_daemons_and_completed_oneshots() {
    for (label, enabled, properties, correct) in [
        ("running daemon", true, "ActiveState=active", true),
        ("stopped daemon", true, "ActiveState=inactive", false),
        ("failed daemon", true, "ActiveState=failed", false),
        ("starting daemon", true, "ActiveState=activating", true),
        ("reloading daemon", true, "ActiveState=reloading", true),
        ("refreshing daemon", true, "ActiveState=refreshing", true),
        ("stopping daemon", true, "ActiveState=deactivating", false),
        (
            "disabled running daemon",
            false,
            "ActiveState=active",
            false,
        ),
        (
            "disabled starting daemon",
            false,
            "ActiveState=activating",
            false,
        ),
        (
            "disabled stopping daemon",
            false,
            "ActiveState=deactivating",
            false,
        ),
        (
            "disabled stopped daemon",
            false,
            "ActiveState=inactive",
            true,
        ),
        ("disabled failed daemon", false, "ActiveState=failed", true),
        (
            "completed oneshot",
            true,
            "ActiveState=inactive\nType=oneshot\nResult=success\nExecMainStartTimestampMonotonic=123",
            true,
        ),
        (
            "unstarted oneshot",
            true,
            "ActiveState=inactive\nType=oneshot\nResult=success\nExecMainStartTimestampMonotonic=0\nConditionResult=yes",
            false,
        ),
        (
            "failed oneshot",
            true,
            "ActiveState=failed\nType=oneshot\nResult=exit-code\nExecMainStartTimestampMonotonic=123",
            false,
        ),
        (
            "oneshot missing history",
            true,
            "ActiveState=inactive\nType=oneshot\nResult=success",
            false,
        ),
    ] {
        let state = runtime_state(enabled, properties);
        assert_eq!(
            matches!(state, ResourceState::Correct),
            correct,
            "{label}: {state:?}"
        );
        if !correct {
            let active = properties
                .lines()
                .next()
                .unwrap()
                .strip_prefix("ActiveState=")
                .unwrap();
            assert_eq!(
                state,
                ResourceState::Incorrect {
                    current: format!("runtime state is {active}")
                },
                "{label}"
            );
        }
    }
    for properties in ["", "ActiveState=unexpected"] {
        assert!(matches!(
            runtime_state(true, properties),
            ResourceState::Unknown { .. }
        ));
    }
}

#[test]
fn runtime_state_retries_condition_skips_without_hiding_stopped_or_failed_units() {
    for (label, properties, expected) in [
        (
            "condition-skipped timer",
            "ActiveState=inactive\nConditionResult=no\nResult=success",
            ResourceState::Incorrect {
                current: "previous start was skipped by a condition".into(),
            },
        ),
        (
            "condition-skipped daemon",
            "ActiveState=inactive\nType=simple\nConditionResult=no\nResult=success",
            ResourceState::Incorrect {
                current: "previous start was skipped by a condition".into(),
            },
        ),
        (
            "condition skip after a previous successful start",
            "ActiveState=inactive\nType=oneshot\nResult=success\nExecMainStartTimestampMonotonic=123\nConditionResult=no",
            ResourceState::Incorrect {
                current: "previous start was skipped by a condition".into(),
            },
        ),
        (
            "stopped timer",
            "ActiveState=inactive\nConditionResult=yes\nResult=success",
            ResourceState::Incorrect {
                current: "runtime state is inactive".into(),
            },
        ),
        (
            "failed unit with unmet condition",
            "ActiveState=failed\nConditionResult=no\nResult=exit-code",
            ResourceState::Incorrect {
                current: "runtime state is failed".into(),
            },
        ),
    ] {
        assert_eq!(runtime_state(true, properties), expected, "{label}");
    }
    assert_eq!(
        runtime_state(false, "ActiveState=inactive\nConditionResult=no"),
        ResourceState::Correct
    );
}

#[test]
fn first_login_marker_policy_only_applies_to_enabled_user_units() {
    let home = tempfile::tempdir().unwrap();
    let runtime = "ActiveState=inactive\nConditionResult=no\n";
    let mut system = queried_unit(
        FIRST_LOGIN_UNIT,
        UnitScope::System,
        true,
        ExecResult::success("enabled"),
        Some(runtime),
    );
    system.home = Some(home.path().to_path_buf());
    assert!(matches!(
        system.current_state().unwrap(),
        ResourceState::Incorrect { .. }
    ));

    let marker = home.path().join(DEFERRED_EXTENSIONS_MARKER);
    std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
    std::fs::write(marker, "pending").unwrap();
    let mut disabled = queried_unit(
        FIRST_LOGIN_UNIT,
        UnitScope::User,
        false,
        ExecResult::failure("disabled", "", Some(1)),
        Some(runtime),
    );
    disabled.home = Some(home.path().to_path_buf());
    assert_eq!(disabled.current_state().unwrap(), ResourceState::Correct);
}

#[test]
fn live_aliases_report_the_canonical_name_without_probing_runtime() {
    let name = "example-alias.service";
    let fallback = "example-alias.service is an alias. Declare the canonical unit name";
    for (result, reason) in [
        (
            ExecResult::success("example.service\n"),
            "example-alias.service is an alias. Declare the canonical unit example.service",
        ),
        (
            ExecResult::failure("", "manager unavailable", Some(1)),
            fallback,
        ),
        (ExecResult::success("/invalid.service\n"), fallback),
        (ExecResult::success(format!("{name}\n")), fallback),
    ] {
        let executor = ScriptedExecutor::new()
            .command(
                systemctl(&["--user", "is-enabled", name]),
                Ok(ExecResult::success("alias\n")),
            )
            .command(
                systemctl(&["--user", "show", "--property=Id", "--value", name]),
                Ok(result),
            );
        let resource = SystemdUnitResource::new(name, UnitScope::User, Arc::new(executor));
        assert_eq!(
            resource.current_state().unwrap(),
            ResourceState::Invalid {
                reason: reason.into()
            }
        );
    }
    let failed = queried_unit(
        name,
        UnitScope::User,
        true,
        ExecResult::failure("alias\n", "probe failed", Some(1)),
        None,
    );
    assert!(matches!(
        failed.current_state().unwrap(),
        ResourceState::Unknown { .. }
    ));
}

#[test]
fn disabled_or_system_scope_thunar_units_use_the_regular_systemd_lifecycle() {
    for (scope, enabled, enablement, runtime) in [
        (
            UnitScope::User,
            false,
            ExecResult::failure("disabled", "", Some(1)),
            "ActiveState=inactive\n",
        ),
        (
            UnitScope::System,
            true,
            ExecResult::success("enabled"),
            "ActiveState=active\n",
        ),
    ] {
        let entry = crate::domains::system::config::systemd_units::SystemdUnit {
            name: THUNAR_SETTINGS_UNIT.into(),
            scope,
            graphical: false,
            enabled,
        };
        let executor = ScriptedExecutor::new()
            .command(
                unit_probe(&entry.scope, &entry.name, &["is-enabled"]),
                Ok(enablement),
            )
            .command(
                runtime_probe(&entry.scope, &entry.name),
                Ok(ExecResult::success(runtime)),
            );
        let resource = SystemdUnitResource::from_entry(
            &entry,
            Arc::new(executor),
            Path::new("/tmp"),
            true,
            true,
        );
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }
}

#[test]
fn runtime_probe_failure_is_not_converged() {
    let name = "test.service";
    let script = ScriptedExecutor::new()
        .command(
            systemctl(&["is-enabled", name]),
            Ok(ExecResult::success("enabled\n")),
        )
        .command(
            runtime_probe(&UnitScope::System, name),
            Ok(ExecResult::failure("", "Failed to connect to bus", Some(1))),
        );
    let resource = SystemdUnitResource::new(name, UnitScope::System, Arc::new(script));
    assert!(
        matches!(resource.current_state().unwrap(), ResourceState::Unknown { reason } if reason.contains("Failed to connect to bus"))
    );
}

#[test]
fn description_returns_unit_name() {
    let executor: Arc<dyn Executor> = Arc::new(crate::infra::exec::ProcessExecutor::system());
    let resource = SystemdUnitResource::new(
        "clean-home-tmp.timer".to_string(),
        UnitScope::User,
        executor,
    );
    assert_eq!(resource.description(), "clean-home-tmp.timer");
}

#[test]
fn current_state_combines_enablement_and_requested_runtime() {
    for (scope, enabled, output, runtime) in [
        (
            UnitScope::User,
            true,
            ExecResult::success("enabled\n"),
            Some("ActiveState=active\n"),
        ),
        (
            UnitScope::System,
            false,
            ExecResult::failure("disabled\n", "", Some(1)),
            Some("ActiveState=inactive\n"),
        ),
        (
            UnitScope::User,
            false,
            ExecResult::failure("masked\n", "", Some(1)),
            Some("ActiveState=inactive\n"),
        ),
        (
            UnitScope::System,
            true,
            ExecResult::success(""),
            Some("ActiveState=active\n"),
        ),
        (
            UnitScope::System,
            false,
            ExecResult::failure(
                "not-found\n",
                "Unit dhcpcd.service could not be found.",
                Some(4),
            ),
            None,
        ),
    ] {
        let resource = queried_unit("dhcpcd.service", scope, enabled, output, runtime);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }
}

#[test]
fn current_state_missing_for_disabled_or_linked_units_requested_enabled() {
    for state in ["disabled", "linked", "linked-runtime"] {
        let resource = queried_unit(
            "dunst.service",
            UnitScope::User,
            true,
            ExecResult::failure(format!("{state}\n"), "", Some(1)),
            None,
        );
        assert_eq!(
            resource.current_state().unwrap(),
            ResourceState::Missing,
            "{state}"
        );
    }
}

#[test]
fn current_state_incorrect_when_disabled_unit_is_enabled() {
    let resource = queried_unit(
        "dhcpcd.service",
        UnitScope::System,
        false,
        ExecResult::success("enabled\n"),
        None,
    );
    assert_eq!(
        resource.current_state().unwrap(),
        ResourceState::Incorrect {
            current: "enabled".into()
        }
    );
}

#[test]
fn static_units_cannot_satisfy_persistent_enablement() {
    let resource = queried_unit(
        "fixture.service",
        UnitScope::User,
        true,
        ExecResult::success("static\n"),
        None,
    );
    assert!(matches!(
        resource.current_state().unwrap(),
        ResourceState::Invalid { reason } if reason.contains("persistent enablement")
    ));
}

#[test]
fn disabled_static_units_still_check_runtime_state() {
    for (active, correct) in [
        ("inactive", true),
        ("failed", true),
        ("active", false),
        ("activating", false),
    ] {
        let resource = queried_unit(
            "fixture.service",
            UnitScope::System,
            false,
            ExecResult::success("static\n"),
            Some(&format!("ActiveState={active}\n")),
        );
        assert_eq!(
            resource.current_state().unwrap() == ResourceState::Correct,
            correct
        );
    }
}

#[test]
fn disabling_a_static_unit_converges_after_stopping_it() {
    let name = "static.service";
    let mut script = ScriptedExecutor::new();
    for active in ["active", "inactive", "inactive"] {
        script = script
            .command(
                systemctl(&["is-enabled", name]),
                Ok(ExecResult::success("static\n")),
            )
            .command(
                runtime_probe(&UnitScope::System, name),
                Ok(ExecResult::success(format!("ActiveState={active}\n"))),
            );
        if active == "active" {
            for mode in ["--runtime", "--now"] {
                let change = CommandSpec::new("sudo")
                    .args(&["systemctl", "disable", mode, name])
                    .unchecked();
                script = script.command(change, Ok(ExecResult::success("")));
            }
        }
    }
    let mut resource = SystemdUnitResource::new(name, UnitScope::System, Arc::new(script));
    resource.enabled = false;

    assert!(matches!(
        resource.current_state().unwrap(),
        ResourceState::Incorrect { .. }
    ));
    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
}

#[test]
fn current_state_unknown_for_ambiguous_or_failed_static_probes() {
    for (scope, enabled, stdout, stderr) in [
        (UnitScope::User, true, "", "Failed to connect to bus"),
        (UnitScope::System, false, "static\n", "probe failed"),
    ] {
        let resource = queried_unit(
            "dunst.service",
            scope,
            enabled,
            ExecResult::failure(stdout, stderr, Some(1)),
            None,
        );
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Unknown { .. }
        ));
    }
}

#[test]
fn current_state_invalid_for_unknown_scope() {
    let mock = MockExecutor::new();
    let executor: Arc<dyn Executor> = Arc::new(mock);
    let resource = SystemdUnitResource::new(
        "dunst.service",
        UnitScope::Invalid("global".to_string()),
        executor,
    );
    assert!(matches!(
        resource.current_state().unwrap(),
        ResourceState::Invalid { .. }
    ));
    assert!(
        resource.apply().is_err(),
        "invalid scopes cannot invoke systemctl"
    );
}

#[test]
fn configured_scope_and_enablement_drive_exact_apply_commands() {
    for (scope, enabled, program, args) in [
        (
            UnitScope::User,
            true,
            "systemctl",
            ["--user", "enable", "--now", "fixture.service"],
        ),
        (
            UnitScope::User,
            false,
            "systemctl",
            ["--user", "disable", "--now", "fixture.service"],
        ),
        (
            UnitScope::System,
            true,
            "sudo",
            ["systemctl", "enable", "--now", "fixture.service"],
        ),
        (
            UnitScope::System,
            false,
            "sudo",
            ["systemctl", "disable", "--now", "fixture.service"],
        ),
    ] {
        let command = |arguments: &[&str]| CommandSpec::new(program).args(arguments).unchecked();
        let mut script = ScriptedExecutor::new();
        if !enabled {
            let mut runtime = args;
            runtime[2] = "--runtime";
            script = script.command(command(&runtime), Ok(ExecResult::success("")));
        }
        script = script.command(command(&args), Ok(ExecResult::success("")));
        let entry = crate::domains::system::config::systemd_units::SystemdUnit {
            name: "fixture.service".into(),
            scope,
            graphical: false,
            enabled,
        };
        let home = Path::new("unused-home");
        let resource = SystemdUnitResource::from_entry(&entry, Arc::new(script), home, true, true);
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }
}

#[test]
fn runtime_disable_failure_is_reported_before_persistent_disable() {
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .once()
        .withf(|spec| {
            spec.program() == "systemctl"
                && spec.arguments() == ["--user", "disable", "--runtime", "fixture.service"]
                && !spec.is_checked()
        })
        .returning(|_| Ok(ExecResult::failure("", "permission denied", Some(1))));
    let mut resource = SystemdUnitResource::new("fixture.service", UnitScope::User, Arc::new(mock));
    resource.enabled = false;
    assert!(matches!(
        resource.apply().unwrap(),
        ResourceChange::Skipped { reason, .. } if reason.contains("permission denied")
    ));
}

#[test]
fn system_automount_and_swap_units_use_the_existing_runtime_contract() {
    for name in ["data.automount", "swapfile.swap"] {
        for enabled in [true, false] {
            let (state, runtime, action) = if enabled {
                (
                    ExecResult::success("enabled\n"),
                    "ActiveState=active\n",
                    "enable",
                )
            } else {
                (
                    ExecResult::failure("disabled\n", "", Some(1)),
                    "ActiveState=inactive\n",
                    "disable",
                )
            };
            let resource = queried_unit(name, UnitScope::System, enabled, state, Some(runtime));

            assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
            assert_eq!(
                resource.change_invocation(&[action, "--now"]).unwrap(),
                ("sudo", vec!["systemctl", action, "--now", name])
            );
        }
    }
}

#[test]
fn apply_reports_unmet_work_with_exit_diagnostics() {
    for (code, output, error, expected) in [
        (
            Some(42),
            "partial output",
            "bus unavailable",
            ["42", "partial output", "bus unavailable"],
        ),
        (None, "", "", ["signal", "dunst.service", "enable"]),
    ] {
        let mut mock = MockExecutor::new();
        mock.expect_execute().once().returning(move |spec| {
            assert_eq!(spec.program(), "systemctl");
            assert_eq!(
                spec.arguments(),
                ["--user", "enable", "--now", "dunst.service"]
            );
            assert!(!spec.is_checked());
            Ok(ExecResult::failure(output, error, code))
        });
        let resource = SystemdUnitResource::new("dunst.service", UnitScope::User, Arc::new(mock));
        let change = resource.apply().unwrap();
        let ResourceChange::Skipped { reason, .. } = &change else {
            panic!("failed systemctl must not report a change");
        };
        for detail in expected {
            assert!(reason.contains(detail), "missing {detail:?}: {reason}");
        }
        assert_eq!(change, ResourceChange::unusable(reason.clone()));
    }
}

#[cfg(unix)]
#[test]
fn offline_packaged_unit_enablement_uses_user_directory() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    let system_dir = fixture.path().join("usr/lib/systemd/user");
    std::fs::create_dir_all(&system_dir).unwrap();
    let unit = "gnome-keyring-daemon.socket";
    let source = system_dir.join(unit);
    std::fs::write(&source, "[Install]\nWantedBy=sockets.target\n").unwrap();
    let mut resource = offline_unit(&home, unit, true);
    resource.system_user_unit_dirs = vec![system_dir.clone()];
    assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    assert!(!home.exists(), "discovery must not create user directories");
    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    let link = home
        .join(".config/systemd/user/sockets.target.wants")
        .join(unit);
    assert_eq!(std::fs::read_link(&link).unwrap(), source);
    assert!(!system_dir.join("sockets.target.wants").exists());
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    resource.enabled = false;
    assert!(matches!(
        resource.current_state().unwrap(),
        ResourceState::Incorrect { .. }
    ));
    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    assert!(!link.is_symlink());
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
}

#[test]
fn runtime_enablement_does_not_satisfy_persistent_enablement() {
    for scope in [UnitScope::User, UnitScope::System] {
        let resource = queried_unit(
            "fixture.service",
            scope.clone(),
            true,
            ExecResult::success("enabled-runtime\n"),
            None,
        );
        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
        let disabled_resource = queried_unit(
            "fixture.service",
            scope,
            false,
            ExecResult::success("enabled-runtime\n"),
            None,
        );
        assert!(matches!(
            disabled_resource.current_state().unwrap(),
            ResourceState::Incorrect { .. }
        ));
    }
}

#[test]
fn indirect_units_are_rejected_for_both_desired_states() {
    for enabled in [true, false] {
        let resource = queried_unit(
            "fixture.service",
            UnitScope::User,
            enabled,
            ExecResult::success("indirect\n"),
            None,
        );
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Invalid { reason } if reason.contains("declare the unit with direct install links")
        ));
    }
}
