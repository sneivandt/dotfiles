//! Task: configure systemd units.

use std::sync::Arc;

use anyhow::{Context as _, Result};

use crate::domains::system::config::systemd_units::{SystemdUnit, UnitScope};
use crate::domains::system::resources::systemd_unit::SystemdUnitResource;
use crate::engine::{
    Context, IntrinsicState, ProcessOpts, ResourceState, Task, TaskMeta, TaskResult,
    process_resources,
};
use crate::infra::ConfigHandle;
use crate::infra::exec::CommandSpec;
use crate::infra::logging::OutputExt as _;

/// Enable and start systemd units.
#[derive(Debug)]
pub struct ConfigureSystemd {
    config: ConfigHandle<Vec<SystemdUnit>>,
}

impl ConfigureSystemd {
    /// Create the task with a handle to the systemd unit configuration.
    #[must_use]
    pub const fn new(config: ConfigHandle<Vec<SystemdUnit>>) -> Self {
        Self { config }
    }
}

impl Task for ConfigureSystemd {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new("Systemd units").with_selector("systemd")
    }

    fn should_run(&self, ctx: &Context) -> bool {
        let system = ctx.system();
        system.platform().supports_systemd()
            && !self.config.read().is_empty()
            && system.which("systemctl")
            && systemd_available(ctx)
            && !system.is_ci()
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        system_unit_needs_enablement(ctx, &self.config.read())
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let units = self.config.read().to_vec();
        if units.is_empty() {
            return Ok(TaskResult::NotApplicable("nothing configured".to_string()));
        }

        let user_manager_available = user_manager_available(ctx, &units);
        let system_reload_required = system_unit_needs_enablement(ctx, &units);
        reload_daemons(ctx, user_manager_available, system_reload_required)?;

        let system = ctx.system();
        let resources = units.iter().map(|entry| {
            SystemdUnitResource::from_entry(
                entry,
                system.executor_arc(),
                system.home(),
                user_manager_available,
            )
        });
        process_resources(
            ctx,
            resources,
            &ProcessOpts::install_missing("enable").sequential(),
        )
    }
}

fn system_unit_needs_enablement(ctx: &Context, units: &[SystemdUnit]) -> bool {
    let executor = ctx.system().executor_arc();
    units
        .iter()
        .filter(|unit| unit.scope == UnitScope::System)
        .any(|unit| {
            matches!(
                SystemdUnitResource::new(
                    unit.name.clone(),
                    UnitScope::System,
                    Arc::clone(&executor),
                )
                .current_state(),
                Ok(ResourceState::Missing)
            )
        })
}

fn systemd_available(ctx: &Context) -> bool {
    let system = ctx.system();
    if system.platform().is_wsl() {
        system
            .executor()
            .execute(
                CommandSpec::new("systemctl")
                    .arg("is-system-running")
                    .unchecked(),
            )
            .is_ok_and(|result| result.success || result.stdout.trim() == "degraded")
    } else {
        true
    }
}

fn user_manager_available(ctx: &Context, units: &[SystemdUnit]) -> bool {
    if !units.iter().any(|unit| unit.scope == UnitScope::User) {
        return false;
    }
    if crate::infra::provisioning::is_arch_chroot(ctx.env().as_ref()) {
        ctx.log().debug(
            "Arch chroot provisioning has no user session; enabling user units offline for the next login",
        );
        return false;
    }
    let available = ctx
        .executor()
        .execute(
            CommandSpec::new("systemctl")
                .args(&["--user", "show-environment"])
                .unchecked(),
        )
        .is_ok_and(|result| result.success);
    if !available {
        ctx.log().debug(
            "user systemd manager unavailable; enabling user units offline for the next login",
        );
    }
    available
}

