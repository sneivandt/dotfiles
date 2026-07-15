//! Task: configure file permissions.

use crate::domains::files::config::chmod::ChmodEntry;
use crate::domains::files::resources::chmod::ChmodResource;
use crate::engine::{Domain, PlatformCapability, ProcessOpts, TaskPhase, config_resource_task};

config_resource_task! {
    /// Configure file permissions from chmod.toml.
    pub ApplyFilePermissions {
        name: "Configure file permissions",
        phase: TaskPhase::Provision,
        domain: Domain::Files,
        config: Vec<ChmodEntry>,
        policy: [PlatformCapability::Chmod.policy()],
        deps: [crate::domains::files::tasks::symlinks::InstallSymlinks],
        guard: |_cfg, ctx| ctx.system().platform().supports_chmod(),
        items: |cfg| cfg.clone(),
        build: |entry, ctx| ChmodResource::from_entry(&entry, ctx.paths().home()),
        opts: ProcessOpts::fix_existing("configure"),
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
    use crate::domains::files::config::chmod::ChmodEntry;
    use crate::engine::Task;
    use crate::runtime::ConfigHandle;
    use crate::test_helpers::{empty_config, make_linux_context, make_windows_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_false_on_windows() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_windows_context(config);
        assert!(!ApplyFilePermissions::new(ConfigHandle::new(vec![])).should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_linux_when_guard_passes() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(ApplyFilePermissions::new(ConfigHandle::new(vec![])).should_run(&ctx));
    }

    #[test]
    fn should_run_true_when_chmod_entries_present_on_linux() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        let task = ApplyFilePermissions::new(ConfigHandle::new(vec![ChmodEntry::new(
            "600",
            "ssh/config",
        )]));
        assert!(task.should_run(&ctx));
    }
}
