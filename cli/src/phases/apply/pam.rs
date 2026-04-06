//! Task: configure PAM service files.

use std::sync::Arc;

use anyhow::Result;

use crate::config::category_matcher::Category;
use crate::phases::{Context, ProcessOpts, Task, TaskPhase, TaskResult, process_resources};
use crate::resources::pam::PamConfigResource;

/// The PAM service name to configure on Arch Linux desktop systems.
const HYPRLOCK_SERVICE: &str = "hyprlock";

/// Install PAM configuration files for services that need standard
/// `system-auth` includes (e.g. screen lockers).
#[derive(Debug)]
pub struct ConfigurePam;

impl ConfigurePam {
    /// Returns `true` when running on Arch Linux with the desktop profile active.
    fn is_arch_desktop(ctx: &Context) -> bool {
        ctx.platform.is_arch_linux()
            && ctx
                .config_read()
                .profile
                .active_categories
                .contains(&Category::Desktop)
    }
}

impl Task for ConfigurePam {
    fn name(&self) -> &'static str {
        "Configure PAM services"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Apply
    }

    fn should_run(&self, ctx: &Context) -> bool {
        Self::is_arch_desktop(ctx)
    }

    fn needs_sudo(&self, ctx: &Context) -> bool {
        if ctx.dry_run {
            return false;
        }
        Self::is_arch_desktop(ctx)
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if !Self::is_arch_desktop(ctx) {
            return Ok(TaskResult::NotApplicable("not arch desktop".to_string()));
        }
        let resources = std::iter::once(PamConfigResource::new(
            HYPRLOCK_SERVICE.to_string(),
            Arc::clone(&ctx.executor),
        ));
        process_resources(ctx, resources, &ProcessOpts::lenient("configure"))
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::category_matcher::Category;
    use crate::config::profiles::Profile;
    use crate::phases::test_helpers::{empty_config, make_linux_context, make_windows_context};
    use crate::platform::{Os, Platform};
    use std::path::PathBuf;

    fn arch_desktop_config() -> crate::config::Config {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.profile = Profile {
            name: "desktop".to_string(),
            active_categories: vec![Category::Base, Category::Desktop, Category::Arch],
            excluded_categories: vec![],
        };
        config
    }

    fn make_arch_desktop_context() -> Context {
        use crate::exec::MockExecutor;
        use crate::phases::test_helpers::make_context;

        let config = arch_desktop_config();
        let platform = Platform::new(Os::Linux, true);
        make_context(config, platform, Arc::new(MockExecutor::new()))
    }

    #[test]
    fn should_run_false_on_windows() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_windows_context(config);
        assert!(!ConfigurePam.should_run(&ctx));
    }

    #[test]
    fn should_run_false_on_linux_non_arch() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(!ConfigurePam.should_run(&ctx));
    }

    #[test]
    fn should_run_false_on_arch_without_desktop() {
        use crate::phases::test_helpers::ContextBuilder;

        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = ContextBuilder::new(config).os(Os::Linux).arch(true).build();
        assert!(!ConfigurePam.should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_arch_desktop() {
        let ctx = make_arch_desktop_context();
        assert!(ConfigurePam.should_run(&ctx));
    }

    #[test]
    fn needs_sudo_false_on_windows() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_windows_context(config);
        assert!(!ConfigurePam.needs_sudo(&ctx));
    }

    #[test]
    fn needs_sudo_false_on_non_arch() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(!ConfigurePam.needs_sudo(&ctx));
    }

    #[test]
    fn needs_sudo_true_on_arch_desktop() {
        let ctx = make_arch_desktop_context();
        assert!(ConfigurePam.needs_sudo(&ctx));
    }

    #[test]
    fn needs_sudo_false_in_dry_run() {
        let ctx = make_arch_desktop_context().with_dry_run(true);
        assert!(!ConfigurePam.needs_sudo(&ctx));
    }
}
