use std::sync::Arc;

use super::*;
use crate::domains::system::systemd_fixture::{runtime_probe, systemctl, unit_probe};
use crate::infra::exec::{ExecResult, MockExecutor};
use crate::test_helpers::ScriptedExecutor;

#[cfg(unix)]
#[test]
fn batch_probe_eligibility_state_transitions_and_apply_unit_routing() {
    use crate::domains::system::config::systemd_units::SystemdUnit;

    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();

    // is_batch_probeable covers all 4 gates.
    let entry = |name: &str, scope: UnitScope, enabled: bool| SystemdUnit {
        name: name.into(),
        scope,
        graphical: false,
        enabled,
    };
    let live = SystemdUnitResource::from_entry(
        &entry("demo.service", UnitScope::User, true),
        Arc::new(MockExecutor::new()),
        &home,
        true,
        true,
    );
    assert!(live.is_batch_probeable());
    let mut retired = SystemdUnitResource::from_entry(
        &entry("demo.service", UnitScope::User, true),
        Arc::new(MockExecutor::new()),
        &home,
        true,
        true,
    );
    retired.retired_source = Some(home.join("retired"));
    assert!(!retired.is_batch_probeable());
    let offline = SystemdUnitResource::from_entry(
        &entry("demo.service", UnitScope::User, true),
        Arc::new(MockExecutor::new()),
        &home,
        false,
        true,
    );
    assert!(!offline.is_batch_probeable());
    let invalid_scope = SystemdUnitResource::from_entry(
        &entry("demo.service", UnitScope::Invalid("bad".into()), true),
        Arc::new(MockExecutor::new()),
        &home,
        true,
        true,
    );
    assert!(!invalid_scope.is_batch_probeable());
    let unsafe_name = SystemdUnitResource::from_entry(
        &entry("../bad.service", UnitScope::User, true),
        Arc::new(MockExecutor::new()),
        &home,
        true,
        true,
    );
    assert!(!unsafe_name.is_batch_probeable());

    // batch_probe_scope rejects non-empty stderr even with exit code 0.
    let mut mock_stderr = MockExecutor::new();
    mock_stderr.expect_execute().once().return_once(|_| {
        Ok(ExecResult {
            stdout: "enabled\nenabled\n".into(),
            stderr: "warning on stderr\n".into(),
            success: true,
            code: Some(0),
        })
    });
    assert!(
        SystemdUnitResource::batch_probe_scope(
            &mock_stderr,
            &UnitScope::User,
            &["a.service", "b.service"],
        )
        .unwrap()
        .is_empty()
    );
}

#[cfg(unix)]
#[test]
fn batch_probe_state_transitions_and_not_found_short_circuits() {
    use crate::domains::system::config::systemd_units::SystemdUnit;

    let entry = |name: &str, scope: UnitScope, enabled: bool| SystemdUnit {
        name: name.into(),
        scope,
        graphical: false,
        enabled,
    };
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let live = SystemdUnitResource::from_entry(
        &entry("demo.service", UnitScope::User, true),
        Arc::new(MockExecutor::new()),
        &home,
        true,
        true,
    );

    // state_from_batch_probe: static, not-found, global_enablement_conflict, live_user_link_state, runtime short-circuits.
    let probe = |name: &str, is_enabled: &str, active: &str| BatchUnitProbe {
        is_enabled: is_enabled.into(),
        properties: format!(
            "Id={name}\nActiveState={active}\nType=simple\nResult=success\nExecMainStartTimestampMonotonic=1\nConditionResult=yes\nNeedDaemonReload=no\n"
        ),
    };
    let disabled_user = SystemdUnitResource::from_entry(
        &entry("demo.service", UnitScope::User, false),
        Arc::new(MockExecutor::new()),
        &home,
        true,
        true,
    );
    assert!(matches!(
        live.state_from_batch_probe(&probe("demo.service", "static", "inactive"))
            .unwrap(),
        Some(ResourceState::Invalid { .. })
    ));
    assert_eq!(
        disabled_user
            .state_from_batch_probe(&probe("demo.service", "static", "inactive"))
            .unwrap(),
        Some(ResourceState::Correct)
    );
    assert_eq!(
        live.state_from_batch_probe(&probe("demo.service", "not-found", "inactive"))
            .unwrap(),
        None
    );
    assert_eq!(
        disabled_user
            .state_from_batch_probe(&probe("demo.service", "not-found", "active"))
            .unwrap(),
        Some(ResourceState::Correct)
    );
    // Disabled but active unit must proceed to runtime_state -> Incorrect.
    assert!(matches!(
        disabled_user
            .state_from_batch_probe(&probe("demo.service", "disabled", "active"))
            .unwrap(),
        Some(ResourceState::Incorrect { .. })
    ));
    // Enabled with start=false skips runtime_state -> Correct.
    let mut no_start = SystemdUnitResource::from_entry(
        &entry("demo.service", UnitScope::User, true),
        Arc::new(MockExecutor::new()),
        &home,
        true,
        true,
    );
    no_start.start = false;
    assert_eq!(
        no_start
            .state_from_batch_probe(&probe("demo.service", "enabled", "inactive"))
            .unwrap(),
        Some(ResourceState::Correct)
    );
    // Enabled with start=true and inactive -> proceeds to runtime_state -> Incorrect.
    assert!(matches!(
        live.state_from_batch_probe(&probe("demo.service", "enabled", "inactive"))
            .unwrap(),
        Some(ResourceState::Incorrect { .. })
    ));
}

