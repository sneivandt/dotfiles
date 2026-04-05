//! Task: configure PAM service files.

use std::sync::Arc;

use anyhow::Result;

use crate::phases::{Context, ProcessOpts, Task, TaskPhase, TaskResult, process_resources};
use crate::resources::pam::PamConfigResource;

/// Install PAM configuration files for services that need standard
/// `system-auth` includes (e.g. screen lockers).
#[derive(Debug)]
pub struct ConfigurePam;

impl Task for ConfigurePam {
    fn name(&self) -> &'static str {
        "Configure PAM services"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Apply
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.is_linux()
    }

    fn needs_sudo(&self, ctx: &Context) -> bool {
        if !ctx.platform.is_linux() || ctx.dry_run {
            return false;
        }
        !ctx.config_read().pam.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let items = ctx.config_read().pam.clone();
        if items.is_empty() {
            return Ok(TaskResult::NotApplicable("nothing configured".to_string()));
        }
        let resources = items
            .iter()
            .map(|entry| PamConfigResource::from_entry(entry, Arc::clone(&ctx.executor)));
        process_resources(ctx, resources, &ProcessOpts::lenient("configure"))
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::pam::PamEntry;
    use crate::phases::test_helpers::{empty_config, make_linux_context, make_windows_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_false_on_windows() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_windows_context(config);
        assert!(!ConfigurePam.should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_linux() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(ConfigurePam.should_run(&ctx));
    }

    #[test]
    fn should_run_true_when_pam_entries_present_on_linux() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.pam.push(PamEntry {
            name: "hyprlock".to_string(),
        });
        let ctx = make_linux_context(config);
        assert!(ConfigurePam.should_run(&ctx));
    }

    #[test]
    fn needs_sudo_false_on_windows() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.pam.push(PamEntry {
            name: "hyprlock".to_string(),
        });
        let ctx = make_windows_context(config);
        assert!(!ConfigurePam.needs_sudo(&ctx));
    }

    #[test]
    fn needs_sudo_false_when_no_entries() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(!ConfigurePam.needs_sudo(&ctx));
    }

    #[test]
    fn needs_sudo_true_on_linux_with_entries() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.pam.push(PamEntry {
            name: "hyprlock".to_string(),
        });
        let ctx = make_linux_context(config);
        assert!(ConfigurePam.needs_sudo(&ctx));
    }

    #[test]
    fn needs_sudo_false_in_dry_run() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.pam.push(PamEntry {
            name: "hyprlock".to_string(),
        });
        let ctx = make_linux_context(config).with_dry_run(true);
        assert!(!ConfigurePam.needs_sudo(&ctx));
    }
}
