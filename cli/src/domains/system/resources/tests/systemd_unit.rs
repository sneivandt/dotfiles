use std::sync::Arc;

use super::*;
use crate::infra::exec::{ExecResult, MockExecutor};

fn expect_runtime(mock: &mut MockExecutor, unit: &str, scope: UnitScope, output: &str) {
    let unit = unit.to_string();
    let output = output.to_string();
    mock.expect_execute()
            .once()
            .withf(move |spec| {
                let mut args = vec![
                    "show",
                    "--property=ActiveState,Type,Result,ExecMainStartTimestampMonotonic,ConditionResult",
                    &unit,
                ];
                if scope == UnitScope::User {
                    args.insert(0, "--user");
                }
                spec.program() == "systemctl"
                    && spec.arguments() == args.as_slice()
                    && !spec.is_checked()
            })
            .returning(move |_| Ok(ExecResult::success(output.clone())));
}

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
            "condition-skipped oneshot",
            true,
            "ActiveState=inactive\nType=oneshot\nResult=success\nExecMainStartTimestampMonotonic=0\nConditionResult=no",
            true,
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
        assert!(
            !matches!(state, ResourceState::Unknown { .. }),
            "{label}: {state:?}"
        );
    }
    for properties in ["", "ActiveState=unexpected"] {
        assert!(matches!(
            runtime_state(true, properties),
            ResourceState::Unknown { .. }
        ));
    }
}

