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
