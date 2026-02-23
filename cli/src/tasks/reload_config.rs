use anyhow::{Context as _, Result};
use std::any::TypeId;

use super::{Context, Task, TaskResult};

/// Re-parse all configuration files after `UpdateRepository` has pulled the
/// latest changes.
///
/// Because `Config` is wrapped in an `Arc<RwLock<Config>>`, this task can
/// atomically swap the shared configuration seen by every downstream task.
/// All tasks that read from `ctx.config_read()` must declare this task as a
/// dependency so they always operate on post-pull configuration.
#[derive(Debug)]
pub struct ReloadConfig;

impl Task for ReloadConfig {
    fn name(&self) -> &'static str {
        "Reload configuration"
    }

    fn dependencies(&self) -> &[TypeId] {
        const DEPS: &[TypeId] = &[TypeId::of::<super::update::UpdateRepository>()];
        DEPS
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        true
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if !ctx.repo_updated.load(std::sync::atomic::Ordering::Acquire) {
            ctx.log
                .debug("repository was not updated, skipping config reload");
            return Ok(TaskResult::Skipped("repository not updated".to_string()));
        }

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
    use crate::tasks::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;
    use std::sync::atomic::Ordering;

    #[test]
    fn should_run_always() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(ReloadConfig.should_run(&ctx));
    }

    #[test]
    fn run_skips_when_repo_not_updated() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        // repo_updated defaults to false
        assert!(!ctx.repo_updated.load(Ordering::Acquire));
        let result = ReloadConfig.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Skipped(_)));
    }

    #[test]
    fn run_reloads_config_when_repo_updated() {
        let dir = tempfile::tempdir().unwrap();
        let conf = dir.path().join("conf");
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::write(
            conf.join("profiles.ini"),
            "[base]\ninclude=\nexclude=desktop\n",
        )
        .unwrap();
        for file in &[
            "symlinks.ini",
            "packages.ini",
            "manifest.ini",
            "chmod.ini",
            "systemd-units.ini",
            "vscode-extensions.ini",
            "copilot-skills.ini",
            "registry.ini",
        ] {
            std::fs::write(conf.join(file), "").unwrap();
        }

        let mut config = empty_config(dir.path().to_path_buf());
        config.profile = crate::config::profiles::Profile {
            name: "base".to_string(),
            active_categories: vec!["base".to_string()],
            excluded_categories: vec![],
        };
        let ctx = make_linux_context(config);
        ctx.repo_updated.store(true, Ordering::Release);
        let result = ReloadConfig.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Ok));
    }
}
