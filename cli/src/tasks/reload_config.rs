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
    use crate::exec::Executor;
    use crate::platform::{Os, Platform};
    use crate::tasks::test_helpers::{NoOpExecutor, empty_config, make_context};
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn should_run_always() {
        let config = empty_config(PathBuf::from("/tmp"));
        let platform = Arc::new(Platform::new(Os::Linux, false));
        let executor: Arc<dyn Executor> = Arc::new(NoOpExecutor);
        let ctx = make_context(config, platform, executor);
        assert!(ReloadConfig.should_run(&ctx));
    }
}
