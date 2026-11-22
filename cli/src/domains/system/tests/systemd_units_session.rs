use super::*;

fn graphical_probe(
    script: ScriptedExecutor,
    result: Result<ExecResult, ExecError>,
) -> ScriptedExecutor {
    let probe = CommandSpec::new("systemctl").args(&[
        "--user",
        "show",
        "--property=ActiveState",
        "--value",
        "graphical-session.target",
    ]);
    script
        .command(user_manager_probe(), Ok(ExecResult::success("")))
        .command(probe, result)
}

fn graphical_unit() -> SystemdUnit {
    SystemdUnit {
        name: "hypridle.service".into(),
        scope: UnitScope::User,
        enabled: true,
        graphical: true,
    }
}

#[test]
fn graphical_units_enable_without_starting_until_the_session_is_active() {
    use UnitProbe::{Disabled, Enabled, Runtime};

    #[derive(Debug, Clone, Copy)]
    enum UnitProbe {
        Disabled,
        Enabled,
        Runtime(&'static str),
    }

    let unit = "hypridle.service";
    let enable = ["--user", "enable", unit];
    let start = ["--user", "enable", "--now", unit];
    let cases = [
        ("inactive", Disabled, Some(enable.as_slice())),
        ("inactive", Enabled, None),
        ("activating", Enabled, None),
        ("deactivating", Enabled, None),
        ("failed", Enabled, None),
        (
            "active",
            Runtime("ActiveState=inactive"),
            Some(start.as_slice()),
        ),
        ("active", Runtime("ActiveState=active"), None),
    ];
    let task = ConfigureSystemd::new(ConfigHandle::new(vec![graphical_unit()]));
    for dry_run in [true, false] {
        let mut executor = script();
        for (session, observed, action) in cases {
            executor = graphical_probe(executor, Ok(ExecResult::success(session)));
            if !dry_run {
                executor = executor.command(user_reload(), Ok(ExecResult::success("")));
            }
            let enablement = match observed {
                Disabled => disabled_result(),
                Enabled | Runtime(_) => ExecResult::success("enabled"),
            };
            executor = executor.command(systemctl(&["--user", "is-enabled", unit]), Ok(enablement));
            if let Runtime(properties) = observed {
                executor = executor.command(
                    runtime_probe(&UnitScope::User, unit),
                    Ok(ExecResult::success(properties)),
                );
            }
            if !dry_run && let Some(args) = action {
                executor = executor.command(systemctl(args), Ok(ExecResult::success("")));
            }
        }
        let ctx = context(executor).with_dry_run(dry_run);
        for (session, observed, action) in cases {
            let result = task.run(&ctx).unwrap();
            let stats = crate::test_helpers::task_batch(&result);
            let expected = if action.is_some() { (1, 0) } else { (0, 1) };
            let counts = (stats.changed_count(), stats.already_ok_count());
            assert_eq!(
                counts, expected,
                "{session}, {observed:?}, dry_run={dry_run}"
            );
            assert_eq!(
                (stats.failed_count(), stats.skipped_count()),
                (0, 0),
                "{result:?}"
            );
        }
    }
}

#[test]
fn graphical_session_probe_errors_never_reload_or_start_units() {
    let units = vec![graphical_unit()];
    let task = ConfigureSystemd::new(ConfigHandle::new(units.clone()));
    for dry_run in [true, false] {
        for (response, diagnostic) in [
            (
                Ok(ExecResult::success("")),
                "unexpected graphical-session.target ActiveState: \"\"",
            ),
            (
                Ok(ExecResult::success("unexpected")),
                "unexpected graphical-session.target ActiveState: \"unexpected\"",
            ),
            (
                Err(ExecError::NonZero {
                    command: "systemctl show".into(),
                    result: ExecResult::failure("", "no bus", Some(1)),
                }),
                "no bus",
            ),
        ] {
            let executor = graphical_probe(script(), response);
            let error = task
                .run(&context(executor).with_dry_run(dry_run))
                .unwrap_err();
            assert!(format!("{error:#}").contains(diagnostic), "{error:#}");
        }
    }
    let ctx = context(MockExecutor::new());
    assert!(!graphical_session_active(&ctx, &units, false).unwrap());
    let mut disabled = units;
    disabled[0].enabled = false;
    assert!(!graphical_session_active(&ctx, &disabled, true).unwrap());
}

#[test]
fn gtk_service_migration_runs_only_for_the_enabled_user_replacement() {
    let unit = |name: &str, scope, enabled| SystemdUnit {
        name: name.into(),
        scope,
        graphical: false,
        enabled,
    };
    let gtk = unit("gtk-settings.service", UnitScope::User, true);
    for (label, units, expected) in [
        ("replacement", vec![gtk.clone()], true),
        (
            "no replacement",
            vec![unit("other.service", UnitScope::User, true)],
            false,
        ),
        (
            "disabled replacement",
            vec![unit("gtk-settings.service", UnitScope::User, false)],
            false,
        ),
        (
            "system replacement",
            vec![unit("gtk-settings.service", UnitScope::System, true)],
            false,
        ),
        (
            "explicit user unit",
            vec![
                gtk.clone(),
                unit("nautilus-settings.service", UnitScope::User, true),
            ],
            false,
        ),
        (
            "unrelated system unit",
            vec![
                gtk,
                unit("nautilus-settings.service", UnitScope::System, true),
            ],
            true,
        ),
    ] {
        assert_eq!(should_retire_nautilus_settings(&units), expected, "{label}");
    }
}

#[cfg(unix)]
#[test]
fn gtk_service_migration_previews_then_removes_retired_links_once() {
    let task = systemd_task("gtk-settings.service", UnitScope::User, true);
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("checkout");
    let home = fixture.path().join("home");
    let source_dir = root.join("symlinks/config/systemd/user");
    let user_dir = home.join(".config/systemd/user");
    let wants = user_dir.join("graphical-session.target.wants");
    std::fs::create_dir_all(&source_dir).unwrap();
    std::fs::create_dir_all(&wants).unwrap();
    let old_name = "nautilus-settings.service";
    let old_source = source_dir.join(old_name);
    let old_unit = user_dir.join(old_name);
    let old_enablement = wants.join(old_name);
    std::os::unix::fs::symlink(&old_source, &old_unit).unwrap();
    std::os::unix::fs::symlink(&old_source, &old_enablement).unwrap();
    let gtk = user_dir.join("gtk-settings.service");
    std::fs::write(&gtk, "[Install]\nWantedBy=graphical-session.target\n").unwrap();
    std::os::unix::fs::symlink(&gtk, wants.join("gtk-settings.service")).unwrap();

    let mut executor = script();
    for _ in 0..3 {
        executor = executor.command(
            user_manager_probe(),
            Ok(ExecResult::failure(
                "",
                "Failed to connect to bus: No medium found",
                Some(1),
            )),
        );
    }
    let ctx = make_context(
        root,
        None,
        Platform::new(Os::Linux, true),
        Arc::new(executor),
    )
    .with_home(home);
    let preview = task.run(&ctx.with_dry_run(true)).unwrap();
    assert_eq!(crate::test_helpers::task_batch(&preview).changed_count(), 1);
    assert_eq!(std::fs::read_link(&old_unit).unwrap(), old_source);
    assert_eq!(std::fs::read_link(&old_enablement).unwrap(), old_source);
    let applied = task.run(&ctx).unwrap();
    assert_eq!(crate::test_helpers::task_batch(&applied).changed_count(), 1);
    assert!(!old_unit.is_symlink());
    assert!(!old_enablement.is_symlink());
    let repeated = task.run(&ctx).unwrap();
    assert_eq!(
        crate::test_helpers::task_batch(&repeated).changed_count(),
        0
    );
}
