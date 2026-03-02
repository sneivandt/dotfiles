//! Task: reload configuration after repository update.
use anyhow::{Context as _, Result};

use super::{Context, Task, TaskResult, UpdateSignal, task_deps};

/// Re-parse all configuration files after `UpdateRepository` has pulled the
/// latest changes.
///
/// Because `Config` is wrapped in an `Arc<RwLock<Config>>`, this task can
/// atomically swap the shared configuration seen by every downstream task.
/// All tasks that read from `ctx.config_read()` must declare this task as a
/// dependency so they always operate on post-pull configuration.
#[derive(Debug)]
pub struct ReloadConfig {
    /// Shared flag set by [`super::update::UpdateRepository`] when new commits
    /// were fetched.  When `false`, the reload is a no-op.
    pub(super) repo_updated: UpdateSignal,
}

impl ReloadConfig {
    /// Create a new task, sharing `repo_updated` with `UpdateRepository`.
    #[must_use]
    pub const fn new(repo_updated: UpdateSignal) -> Self {
        Self { repo_updated }
    }
}

impl Task for ReloadConfig {
    fn name(&self) -> &'static str {
        "Reload configuration"
    }

    task_deps![super::update::UpdateRepository];

    fn should_run(&self, _ctx: &Context) -> bool {
        self.repo_updated.was_updated()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        // Load the new config while holding only a read lock, so there is no
        // window where the lock is held for writing across I/O.
        let new_config = {
            let old = ctx.config_read();
            crate::config::Config::load(&old.root, &old.profile, &ctx.platform)
                .context("reloading configuration after repository update")?
        };

        ctx.log.debug(&format!(
            "{} packages, {} symlinks after reload",
            new_config.packages.len(),
            new_config.symlinks.len()
        ));

        // Atomically replace the shared config so all downstream tasks see the
        // freshly-loaded values.
        let mut guard = ctx
            .config
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = new_config;
        drop(guard);

        ctx.log.info("configuration reloaded");
        Ok(TaskResult::Ok)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::tasks::UpdateSignal;
    use crate::tasks::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    #[test]
    fn should_not_run_when_repo_not_updated() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        let task = ReloadConfig::new(UpdateSignal::new());
        assert!(!task.should_run(&ctx));
    }

    #[test]
    fn should_run_when_repo_updated() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        let signal = UpdateSignal::new();
        signal.mark_updated();
        let task = ReloadConfig::new(signal);
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
            "copilot-skills.toml",
            "git-config.toml",
            "registry.toml",
        ] {
            std::fs::write(conf.join(file), "").unwrap();
        }

        let mut config = empty_config(dir.path().to_path_buf());
        config.profile = crate::config::profiles::Profile {
            name: "base".to_string(),
            active_categories: vec![crate::config::category_matcher::Category::Base],
            excluded_categories: vec![],
        };
        let ctx = make_linux_context(config);
        let repo_updated = UpdateSignal::new();
        repo_updated.mark_updated();
        let task = ReloadConfig::new(repo_updated);
        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Ok));
    }
}