#[test]
fn runtime_probe_failure_is_not_converged() {
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .once()
        .withf(|spec| spec.arguments() == ["is-enabled", "test.service"])
        .returning(|_| Ok(ExecResult::success("enabled\n")));
    mock.expect_execute()
        .once()
        .withf(|spec| spec.arguments().first().is_some_and(|arg| arg == "show"))
        .returning(|_| Ok(ExecResult::failure("", "Failed to connect to bus", Some(1))));
    let resource = SystemdUnitResource::new("test.service", UnitScope::System, Arc::new(mock));
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
fn from_entry_copies_name() {
    let executor: Arc<dyn Executor> = Arc::new(crate::infra::exec::ProcessExecutor::system());
    let entry = crate::domains::system::config::systemd_units::SystemdUnit {
        name: "dunst.service".to_string(),
        scope: UnitScope::User,
        enabled: true,
    };
    let resource = SystemdUnitResource::from_entry(&entry, executor, Path::new("/home/test"), true);
    assert_eq!(resource.name, "dunst.service");
    assert_eq!(resource.scope, UnitScope::User);
    assert!(resource.enabled);
}

#[test]
fn from_entry_copies_disabled_state() {
    let executor: Arc<dyn Executor> = Arc::new(crate::infra::exec::ProcessExecutor::system());
    let entry = crate::domains::system::config::systemd_units::SystemdUnit {
        name: "dhcpcd.service".to_string(),
        scope: UnitScope::System,
        enabled: false,
    };
    let resource = SystemdUnitResource::from_entry(&entry, executor, Path::new("/home/test"), true);
    assert!(!resource.enabled);
}

// ------------------------------------------------------------------
// current_state
// ------------------------------------------------------------------

#[test]
fn current_state_correct_when_systemctl_reports_enabled() {
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .once()
        .withf(|spec| spec.arguments().iter().any(|arg| arg == "is-enabled"))
        .returning(|_| Ok(ExecResult::success("enabled\n")));
    expect_runtime(
        &mut mock,
        "dunst.service",
        UnitScope::User,
        "ActiveState=active\n",
    );
    let executor: Arc<dyn Executor> = Arc::new(mock);
    let resource = SystemdUnitResource::new("dunst.service", UnitScope::User, executor);
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
}

#[test]
fn current_state_missing_when_systemctl_reports_disabled() {
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .once()
        .returning(|_| Ok(ExecResult::failure("disabled\n", "", Some(1))));
    let executor: Arc<dyn Executor> = Arc::new(mock);
    let resource = SystemdUnitResource::new("dunst.service", UnitScope::User, executor);
    assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
}

#[test]
fn current_state_correct_when_disabled_unit_is_disabled() {
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .once()
        .withf(|spec| spec.arguments().iter().any(|arg| arg == "is-enabled"))
        .returning(|_| Ok(ExecResult::failure("disabled\n", "", Some(1))));
    expect_runtime(
        &mut mock,
        "dhcpcd.service",
        UnitScope::System,
        "ActiveState=inactive\n",
    );
    let executor: Arc<dyn Executor> = Arc::new(mock);
    let mut resource = SystemdUnitResource::new("dhcpcd.service", UnitScope::System, executor);
    resource.enabled = false;
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
}

#[test]
fn current_state_incorrect_when_disabled_unit_is_enabled() {
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .once()
        .returning(|_| Ok(ExecResult::success("enabled\n")));
    let executor: Arc<dyn Executor> = Arc::new(mock);
    let mut resource = SystemdUnitResource::new("dhcpcd.service", UnitScope::System, executor);
    resource.enabled = false;
    assert_eq!(
        resource.current_state().unwrap(),
        ResourceState::Incorrect {
            current: "enabled".to_string()
        }
    );
}

#[test]
fn current_state_correct_when_disabled_unit_is_not_installed() {
    let mut mock = MockExecutor::new();
    mock.expect_execute().once().returning(|_| {
        Ok(ExecResult::failure(
            "not-found\n",
            "Unit dhcpcd.service could not be found.",
            Some(4),
        ))
    });
    let executor: Arc<dyn Executor> = Arc::new(mock);
    let mut resource = SystemdUnitResource::new("dhcpcd.service", UnitScope::System, executor);
    resource.enabled = false;
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
}

#[test]
fn current_state_missing_when_systemctl_reports_linked() {
    for state in ["linked", "linked-runtime"] {
        let state = state.to_string();
        let output = state.clone();
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .returning(move |_| Ok(ExecResult::failure(format!("{output}\n"), "", Some(1))));
        let executor: Arc<dyn Executor> = Arc::new(mock);
        let resource =
            SystemdUnitResource::new("network-manager-applet.service", UnitScope::User, executor);
        assert_eq!(
            resource.current_state().unwrap(),
            ResourceState::Missing,
            "{state} units should be enabled"
        );
    }
}

#[test]
fn current_state_unknown_when_systemctl_failure_is_ambiguous() {
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .once()
        .returning(|_| Ok(ExecResult::failure("", "Failed to connect to bus", Some(1))));
    let executor: Arc<dyn Executor> = Arc::new(mock);
    let resource = SystemdUnitResource::new("dunst.service", UnitScope::User, executor);
    assert!(matches!(
        resource.current_state().unwrap(),
        ResourceState::Unknown { .. }
    ));
}

#[test]
fn current_state_uses_system_scope_without_user_flag() {
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .once()
        .withf(|spec| {
            spec.program() == "systemctl"
                && spec.arguments() == ["is-enabled", "sshd.service"]
                && !spec.is_checked()
        })
        .returning(|_| Ok(ExecResult::success("")));
    expect_runtime(
        &mut mock,
        "sshd.service",
        UnitScope::System,
        "ActiveState=active\n",
    );
    let executor: Arc<dyn Executor> = Arc::new(mock);
    let resource = SystemdUnitResource::new("sshd.service", UnitScope::System, executor);
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
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
}

// ------------------------------------------------------------------
// apply
// ------------------------------------------------------------------

#[test]
fn apply_returns_applied_when_systemctl_succeeds() {
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .once()
        .returning(|_| Ok(ExecResult::success("")));
    let executor: Arc<dyn Executor> = Arc::new(mock);
    let resource = SystemdUnitResource::new("dunst.service", UnitScope::User, executor);
    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
}

#[test]
fn apply_returns_skipped_when_systemctl_fails() {
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .once()
        .returning(|_| Ok(ExecResult::failure("", "", Some(1))));
    let executor: Arc<dyn Executor> = Arc::new(mock);
    let resource = SystemdUnitResource::new("dunst.service", UnitScope::User, executor);
    assert!(
        matches!(resource.apply().unwrap(), ResourceChange::Skipped { .. }),
        "expected Skipped when systemctl enable fails"
    );
}

#[test]
fn apply_uses_sudo_for_system_scope() {
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .once()
        .withf(|spec| {
            spec.program() == "sudo"
                && spec.arguments() == ["systemctl", "enable", "--now", "sshd.service"]
                && !spec.is_checked()
        })
        .returning(|_| Ok(ExecResult::success("")));
    let executor: Arc<dyn Executor> = Arc::new(mock);
    let resource = SystemdUnitResource::new("sshd.service", UnitScope::System, executor);
    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
}

#[test]
fn apply_disables_system_scope_unit_with_sudo() {
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .once()
        .withf(|spec| {
            spec.program() == "sudo"
                && spec.arguments() == ["systemctl", "disable", "--now", "dhcpcd.service"]
                && !spec.is_checked()
        })
        .returning(|_| Ok(ExecResult::success("")));
    let executor: Arc<dyn Executor> = Arc::new(mock);
    let mut resource = SystemdUnitResource::new("dhcpcd.service", UnitScope::System, executor);
    resource.enabled = false;
    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
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
    let entry = crate::domains::system::config::systemd_units::SystemdUnit {
        name: unit.to_string(),
        scope: UnitScope::User,
        enabled: true,
    };
    let mut resource =
        SystemdUnitResource::from_entry(&entry, Arc::new(MockExecutor::new()), &home, false);
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

#[cfg(unix)]
#[test]
fn offline_lookup_preserves_override_precedence_and_masks() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    let user_dir = home.join(".config/systemd/user");
    let admin_dir = fixture.path().join("etc/systemd/user");
    let package_dir = fixture.path().join("usr/lib/systemd/user");
    for dir in [&user_dir, &admin_dir, &package_dir] {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("example.service"),
            "[Install]\nWantedBy=default.target\n",
        )
        .unwrap();
    }
    let entry = crate::domains::system::config::systemd_units::SystemdUnit {
        name: "example.service".to_string(),
        scope: UnitScope::User,
        enabled: true,
    };
    let mut resource =
        SystemdUnitResource::from_entry(&entry, Arc::new(MockExecutor::new()), &home, false);
    resource.system_user_unit_dirs = vec![admin_dir.clone(), package_dir.clone()];
    for expected in [&user_dir, &admin_dir, &package_dir] {
        assert_eq!(
            resource.offline_unit_path().unwrap(),
            Some(expected.join(&entry.name))
        );
        std::fs::remove_file(expected.join(&entry.name)).unwrap();
    }
    std::fs::write(
        package_dir.join(&entry.name),
        "[Install]\nWantedBy=default.target\n",
    )
    .unwrap();
    for target in [Path::new("/dev/null"), &fixture.path().join("missing")] {
        let mask = user_dir.join(&entry.name);
        std::os::unix::fs::symlink(target, &mask).unwrap();
        assert_eq!(resource.offline_unit_path().unwrap(), Some(mask.clone()));
        assert!(
            resource.apply().is_err(),
            "must not fall back past a masked or broken override"
        );
        assert!(!user_dir.join("default.target.wants").exists());
        std::fs::remove_file(mask).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn offline_enablement_repairs_checkout_links_and_accepts_relative_installed_links() {
    let home = tempfile::tempdir().unwrap();
    let source = home.path().join("checkout/example.service");
    let unit_dir = home.path().join(".config/systemd/user");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::create_dir_all(unit_dir.join("default.target.wants")).unwrap();
    std::fs::write(&source, "[Install]\nWantedBy=default.target\n").unwrap();
    let installed = unit_dir.join("example.service");
    std::os::unix::fs::symlink(&source, &installed).unwrap();
    let link = unit_dir.join("default.target.wants/example.service");
    std::os::unix::fs::symlink(&source, &link).unwrap();
    let entry = crate::domains::system::config::systemd_units::SystemdUnit {
        name: "example.service".to_string(),
        scope: UnitScope::User,
        enabled: true,
    };
    let resource =
        SystemdUnitResource::from_entry(&entry, Arc::new(MockExecutor::new()), home.path(), false);
    assert!(matches!(
        resource.current_state().unwrap(),
        ResourceState::Incorrect { .. }
    ));
    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    assert_eq!(std::fs::read_link(&link).unwrap(), installed);
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink("../example.service", &link).unwrap();
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
}

#[cfg(unix)]
#[test]
fn offline_user_unit_is_enabled_without_contacting_the_bus() {
    let home = tempfile::tempdir().unwrap();
    let unit_dir = home.path().join(".config/systemd/user");
    std::fs::create_dir_all(&unit_dir).unwrap();
    std::fs::write(
        unit_dir.join("clean-home-tmp.timer"),
        "[Unit]\nDescription=Clean tmp\n[Install]\nWantedBy=timers.target\n",
    )
    .unwrap();
    let entry = crate::domains::system::config::systemd_units::SystemdUnit {
        name: "clean-home-tmp.timer".to_string(),
        scope: UnitScope::User,
        enabled: true,
    };
    let resource =
        SystemdUnitResource::from_entry(&entry, Arc::new(MockExecutor::new()), home.path(), false);

    assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    let enabled = unit_dir.join("timers.target.wants/clean-home-tmp.timer");
    assert!(
        enabled.is_symlink(),
        "offline enablement should create a symlink"
    );
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
}

#[cfg(unix)]
#[test]
fn offline_user_unit_missing_definition_is_plannable_for_dry_run() {
    let home = tempfile::tempdir().unwrap();
    let entry = crate::domains::system::config::systemd_units::SystemdUnit {
        name: "not-linked-yet.timer".to_string(),
        scope: UnitScope::User,
        enabled: true,
    };
    let resource =
        SystemdUnitResource::from_entry(&entry, Arc::new(MockExecutor::new()), home.path(), false);

    assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
}

#[cfg(unix)]
#[test]
fn offline_user_unit_supports_required_by_targets() {
    let home = tempfile::tempdir().unwrap();
    let unit_dir = home.path().join(".config/systemd/user");
    std::fs::create_dir_all(&unit_dir).unwrap();
    std::fs::write(
        unit_dir.join("session.service"),
        "[Install]\nRequiredBy=graphical-session.target\n",
    )
    .unwrap();
    let entry = crate::domains::system::config::systemd_units::SystemdUnit {
        name: "session.service".to_string(),
        scope: UnitScope::User,
        enabled: true,
    };
    let resource =
        SystemdUnitResource::from_entry(&entry, Arc::new(MockExecutor::new()), home.path(), false);

    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    assert!(
        unit_dir
            .join("graphical-session.target.requires/session.service")
            .is_symlink()
    );
}

#[cfg(unix)]
#[test]
fn offline_user_unit_can_be_disabled() {
    let home = tempfile::tempdir().unwrap();
    let unit_dir = home.path().join(".config/systemd/user");
    std::fs::create_dir_all(&unit_dir).unwrap();
    std::fs::write(
        unit_dir.join("session.service"),
        "[Install]\nWantedBy=graphical-session.target\n",
    )
    .unwrap();
    let entry = crate::domains::system::config::systemd_units::SystemdUnit {
        name: "session.service".to_string(),
        scope: UnitScope::User,
        enabled: true,
    };
    let resource =
        SystemdUnitResource::from_entry(&entry, Arc::new(MockExecutor::new()), home.path(), false);
    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);

    let disabled_entry = crate::domains::system::config::systemd_units::SystemdUnit {
        enabled: false,
        ..entry
    };
    let disabled = SystemdUnitResource::from_entry(
        &disabled_entry,
        Arc::new(MockExecutor::new()),
        home.path(),
        false,
    );
    assert!(matches!(
        disabled.current_state().unwrap(),
        ResourceState::Incorrect { .. }
    ));
    assert_eq!(disabled.apply().unwrap(), ResourceChange::Applied);
    assert_eq!(disabled.current_state().unwrap(), ResourceState::Correct);
}

#[cfg(unix)]
#[test]
fn offline_user_unit_rejects_install_targets_that_escape_the_user_directory() {
    let home = tempfile::tempdir().unwrap();
    let unit_dir = home.path().join(".config/systemd/user");
    std::fs::create_dir_all(&unit_dir).unwrap();
    std::fs::write(
        unit_dir.join("unsafe.service"),
        "[Install]\nWantedBy=../../outside.target\n",
    )
    .unwrap();
    let entry = crate::domains::system::config::systemd_units::SystemdUnit {
        name: "unsafe.service".to_string(),
        scope: UnitScope::User,
        enabled: true,
    };
    let resource =
        SystemdUnitResource::from_entry(&entry, Arc::new(MockExecutor::new()), home.path(), false);

    let error = resource
        .apply()
        .expect_err("escaping target must be rejected");
    assert!(error.to_string().contains("invalid [Install] target"));
    assert!(!home.path().join("outside.target.wants").exists());
}
