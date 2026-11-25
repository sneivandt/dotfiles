//! Task: configure global git settings.

use anyhow::Result;
use std::path::PathBuf;

use crate::domains::git::config::git_config::GitSetting;
use crate::domains::git::resources::git_config::{
    GitConfigPlan, GitConfigResource, config_access_claim,
};
use crate::engine::{
    AccessClaim, Context, Operation, OperationState, Resource, Task, TaskResult, TaskStats,
    process_operation, task_metadata,
};
use crate::infra::ConfigHandle;

/// Configure global git settings.
#[derive(Debug)]
pub struct ConfigureGit {
    config: ConfigHandle<Vec<GitSetting>>,
    config_path: Option<PathBuf>,
}

const NAME: &str = "Global Git settings";

impl ConfigureGit {
    /// Create the task with a handle to its configuration slice.
    #[must_use]
    pub const fn new(config: ConfigHandle<Vec<GitSetting>>) -> Self {
        Self {
            config,
            config_path: None,
        }
    }

    /// Create the task with all settings scoped to one explicit config file.
    #[must_use]
    pub const fn with_config_path(
        config: ConfigHandle<Vec<GitSetting>>,
        config_path: PathBuf,
    ) -> Self {
        Self {
            config,
            config_path: Some(config_path),
        }
    }
}

impl Task for ConfigureGit {
    task_metadata! {
        name: NAME,
        selector: "git",
    }

    fn access_claims(&self, _ctx: &Context) -> Vec<AccessClaim> {
        vec![config_access_claim()]
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let settings = self.config.get();
        if settings.is_empty() {
            return Ok(TaskResult::NotApplicable("no Git settings".into()));
        }
        let config_path = self.config_path.clone().map_or_else(
            || {
                if let Some(path) = ctx.env().var_os("GIT_CONFIG_GLOBAL") {
                    anyhow::ensure!(!path.is_empty(), "GIT_CONFIG_GLOBAL is empty");
                    return Ok(std::path::absolute(path)?);
                }
                Ok(global_config_path(git2::Config::find_global(), ctx.home())?)
            },
            Ok,
        )?;
        let settings: Vec<_> = settings
            .iter()
            .map(|setting| GitConfigResource::new(setting.key.clone(), setting.value.clone()))
            .collect();
        let operation = ConfigureGitFile {
            path: config_path,
            settings,
        };
        let result = process_operation(ctx, &operation)?;
        if matches!(result, TaskResult::Ok) {
            return operation.report(ctx, &[], ctx.dry_run());
        }
        Ok(result)
    }
}

struct ConfigureGitFile {
    path: PathBuf,
    settings: Vec<GitConfigResource>,
}

impl ConfigureGitFile {
    fn report(
        &self,
        ctx: &Context,
        changed: &[GitConfigResource],
        dry_run: bool,
    ) -> Result<TaskResult> {
        for setting in changed {
            let subject = setting.description();
            ctx.log().action(
                "configure",
                &subject,
                dry_run,
                &format!("configure {subject}"),
            );
        }
        Ok(TaskStats::from_counts(
            u32::try_from(changed.len())?,
            u32::try_from(self.settings.len() - changed.len())?,
            0,
            0,
        )
        .finish())
    }
}

impl Operation for ConfigureGitFile {
    type Plan = GitConfigPlan;

    fn current_state(&self, ctx: &Context) -> Result<OperationState<Self::Plan>> {
        let plan = GitConfigPlan::inspect(&self.path, &self.settings)?;
        for setting in &self.settings {
            if !plan
                .changed()
                .iter()
                .any(|changed| changed.key == setting.key)
            {
                ctx.debug_fmt(|| format!("ok {}", setting.description()));
            }
        }
        if plan.changed().is_empty() {
            Ok(OperationState::Complete)
        } else {
            Ok(OperationState::needs_run(plan))
        }
    }

    fn preview(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        self.report(ctx, plan.changed(), true)
    }

    fn apply(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        plan.apply(&self.path)?;
        self.report(ctx, plan.changed(), false)
    }
}

