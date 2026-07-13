//! Application-owned configuration reload after a repository update.
//!
//! Reloading re-parses every TOML file and re-composes the aggregate
//! [`Config`](crate::app::config::Config), then swaps each per-domain handle in
//! the shared [`ConfigStore`].  Because composing the aggregate configuration is
//! an application concern (it spans every domain), this task lives in the `app`
//! layer rather than in any single domain.

use anyhow::{Context as _, Result};

use crate::app::config::Config;
use crate::app::config::store::ConfigStore;
use crate::engine::{
    Context, Domain, Operation, OperationState, Task, TaskPhase, TaskResult, UpdateSignal,
    process_operation, task_metadata,
};

/// Re-parse all configuration files after `UpdateRepository` has pulled the
/// latest changes and swap the freshly-loaded values into the shared store.
///
/// Every task that reads configuration does so through a handle owned by the
/// [`ConfigStore`]; swapping those handles here makes the new configuration
/// visible to all downstream tasks without rebuilding them.
#[derive(Debug)]
pub struct ReloadConfig {
    /// Shared flag set by the repository-update task when new commits were
    /// fetched.  When `false`, the reload is a no-op.
    repo_updated: UpdateSignal,
    /// Shared configuration store whose handles are swapped on reload.
    store: ConfigStore,
}

impl ReloadConfig {
    /// Create a new task, sharing `repo_updated` with the repository-update
    /// task and the [`ConfigStore`] with every configuration-reading task.
    #[must_use]
    pub const fn new(repo_updated: UpdateSignal, store: ConfigStore) -> Self {
        Self {
            repo_updated,
            store,
        }
    }

    fn reload(&self, ctx: &Context) -> Result<TaskResult> {
        // Re-load configuration using the repository parameters recorded on the
        // current aggregate snapshot; the root, profile, and overlay are fixed
        // for the lifetime of the process, so re-reading them here is safe.
        let new_config = {
            let old = self.store.aggregate.read();
            Config::load(
                &old.root,
                &old.profile,
                ctx.platform,
                old.overlay.as_deref(),
            )
            .context("reloading configuration after repository update")?
        };

        ctx.debug_fmt(|| {
            format!(
                "{} packages, {} symlinks after reload",
                new_config.packages.len(),
                new_config.symlinks.len()
            )
        });

        self.store.reload(new_config);

        ctx.log.info("configuration reloaded");
        Ok(TaskResult::Ok)
    }
}

struct ReloadConfigOperation<'a> {
    task: &'a ReloadConfig,
}

impl Operation for ReloadConfigOperation<'_> {
    type Plan = ();

    fn current_state(&self, _ctx: &Context) -> Result<OperationState<Self::Plan>> {
        if self.task.repo_updated.was_updated() {
            Ok(OperationState::needs_run("repository changed", ()))
        } else {
            Ok(OperationState::not_applicable(
                "repository was already current",
            ))
        }
    }

    fn preview(&self, _ctx: &Context, _plan: &Self::Plan) -> Result<TaskResult> {
        Ok(TaskResult::DryRun)
    }

    fn apply(&self, ctx: &Context, _plan: &Self::Plan) -> Result<TaskResult> {
        self.task.reload(ctx)
    }
}

impl Task for ReloadConfig {
    task_metadata! {
        name: "Reload configuration",
        phase: TaskPhase::Sync,
        domain: Domain::Repository,
        deps: [crate::domains::repository::tasks::update::UpdateRepository],
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        self.repo_updated.was_updated()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        process_operation(ctx, &ReloadConfigOperation { task: self })
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::app::config::profiles::Profile;
    use crate::engine::UpdateSignal;
    use crate::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    fn make_task(root: PathBuf, signal: UpdateSignal) -> ReloadConfig {
        let store = ConfigStore::from_config(empty_config(root));
        ReloadConfig::new(signal, store)
    }

    #[test]
    fn should_not_run_when_repo_not_updated() {
        let ctx = make_linux_context(empty_config(PathBuf::from("/tmp")));
        let task = make_task(PathBuf::from("/tmp"), UpdateSignal::new());
        assert!(!task.should_run(&ctx));
    }

    #[test]
    fn should_run_when_repo_updated() {
        let ctx = make_linux_context(empty_config(PathBuf::from("/tmp")));
        let signal = UpdateSignal::new();
        signal.mark_updated();
        let task = make_task(PathBuf::from("/tmp"), signal);
        assert!(task.should_run(&ctx));
    }

    #[test]
    fn run_reloads_config_when_repo_updated() {
        let dir = tempfile::tempdir().unwrap();
        let conf = dir.path().join("conf");
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::write(
            conf.join("profiles.toml"),
            "[base]\ninclude = []\nexclude = [\"desktop\"]\n",
        )
        .unwrap();
        for file in &[
            "symlinks.toml",
            "packages.toml",
            "manifest.toml",
            "chmod.toml",
            "systemd-units.toml",
            "vscode-extensions.toml",
            "git-config.toml",
            "registry.toml",
        ] {
            std::fs::write(conf.join(file), "").unwrap();
        }

        let ctx = make_linux_context(empty_config(dir.path().to_path_buf()));
        let repo_updated = UpdateSignal::new();
        repo_updated.mark_updated();
        // Build a store whose aggregate carries a base profile so the reload
        // re-loads with matching category selection.
        let mut config = empty_config(dir.path().to_path_buf());
        config.profile = Profile {
            name: "base".to_string(),
            active_categories: vec![
                crate::runtime::config_support::category_matcher::Category::Base,
            ],
            excluded_categories: vec![],
        };
        let store = ConfigStore::from_config(config);
        let task = ReloadConfig::new(repo_updated, store);
        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Ok));
    }
}