fn reload_daemons(
    ctx: &Context,
    user_manager_available: bool,
    system_reload_required: bool,
) -> Result<()> {
    if ctx.dry_run() {
        return Ok(());
    }

    if user_manager_available {
        ctx.log().debug("running systemctl --user daemon-reload");
        ctx.executor()
            .execute(CommandSpec::new("systemctl").args(&["--user", "daemon-reload"]))
            .context("reloading user systemd daemon")?;
        ctx.log().debug("user daemon-reload succeeded");
    }

    if system_reload_required {
        ctx.log().debug("running sudo systemctl daemon-reload");
        ctx.executor()
            .execute(CommandSpec::new("sudo").args(&["systemctl", "daemon-reload"]))
            .context("reloading system systemd daemon")?;
        ctx.log().debug("system daemon-reload succeeded");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::system::config::systemd_units::SystemdUnit;
    use crate::engine::{Context, Task, TaskResult};
    use crate::infra::ConfigHandle;
    #[cfg(unix)]
    use crate::infra::env::MapEnv;
    use crate::infra::exec::{ExecError, ExecResult, MockExecutor};
    use crate::infra::platform::{Os, Platform};
    use crate::test_helpers::{
        ContextBuilder, empty_config, make_context, make_linux_context,
        make_platform_context_with_which,
    };
    use std::path::PathBuf;
    use std::sync::Arc;

    fn disabled_result() -> ExecResult {
        ExecResult::failure("disabled\n", "", Some(1))
    }

    #[test]
    fn should_run_false_on_windows() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
            scope: UnitScope::User,
        });
        let units = ConfigHandle::new(config.units.clone());
        let ctx = make_platform_context_with_which(config, Os::Windows, false, true);
        assert!(!ConfigureSystemd::new(units).should_run(&ctx));
    }

    #[test]
    fn should_run_false_when_units_empty() {
        let config = empty_config(PathBuf::from("/tmp"));
        let units = ConfigHandle::new(config.units.clone());
        let ctx = make_platform_context_with_which(config, Os::Linux, false, true);
        assert!(!ConfigureSystemd::new(units).should_run(&ctx));
    }

    #[test]
    fn should_run_false_when_systemctl_not_found() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
            scope: UnitScope::User,
        });
        let units = ConfigHandle::new(config.units.clone());
        let ctx = make_linux_context(config); // which() returns false
        assert!(!ConfigureSystemd::new(units).should_run(&ctx));
    }

    #[test]
    fn should_run_false_when_ci() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
            scope: UnitScope::User,
        });
        let units = ConfigHandle::new(config.units.clone());
        let ctx = ContextBuilder::new(config)
            .os(Os::Linux)
            .which(true)
            .ci(true)
            .build();
        assert!(!ConfigureSystemd::new(units).should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_linux_with_units_and_systemctl() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
            scope: UnitScope::User,
        });
        let units = ConfigHandle::new(config.units.clone());
        let ctx = ContextBuilder::new(config)
            .os(Os::Linux)
            .which(true)
            .ci(false)
            .build();
        assert!(ConfigureSystemd::new(units).should_run(&ctx));
    }

    // ------------------------------------------------------------------
    // ConfigureSystemd::run
    // ------------------------------------------------------------------

    /// Build a context backed by `MockExecutor` for `run()` tests.
    fn make_systemd_context(config: crate::Config, executor: MockExecutor) -> Context {
        make_context(config, Platform::new(Os::Linux, false), Arc::new(executor))
    }

    #[test]
    fn run_calls_daemon_reload_before_enabling_unit() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
            scope: UnitScope::User,
        });
        // Ordered expectations:
        //   1. probe the live user manager
        //   2. run("systemctl", ["--user", "daemon-reload"]) -> success
        //   3. run_unchecked("systemctl", ["--user", "is-enabled", "dunst.service"]) -> disabled
        //   4. run_unchecked("systemctl", ["--user", "enable", "--now", "dunst.service"]) → success
        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .returning(|_| Ok(ExecResult::success("")));
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .returning(|_| Ok(ExecResult::success("")));
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .returning(|_| Ok(disabled_result()));
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .returning(|_| Ok(ExecResult::success("")));
        let units = ConfigHandle::new(config.units.clone());
        let ctx = make_systemd_context(config, mock);

        let result = ConfigureSystemd::new(units).run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() == 1),
            "expected one changed action after daemon-reload + enable, got {result:?}"
        );
    }

    #[test]
    fn run_skips_daemon_reload_in_dry_run() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
            scope: UnitScope::User,
        });
        // In dry-run mode daemon-reload is NOT called (guarded by `!ctx.dry_run`).
        // The manager probe and current_state() still run to decide whether a
        // change would be needed.
        let mut mock = MockExecutor::new();
        mock.expect_execute().times(2).returning(|spec| {
            if spec.arguments() == ["--user", "show-environment"] {
                Ok(ExecResult::success(""))
            } else {
                Ok(disabled_result())
            }
        });
        let units = ConfigHandle::new(config.units.clone());
        let mut ctx = make_systemd_context(config, mock);
        ctx = ctx.with_dry_run(true);

        let result = ConfigureSystemd::new(units).run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() == 1),
            "expected one planned action when unit is missing in dry-run mode, got {result:?}"
        );
    }

    #[test]
    fn run_propagates_user_daemon_reload_failure() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
            scope: UnitScope::User,
        });
        let mut mock = MockExecutor::new();
        let mut seq = mockall::Sequence::new();
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .withf(|spec| {
                spec.program() == "systemctl"
                    && spec.arguments() == ["--user", "show-environment"]
                    && !spec.is_checked()
            })
            .returning(|_| Ok(ExecResult::success("")));
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .withf(|spec| {
                spec.program() == "systemctl"
                    && spec.arguments() == ["--user", "daemon-reload"]
                    && spec.is_checked()
            })
            .returning(|_| {
                Err(ExecError::spawn(
                    "systemctl",
                    std::io::Error::other("reload failed"),
                ))
            });
        let units = ConfigHandle::new(config.units.clone());
        let ctx = make_systemd_context(config, mock);

        let error = ConfigureSystemd::new(units)
            .run(&ctx)
            .expect_err("daemon-reload failure should abort the task");
        assert!(error.to_string().contains("reloading user systemd daemon"));
    }

    #[cfg(unix)]
    #[test]
    fn run_enables_user_units_offline_when_the_user_manager_is_unavailable() {
        let home = tempfile::tempdir().unwrap();
        let unit_dir = home.path().join(".config/systemd/user");
        std::fs::create_dir_all(&unit_dir).unwrap();
        std::fs::write(
            unit_dir.join("clean-home-tmp.timer"),
            "[Install]\nWantedBy=timers.target\n",
        )
        .unwrap();
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "clean-home-tmp.timer".to_string(),
            scope: UnitScope::User,
        });
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .withf(|spec| {
                spec.program() == "systemctl"
                    && spec.arguments() == ["--user", "show-environment"]
                    && !spec.is_checked()
            })
            .returning(|_| {
                Ok(ExecResult::failure(
                    "",
                    "Failed to connect to bus: No medium found",
                    Some(1),
                ))
            });
        let units = ConfigHandle::new(config.units.clone());
        let ctx = make_systemd_context(config, mock).with_home(home.path().to_path_buf());

        let result = ConfigureSystemd::new(units).run(&ctx).unwrap();

        assert!(
            matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() == 1),
            "offline enablement should be reported as changed: {result:?}"
        );
        assert!(
            unit_dir
                .join("timers.target.wants/clean-home-tmp.timer")
                .is_symlink(),
            "offline enablement should converge before the first login"
        );
    }

    #[cfg(unix)]
    #[test]
    fn arch_chroot_provisioning_never_probes_the_user_manager() {
        let home = tempfile::tempdir().unwrap();
        let unit_dir = home.path().join(".config/systemd/user");
        std::fs::create_dir_all(&unit_dir).unwrap();
        std::fs::write(
            unit_dir.join("clean-home-tmp.timer"),
            "[Install]\nWantedBy=timers.target\n",
        )
        .unwrap();
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "clean-home-tmp.timer".to_string(),
            scope: UnitScope::User,
        });
        let units = ConfigHandle::new(config.units.clone());
        let ctx = make_systemd_context(config, MockExecutor::new())
            .with_home(home.path().to_path_buf())
            .with_env(
                MapEnv::new()
                    .with(
                        crate::infra::provisioning::ENV_VAR,
                        crate::infra::provisioning::ARCH_CHROOT,
                    )
                    .into_handle(),
            );

        let result = ConfigureSystemd::new(units).run(&ctx).unwrap();

        assert!(
            matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() == 1),
            "offline provisioning should report one changed unit: {result:?}"
        );
        assert!(
            unit_dir
                .join("timers.target.wants/clean-home-tmp.timer")
                .is_symlink(),
            "offline provisioning should enable the timer without using the user bus"
        );
    }

    #[test]
    fn run_propagates_system_daemon_reload_failure() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "sshd.service".to_string(),
            scope: UnitScope::System,
        });
        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .withf(|spec| {
                spec.program() == "systemctl"
                    && spec.arguments() == ["is-enabled", "sshd.service"]
                    && !spec.is_checked()
            })
            .returning(|_| Ok(disabled_result()));
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .withf(|spec| {
                spec.program() == "sudo"
                    && spec.arguments() == ["systemctl", "daemon-reload"]
                    && spec.is_checked()
            })
            .returning(|_| {
                Err(ExecError::spawn(
                    "sudo",
                    std::io::Error::other("reload failed"),
                ))
            });
        let units = ConfigHandle::new(config.units.clone());
        let ctx = make_systemd_context(config, mock);

        let error = ConfigureSystemd::new(units)
            .run(&ctx)
            .expect_err("daemon-reload failure should abort the task");
        assert!(
            error
                .to_string()
                .contains("reloading system systemd daemon")
        );
    }

    #[test]
    fn needs_sudo_true_for_disabled_system_scope_unit() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "sshd.service".to_string(),
            scope: UnitScope::System,
        });
        let mut mock = MockExecutor::new();
        mock.expect_which()
            .once()
            .with(mockall::predicate::eq("systemctl"))
            .return_const(true);
        mock.expect_execute()
            .once()
            .withf(|spec| {
                spec.program() == "systemctl"
                    && spec.arguments() == ["is-enabled", "sshd.service"]
                    && !spec.is_checked()
            })
            .returning(|_| Ok(disabled_result()));
        let units = ConfigHandle::new(config.units.clone());
        let ctx = make_systemd_context(config, mock);

        assert!(crate::engine::requires_elevation(
            &ConfigureSystemd::new(units),
            &ctx
        ));
    }

    #[test]
    fn needs_sudo_false_for_enabled_system_scope_unit() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "NetworkManager.service".to_string(),
            scope: UnitScope::System,
        });
        let mut mock = MockExecutor::new();
        mock.expect_which()
            .once()
            .with(mockall::predicate::eq("systemctl"))
            .return_const(true);
        mock.expect_execute()
            .once()
            .withf(|spec| {
                spec.program() == "systemctl"
                    && spec.arguments() == ["is-enabled", "NetworkManager.service"]
                    && !spec.is_checked()
            })
            .returning(|_| Ok(ExecResult::success("enabled\n")));
        let units = ConfigHandle::new(config.units.clone());
        let ctx = make_systemd_context(config, mock);

        assert!(!crate::engine::requires_elevation(
            &ConfigureSystemd::new(units),
            &ctx
        ));
    }

    #[test]
    fn run_does_not_reload_enabled_system_scope_units() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "NetworkManager.service".to_string(),
            scope: UnitScope::System,
        });
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .times(2)
            .withf(|spec| {
                spec.program() == "systemctl"
                    && spec.arguments() == ["is-enabled", "NetworkManager.service"]
                    && !spec.is_checked()
            })
            .returning(|_| Ok(ExecResult::success("enabled\n")));
        let units = ConfigHandle::new(config.units.clone());
        let ctx = make_systemd_context(config, mock);

        let result = ConfigureSystemd::new(units).run(&ctx).unwrap();

        assert!(
            matches!(result, TaskResult::Batch(ref stats) if stats.already_ok_count() == 1),
            "enabled system units should remain a no-op without sudo: {result:?}"
        );
    }

    #[test]
    fn run_reloads_and_enables_system_scope_units_with_sudo() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "sshd.service".to_string(),
            scope: UnitScope::System,
        });
        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .withf(|spec| {
                spec.program() == "systemctl"
                    && spec.arguments() == ["is-enabled", "sshd.service"]
                    && !spec.is_checked()
            })
            .returning(|_| Ok(disabled_result()));
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .withf(|spec| {
                spec.program() == "sudo"
                    && spec.arguments() == ["systemctl", "daemon-reload"]
                    && spec.is_checked()
            })
            .returning(|_| Ok(ExecResult::success("")));
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .withf(|spec| {
                spec.program() == "systemctl"
                    && spec.arguments() == ["is-enabled", "sshd.service"]
                    && !spec.is_checked()
            })
            .returning(|_| Ok(disabled_result()));
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .withf(|spec| {
                spec.program() == "sudo"
                    && spec.arguments() == ["systemctl", "enable", "--now", "sshd.service"]
                    && !spec.is_checked()
            })
            .returning(|_| Ok(ExecResult::success("")));
        let units = ConfigHandle::new(config.units.clone());
        let ctx = make_systemd_context(config, mock);

        let result = ConfigureSystemd::new(units).run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() == 1),
            "expected one changed action after system-scope daemon-reload + enable, got {result:?}"
        );
    }
}
