//! Task: configure file permissions.

use anyhow::Result;

use crate::domains::files::config::chmod::ChmodEntry;
use crate::domains::files::resources::chmod::ChmodResource;
use crate::engine::{Context, ProcessOpts, Task, TaskResult, run_resource_task, task_metadata};
use crate::infra::ConfigHandle;

/// Configure file permissions from chmod.toml.
#[derive(Debug)]
pub struct ApplyFilePermissions {
    config: ConfigHandle<Vec<ChmodEntry>>,
}

const NAME: &str = "File permissions";

impl ApplyFilePermissions {
    /// Create the task with a handle to its configuration slice.
    #[must_use]
    pub const fn new(config: ConfigHandle<Vec<ChmodEntry>>) -> Self {
        Self { config }
    }
}

impl Task for ApplyFilePermissions {
    task_metadata! {
        name: NAME,
        selector: "file-permissions",
        deps: [crate::domains::files::symlinks::InstallSymlinks],
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform().supports_chmod()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let resources: Vec<_> = self
            .config
            .get()
            .iter()
            .map(|entry| ChmodResource::from_entry(entry, ctx.home()))
            .collect();
        let targets: Vec<_> = resources
            .iter()
            .map(|resource| resource.target.clone())
            .collect();
        let resources = resources
            .into_iter()
            .map(|resource| resource.excluding(&targets))
            .collect();
        run_resource_task(ctx, resources, &ProcessOpts::fix_existing("configure"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::files::config::chmod::ChmodEntry;
    use crate::engine::Task;
    use crate::infra::ConfigHandle;
    use crate::test_helpers::{empty_config, make_linux_context, make_windows_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_false_on_windows() {
        let ctx = make_windows_context(PathBuf::from("/tmp"), None);
        assert!(!ApplyFilePermissions::new(ConfigHandle::new(vec![])).should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_linux_when_guard_passes() {
        let ctx = make_linux_context(PathBuf::from("/tmp"), None);
        assert!(ApplyFilePermissions::new(ConfigHandle::new(vec![])).should_run(&ctx));
    }

    #[test]
    fn should_run_true_when_chmod_entries_present_on_linux() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config.root.clone(), config.overlay);
        let task = ApplyFilePermissions::new(ConfigHandle::new(vec![ChmodEntry::new(
            "600",
            "ssh/config",
        )]));
        assert!(task.should_run(&ctx));
    }

    #[cfg(unix)]
    #[test]
    fn explicit_child_permissions_override_recursive_modes_and_converge() {
        use std::os::unix::fs::PermissionsExt as _;

        for parallel in [false, true] {
            for child_first in [true, false] {
                let fixture = tempfile::tempdir_in(".").unwrap();
                let home = fixture.path().join("home");
                let directory = home.join(".tools");
                let executable = directory.join("run");
                let ordinary = directory.join("data");
                let private = directory.join("private");
                let private_data = private.join("data");
                std::fs::create_dir_all(&private).unwrap();
                std::fs::write(&executable, "fixture").unwrap();
                std::fs::write(&ordinary, "data").unwrap();
                std::fs::write(&private_data, "private data").unwrap();
                std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
                    .unwrap();
                std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o755)).unwrap();
                std::fs::set_permissions(&private_data, std::fs::Permissions::from_mode(0o644))
                    .unwrap();
                for path in [&executable, &ordinary] {
                    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
                }
                let mut entries = vec![
                    ChmodEntry::new("755", "tools/run"),
                    ChmodEntry::new("700", "tools/private"),
                    ChmodEntry::new("755", "tools"),
                ];
                if !child_first {
                    entries.reverse();
                }
                let task = ApplyFilePermissions::new(ConfigHandle::new(entries));
                let ctx = make_linux_context(fixture.path().to_path_buf(), None)
                    .with_home(home)
                    .with_parallel(parallel);
                let preview = task.run(&ctx.clone().with_dry_run(true)).unwrap();
                assert_eq!(crate::test_helpers::task_batch(&preview).changed_count(), 3);
                assert_eq!(
                    executable.metadata().unwrap().permissions().mode() & 0o777,
                    0o600
                );
                assert_eq!(
                    directory.metadata().unwrap().permissions().mode() & 0o777,
                    0o700
                );
                assert_eq!(
                    private.metadata().unwrap().permissions().mode() & 0o777,
                    0o755
                );

                crate::test_helpers::assert_task_changed(&task.run(&ctx).unwrap());
                assert_eq!(
                    executable.metadata().unwrap().permissions().mode() & 0o777,
                    0o755,
                    "explicit child mode must win; parallel={parallel}, child_first={child_first}"
                );
                assert_eq!(
                    ordinary.metadata().unwrap().permissions().mode() & 0o777,
                    0o644
                );
                assert_eq!(
                    private.metadata().unwrap().permissions().mode() & 0o777,
                    0o700
                );
                assert_eq!(
                    private_data.metadata().unwrap().permissions().mode() & 0o777,
                    0o600
                );
                let repeated = task.run(&ctx).unwrap();
                let stats = crate::test_helpers::task_batch(&repeated);
                assert_eq!(
                    stats.changed_count(),
                    0,
                    "repeated run must not flip child modes"
                );
                assert_eq!(stats.already_ok_count(), 3);
            }
        }
    }
}