#[cfg(unix)]
#[test]
fn batch_probe_global_conflict_live_link_and_not_found_fallbacks() {
    use crate::domains::system::config::systemd_units::SystemdUnit;

    let entry = |name: &str, scope: UnitScope, enabled: bool| SystemdUnit {
        name: name.into(),
        scope,
        graphical: false,
        enabled,
    };
    let probe = |name: &str, is_enabled: &str, active: &str| BatchUnitProbe {
        is_enabled: is_enabled.into(),
        properties: format!(
            "Id={name}\nActiveState={active}\nType=simple\nResult=success\nExecMainStartTimestampMonotonic=1\nConditionResult=yes\nNeedDaemonReload=no\n"
        ),
    };
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let live = SystemdUnitResource::from_entry(
        &entry("demo.service", UnitScope::User, true),
        Arc::new(MockExecutor::new()),
        &home,
        true,
        true,
    );
    let disabled_user = SystemdUnitResource::from_entry(
        &entry("demo.service", UnitScope::User, false),
        Arc::new(MockExecutor::new()),
        &home,
        true,
        true,
    );

    // global_enablement_conflict triggers for disabled User unit, NOT for disabled System unit.
    let global_dir = dir.path().join("etc-systemd-user");
    let global_wants = global_dir.join("default.target.wants");
    std::fs::create_dir_all(&global_wants).unwrap();
    let global_unit = global_dir.join("demo.service");
    std::fs::write(&global_unit, "[Unit]\n").unwrap();
    std::os::unix::fs::symlink(&global_unit, global_wants.join("demo.service")).unwrap();

    let mut disabled_with_global = disabled_user;
    disabled_with_global.system_user_unit_dirs = vec![global_dir.clone()];
    assert!(matches!(
        disabled_with_global
            .state_from_batch_probe(&probe("demo.service", "enabled", "inactive"))
            .unwrap(),
        Some(ResourceState::Invalid { .. })
    ));
    let mut disabled_system = SystemdUnitResource::from_entry(
        &entry("demo.service", UnitScope::System, false),
        Arc::new(MockExecutor::new()),
        &home,
        true,
        true,
    );
    disabled_system.system_user_unit_dirs = vec![global_dir];
    assert!(matches!(
        disabled_system
            .state_from_batch_probe(&probe("demo.service", "enabled", "inactive"))
            .unwrap(),
        Some(ResourceState::Incorrect { .. })
    ));

    // live_user_link_state returning Some(Correct) must NOT short-circuit an inactive unit.
    let user_dir = home.join(".config/systemd/user");
    let wants_dir = user_dir.join("default.target.wants");
    std::fs::create_dir_all(&wants_dir).unwrap();
    let unit_file = user_dir.join("demo.service");
    std::fs::write(&unit_file, "[Unit]\n[Install]\nWantedBy=default.target\n").unwrap();
    std::os::unix::fs::symlink(&unit_file, wants_dir.join("demo.service")).unwrap();
    assert!(matches!(
        live.state_from_batch_probe(&probe("demo.service", "enabled", "inactive"))
            .unwrap(),
        Some(ResourceState::Incorrect { .. })
    ));

    // "could not be found" without "not-found" yields Correct and skips runtime show.
    let not_found_unit = queried_unit(
        "absent.service",
        UnitScope::User,
        false,
        ExecResult::failure("", "Unit absent.service could not be found.", Some(1)),
        None,
    );
    assert_eq!(
        not_found_unit.current_state().unwrap(),
        ResourceState::Correct
    );
}

