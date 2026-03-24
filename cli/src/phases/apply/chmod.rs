//! Task: apply file permissions.

use crate::phases::{ProcessOpts, TaskPhase, resource_task};
use crate::resources::chmod::ChmodResource;

resource_task! {
    /// Apply file permissions from chmod.toml.
    pub ApplyFilePermissions {
        name: "Apply file permissions",
        phase: TaskPhase::Apply,
        deps: [crate::phases::apply::symlinks::InstallSymlinks],
        guard: |ctx| ctx.platform.supports_chmod(),
        items: |ctx| ctx.config_read().chmod.clone(),
        build: |entry, ctx| build_resource(&entry, &ctx.home),
        opts: ProcessOpts::fix_existing("apply permissions"),
    }
}

/// Build a [`ChmodResource`] from a config entry.
///
/// Mode validity is verified by config validation before tasks run, so a
/// parse failure here indicates a bug in the validation pipeline.
#[allow(clippy::expect_used)]
fn build_resource(
    entry: &crate::config::chmod::ChmodEntry,
    home: &std::path::Path,
) -> ChmodResource {
    ChmodResource::from_entry(entry, home)
        .expect("invalid octal mode should have been caught by config validation")
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::chmod::ChmodEntry;
    use crate::phases::Task;
    use crate::phases::test_helpers::{empty_config, make_linux_context, make_windows_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_false_on_windows() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_windows_context(config);
        assert!(!ApplyFilePermissions.should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_linux_when_guard_passes() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(ApplyFilePermissions.should_run(&ctx));
    }

    #[test]
    fn should_run_true_when_chmod_entries_present_on_linux() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.chmod.push(ChmodEntry {
            mode: "600".to_string(),
            path: "ssh/config".to_string(),
        });
        let ctx = make_linux_context(config);
        assert!(ApplyFilePermissions.should_run(&ctx));
    }
}
