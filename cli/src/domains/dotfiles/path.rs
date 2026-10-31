//! Task: ensure `~/.local/bin` is on the user's `PATH`.

use crate::domains::dotfiles::resources::path_entry::PathEntryResource;
use crate::engine::{Context, ProcessOpts, Task, TaskResult, process_resources, task_metadata};

/// Ensure `~/.local/bin` is on the user's `PATH`.
#[derive(Debug)]
pub struct ConfigurePath;

impl Task for ConfigurePath {
    task_metadata! {
        name: "Shell PATH",
        selector: "path",
        deps: [crate::domains::dotfiles::wrapper::InstallWrapper],
    }

    fn run(&self, ctx: &Context) -> anyhow::Result<TaskResult> {
        let resource = PathEntryResource::new(
            ctx.home(),
            ctx.platform(),
            ctx.executor_arc(),
            std::sync::Arc::clone(ctx.env()),
        );
        process_resources(
            ctx,
            std::iter::once(resource),
            &ProcessOpts::strict("configure"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Task;
    use crate::infra::env::MapEnv;
    use crate::test_helpers::{make_linux_context, task_batch};

    #[test]
    fn profile_preview_apply_and_repeat_preserve_user_content() {
        let export = "export PATH=\"$HOME/.local/bin:$PATH\"";
        for (case, initial, needs_change) in [
            ("missing profile", None, true),
            (
                "profile without trailing newline",
                Some("# user settings\nexport EDITOR=vim"),
                true,
            ),
            (
                "existing managed export",
                Some("# user settings\r\n \texport PATH=\"$HOME/.local/bin:$PATH\" \t\r\n"),
                false,
            ),
        ] {
            let root = tempfile::tempdir_in(".").unwrap();
            let profile = root.path().join(".profile");
            if let Some(content) = initial {
                std::fs::write(&profile, content).unwrap();
            }
            let ctx = make_linux_context(root.path().to_path_buf(), None)
                .with_home(root.path().to_path_buf())
                .with_env(
                    MapEnv::new()
                        .with("PATH", root.path().join(".local/bin").into_os_string())
                        .into_handle(),
                );

            let preview = ConfigurePath.run(&ctx.with_dry_run(true)).unwrap();
            assert_eq!(
                task_batch(&preview).changed_count(),
                u32::from(needs_change),
                "{case}: preview"
            );
            assert_eq!(
                task_batch(&preview).already_ok_count(),
                u32::from(!needs_change),
                "{case}: preview"
            );
            if let Some(content) = initial {
                assert_eq!(
                    std::fs::read_to_string(&profile).unwrap(),
                    content,
                    "{case}"
                );
            } else {
                assert!(!profile.exists(), "preview must not create {case}");
            }

            let applied = ConfigurePath.run(&ctx).unwrap();
            assert_eq!(
                task_batch(&applied).changed_count(),
                u32::from(needs_change),
                "{case}: apply"
            );
            let expected = if needs_change {
                format!("{}\n# Added by dotfiles\n{export}\n", initial.unwrap_or(""))
            } else {
                initial.unwrap().to_string()
            };
            assert_eq!(
                std::fs::read_to_string(&profile).unwrap(),
                expected,
                "{case}"
            );

            let repeated = ConfigurePath.run(&ctx).unwrap();
            assert_eq!(task_batch(&repeated).changed_count(), 0, "{case}: repeat");
            assert_eq!(
                task_batch(&repeated).already_ok_count(),
                1,
                "{case}: repeat"
            );
            assert_eq!(
                std::fs::read_to_string(&profile).unwrap(),
                expected,
                "{case}"
            );
        }
    }

    #[test]
    fn unreadable_profile_is_not_treated_as_missing_in_preview_or_apply() {
        let root = tempfile::tempdir_in(".").unwrap();
        let profile = root.path().join(".profile");
        let original = b"# user settings\n\xff\n";
        std::fs::write(&profile, original).unwrap();
        let ctx = make_linux_context(root.path().to_path_buf(), None)
            .with_home(root.path().to_path_buf())
            .with_env(MapEnv::new().into_handle());

        for dry_run in [true, false] {
            let error = ConfigurePath
                .run(&ctx.with_dry_run(dry_run))
                .expect_err("unreadable user content must block PATH configuration");
            assert!(
                format!("{error:#}").contains(".profile"),
                "dry_run={dry_run}: {error:#}"
            );
            assert_eq!(std::fs::read(&profile).unwrap(), original);
        }
    }

    #[test]
    fn depends_on_install_wrapper() {
        let deps = ConfigurePath.dependencies();
        assert_eq!(
            deps,
            &[crate::domains::dotfiles::wrapper::InstallWrapper.task_id()],
            "ConfigurePath should depend on InstallWrapper"
        );
    }
}