#[cfg(unix)]
#[test]
fn apply_reconciles_user_links_and_offline_helpers_propagate_errors() {
    use crate::domains::system::config::systemd_units::SystemdUnit;

    let entry = |name: &str, scope: UnitScope, enabled: bool| SystemdUnit {
        name: name.into(),
        scope,
        graphical: false,
        enabled,
    };
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let user_dir = home.join(".config/systemd/user");
    let wants_dir = user_dir.join("default.target.wants");
    let obsolete_wants = user_dir.join("old.target.wants");
    std::fs::create_dir_all(&wants_dir).unwrap();
    std::fs::create_dir_all(&obsolete_wants).unwrap();
    let unit_file = user_dir.join("demo.service");
    std::fs::write(&unit_file, "[Unit]\n[Install]\nWantedBy=default.target\n").unwrap();

    // apply on a non-FIRST_LOGIN_UNIT with incomplete/obsolete user links uses `start`, not `start --no-block`.
    std::os::unix::fs::symlink(&unit_file, obsolete_wants.join("demo.service")).unwrap();
    let script = ScriptedExecutor::new()
        .command(
            CommandSpec::new("systemctl").args(&["--user", "daemon-reload"]),
            Ok(ExecResult::success("")),
        )
        .command(
            systemctl(&["--user", "start", "demo.service"]),
            Ok(ExecResult::success("")),
        );
    let reconciled = SystemdUnitResource::from_entry(
        &entry("demo.service", UnitScope::User, true),
        Arc::new(script),
        &home,
        true,
        true,
    );
    assert_eq!(reconciled.apply().unwrap(), ResourceChange::Applied);

    // install_targets must not treat a line with only one bracket as a section header.
    let parsed = install_targets("[Install]\n[not-a-section\nWantedBy=default.target\n").unwrap();
    assert_eq!(parsed, vec!["default.target.wants"]);

    // offline_unit_path must propagate non-NotFound metadata errors (e.g. ENOTDIR).
    let not_a_dir = dir.path().join("not-a-dir");
    std::fs::write(&not_a_dir, "file").unwrap();
    let mut bad_dir_unit = offline_unit(&home, "missing.service", true);
    bad_dir_unit.system_user_unit_dirs = vec![not_a_dir];
    assert!(bad_dir_unit.offline_unit_path().is_err());

    // live_user_link_state: returns Ok(None) for System scope even when enabled and home is Some.
    let mut sys_unit = SystemdUnitResource::from_entry(
        &entry("demo.service", UnitScope::System, true),
        Arc::new(MockExecutor::new()),
        &home,
        true,
        true,
    );
    assert_eq!(sys_unit.live_user_link_state().unwrap(), None);
    // And returns Ok(None) when home is None for an enabled user unit.
    sys_unit.scope = UnitScope::User;
    sys_unit.home = None;
    assert_eq!(sys_unit.live_user_link_state().unwrap(), None);

    // live_user_link_state: non-file unit path (directory) yields Ok(None); ELOOP yields Err.
    let dir_unit_path = user_dir.join("dir.service");
    std::fs::create_dir(&dir_unit_path).unwrap();
    let dir_unit = SystemdUnitResource::from_entry(
        &entry("dir.service", UnitScope::User, true),
        Arc::new(MockExecutor::new()),
        &home,
        true,
        true,
    );
    assert_eq!(dir_unit.live_user_link_state().unwrap(), None);
    let loop_unit_path = user_dir.join("loop.service");
    std::os::unix::fs::symlink(&loop_unit_path, &loop_unit_path).unwrap();
    let loop_unit = SystemdUnitResource::from_entry(
        &entry("loop.service", UnitScope::User, true),
        Arc::new(MockExecutor::new()),
        &home,
        true,
        true,
    );
    assert!(loop_unit.live_user_link_state().is_err());
}