fn global_config_path(
    found: Result<PathBuf, git2::Error>,
    home: &std::path::Path,
) -> Result<PathBuf, git2::Error> {
    match found {
        Ok(path) => Ok(path),
        Err(error) if error.code() == git2::ErrorCode::NotFound => Ok(home.join(".gitconfig")),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_config_lookup_preserves_operational_errors_instead_of_using_fallback() {
        let home = std::path::Path::new("fixture-home");
        let existing = PathBuf::from("existing-config");
        assert_eq!(
            global_config_path(Ok(existing.clone()), home).unwrap(),
            existing
        );
        for code in [
            git2::ErrorCode::NotFound,
            git2::ErrorCode::Locked,
            git2::ErrorCode::GenericError,
        ] {
            let failure =
                git2::Error::new(code, git2::ErrorClass::Config, "fixture lookup failure");
            let result = global_config_path(Err(failure), home);
            if code == git2::ErrorCode::NotFound {
                assert_eq!(result.unwrap(), home.join(".gitconfig"));
            } else {
                let error = result.unwrap_err();
                assert_eq!(error.code(), code);
                assert!(error.message().contains("fixture lookup failure"));
            }
        }
    }

    #[test]
    fn configure_git_declares_access_claim_and_logs_only_unchanged_settings_as_ok() {
        use std::sync::Arc;
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("gitconfig");
        std::fs::write(&config_path, "[core]\n autocrlf = false\n").unwrap();

        let log = Arc::new(crate::test_helpers::CapturingOutput::default());
        let log_clone = Arc::clone(&log);
        let log_handle: Arc<dyn crate::infra::logging::Log> = log_clone;
        let ctx = Context::from_raw(
            dir.path().to_path_buf(),
            None,
            crate::infra::platform::Platform::new(crate::infra::platform::Os::Linux, false),
            log_handle,
            Arc::new(crate::infra::exec::MockExecutor::new()),
            dir.path().to_path_buf(),
            crate::engine::ContextOpts::default(),
        );
        let task = ConfigureGit::new(ConfigHandle::new(vec![]));
        assert_eq!(task.access_claims(&ctx), vec![config_access_claim()]);

        let op = ConfigureGitFile {
            path: config_path,
            settings: vec![
                GitConfigResource::new("core.autocrlf".into(), "false".into()),
                GitConfigResource::new("user.name".into(), "Alice".into()),
            ],
        };
        let state = op.current_state(&ctx).unwrap();
        assert!(matches!(state, OperationState::NeedsRun { .. }));
        let debug = log.messages_of(crate::infra::logging::MsgKind::Debug);
        assert!(
            debug.iter().any(|m| m.contains("ok core.autocrlf = false")),
            "already-correct setting should log ok: {debug:?}"
        );
        assert!(
            !debug.iter().any(|m| m.contains("ok user.name = Alice")),
            "changed setting must not log ok: {debug:?}"
        );

        // ConfigureGitFile::report must subtract changed from total (3 - 1 == 2, != 4, != 3).
        let stats_op = ConfigureGitFile {
            path: dir.path().join("gitconfig-stats"),
            settings: vec![
                GitConfigResource::new("a.b".into(), "1".into()),
                GitConfigResource::new("c.d".into(), "2".into()),
                GitConfigResource::new("e.f".into(), "3".into()),
            ],
        };
        let reported = stats_op
            .report(&ctx, &stats_op.settings[..1], false)
            .unwrap();
        let report_stats = crate::test_helpers::task_batch(&reported);
        assert_eq!(report_stats.changed_count(), 1);
        assert_eq!(report_stats.already_ok_count(), 2);
    }

    #[test]
    fn configure_git_is_not_applicable_when_empty_and_preserves_unmanaged_keys() {
        use std::sync::Arc;
        let dir = tempfile::tempdir().unwrap();
        let make_platform_ctx = |os: crate::infra::platform::Os, home: &std::path::Path| {
            let log_handle: Arc<dyn crate::infra::logging::Log> =
                Arc::new(crate::test_helpers::CapturingOutput::default());
            Context::from_raw(
                home.to_path_buf(),
                None,
                crate::infra::platform::Platform::new(os, false),
                log_handle,
                Arc::new(crate::infra::exec::MockExecutor::new()),
                home.to_path_buf(),
                crate::engine::ContextOpts::default(),
            )
        };

        for os in [
            crate::infra::platform::Os::Linux,
            crate::infra::platform::Os::Windows,
        ] {
            let home = dir.path().join(format!("{os:?}"));
            std::fs::create_dir(&home).unwrap();
            let cfg = home.join(".gitconfig");
            std::fs::write(&cfg, "[core]\n\tautocrlf = true\n").unwrap();
            let ctx = make_platform_ctx(os, &home);

            let empty_task = ConfigureGit::with_config_path(ConfigHandle::new(vec![]), cfg.clone());
            assert!(
                matches!(empty_task.run(&ctx).unwrap(), TaskResult::NotApplicable(_)),
                "{os:?} with empty settings must be NotApplicable"
            );
            assert!(
                std::fs::read_to_string(&cfg)
                    .unwrap()
                    .contains("autocrlf = true"),
                "{os:?} empty run must preserve unmanaged core.autocrlf"
            );

            let task = ConfigureGit::with_config_path(
                ConfigHandle::new(vec![GitSetting {
                    key: "user.name".into(),
                    value: "Alice".into(),
                    origin: None,
                }]),
                cfg.clone(),
            );
            let res = task.run(&ctx).unwrap();
            crate::test_helpers::assert_task_changed(&res);
            let after = std::fs::read_to_string(&cfg).unwrap();
            assert!(
                after.contains("autocrlf = true"),
                "{os:?} must not remove unmanaged core.autocrlf: {after}"
            );
        }
    }
}