#[cfg(unix)]
#[test]
fn offline_enablement_and_reconcile_propagate_filesystem_errors() {
    let dir = tempfile::tempdir().unwrap();
    let not_a_dir = dir.path().join("not-a-dir");
    std::fs::write(&not_a_dir, "file").unwrap();

    // enablement_link_state: conflicting non-symlink entry yields Invalid, not Incorrect;
    // non-NotFound error (ENOTDIR) propagates as Err.
    let conflict_home = dir.path().join("conflict-home");
    let conflict_user_dir = conflict_home.join(".config/systemd/user");
    let conflict_wants = conflict_user_dir.join("default.target.wants");
    std::fs::create_dir_all(&conflict_wants).unwrap();
    let conflict_unit_file = conflict_user_dir.join("demo.service");
    std::fs::write(
        &conflict_unit_file,
        "[Unit]\n[Install]\nWantedBy=default.target\n",
    )
    .unwrap();
    std::fs::write(conflict_wants.join("demo.service"), "not-a-link").unwrap();
    let conflict_unit = offline_unit(&conflict_home, "demo.service", true);
    assert!(matches!(
        conflict_unit.current_state().unwrap(),
        ResourceState::Invalid { .. }
    ));

    let enotdir_home = dir.path().join("enotdir-home");
    let enotdir_user_dir = enotdir_home.join(".config/systemd/user");
    std::fs::create_dir_all(&enotdir_user_dir).unwrap();
    std::fs::write(
        enotdir_user_dir.join("demo.service"),
        "[Unit]\n[Install]\nWantedBy=default.target\n",
    )
    .unwrap();
    std::fs::write(enotdir_user_dir.join("default.target.wants"), "file").unwrap();
    let enotdir_unit = offline_unit(&enotdir_home, "demo.service", true);
    assert!(enotdir_unit.current_state().is_err());

    // reconcile_user_links: non-NotFound symlink_metadata error (ENAMETOOLONG) propagates as Err.
    let long_name = "x".repeat(300);
    let long_link = conflict_wants.join(&long_name);
    assert!(
        conflict_unit
            .reconcile_user_links(&[(long_link, conflict_unit_file)], false)
            .is_err()
    );

    // enablement_links_in: non-NotFound read_dir error (ENOTDIR) and symlink_metadata error
    // (ENAMETOOLONG) both propagate as Err via global_enablement_conflict / disable_offline.
    let mut enotdir_global = offline_unit(&conflict_home, "demo.service", false);
    enotdir_global.system_user_unit_dirs = vec![not_a_dir];
    assert!(enotdir_global.global_enablement_conflict().is_err());

    let mut long_name_unit = offline_unit(&conflict_home, "demo.service", false);
    long_name_unit.name = long_name;
    assert!(long_name_unit.disable_offline().is_err());
}

#[cfg(unix)]
fn offline_unit(home: &Path, name: &str, enabled: bool) -> SystemdUnitResource {
    let entry = crate::domains::system::config::systemd_units::SystemdUnit {
        name: name.into(),
        scope: UnitScope::User,
        graphical: false,
        enabled,
    };
    SystemdUnitResource::from_entry(&entry, Arc::new(MockExecutor::new()), home, false, false)
}

fn queried_unit(
    name: &str,
    scope: UnitScope,
    enabled: bool,
    enablement: ExecResult,
    runtime: Option<&str>,
) -> SystemdUnitResource {
    let mut script =
        ScriptedExecutor::new().command(unit_probe(&scope, name, &["is-enabled"]), Ok(enablement));
    if let Some(runtime) = runtime {
        script = script.command(
            runtime_probe(&scope, name),
            Ok(ExecResult::success(runtime)),
        );
    }
    let mut resource = SystemdUnitResource::new(name, scope, Arc::new(script));
    resource.enabled = enabled;
    resource
}

#[path = "systemd_unit_install_targets.rs"]
mod install_targets;
#[path = "systemd_unit_offline.rs"]
mod offline;
#[path = "systemd_unit_runtime.rs"]
mod runtime;
