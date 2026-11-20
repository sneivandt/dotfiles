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

    #[cfg(unix)]
    #[test]
    fn profile_link_matrix_preserves_unresolved_and_shared_destinations() {
        for preview in [false, true] {
            for case in ["dangling", "hardlink", "directory", "working-symlink"] {
                let fixture = tempfile::tempdir().unwrap();
                let home = fixture.path().join("home");
                std::fs::create_dir(&home).unwrap();
                let profile = home.join(".profile");
                let outside = fixture.path().join("outside");
                match case {
                    "dangling" => std::os::unix::fs::symlink(&outside, &profile).unwrap(),
                    "hardlink" => {
                        std::fs::write(&outside, "# user content\n").unwrap();
                        std::fs::hard_link(&outside, &profile).unwrap();
                    }
                    "directory" => std::fs::create_dir(&profile).unwrap(),
                    "working-symlink" => {
                        std::fs::write(&outside, "# user content\n").unwrap();
                        std::os::unix::fs::symlink(&outside, &profile).unwrap();
                    }
                    _ => panic!("unknown profile fixture"),
                }
                let ctx = make_linux_context(fixture.path().to_path_buf(), None)
                    .with_home(home)
                    .with_dry_run(preview);
                let result = ConfigurePath.run(&ctx);
                if case == "working-symlink" {
                    let _result = result.unwrap();
                    assert!(profile.is_symlink());
                    let content = std::fs::read_to_string(&outside).unwrap();
                    assert!(content.contains("# user content\n"));
                    assert_eq!(content.contains("export PATH="), !preview);
                    if !preview {
                        assert_eq!(
                            task_batch(&ConfigurePath.run(&ctx).unwrap()).changed_count(),
                            0
                        );
                    }
                } else {
                    assert!(result.is_err(), "{case}, preview={preview}");
                    if case == "hardlink" {
                        assert_eq!(
                            std::fs::read_to_string(&outside).unwrap(),
                            "# user content\n"
                        );
                    } else {
                        assert!(!outside.exists(), "{case}: must not create a referent");
                    }
                }
            }
        }
    }

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
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt as _;
                    std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o750))
                        .unwrap();
                }
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
                let block = format!(
                    "# Added by dotfiles\ncase \":$PATH:\" in\n    *\":$HOME/.local/bin:\"*) ;;\n    *) {export} ;;\nesac\n"
                );
                initial.map_or_else(
                    || block.clone(),
                    |content| format!("{block}{content}\n{block}"),
                )
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
            #[cfg(unix)]
            if initial.is_some() {
                use std::os::unix::fs::PermissionsExt as _;
                assert_eq!(
                    std::fs::metadata(&profile).unwrap().permissions().mode() & 0o777,
                    0o750,
                    "{case}"
                );
            }
        }
    }

    #[test]
    fn later_profile_edits_do_not_accumulate_path_blocks() {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join(".profile");
        let ctx = make_linux_context(root.path().to_path_buf(), None)
            .with_home(root.path().to_path_buf());
        std::fs::write(&profile, "export EDITOR=vim\n").unwrap();
        assert_eq!(
            task_batch(&ConfigurePath.run(&ctx).unwrap()).changed_count(),
            1
        );
        let installed = std::fs::read_to_string(&profile).unwrap();
        assert_eq!(installed.matches("# Added by dotfiles\n").count(), 2);
        let annotated = format!("# personal settings\n{installed}  # user comment\r\n\t\n");
        std::fs::write(&profile, &annotated).unwrap();
        for dry_run in [true, false] {
            let result = ConfigurePath.run(&ctx.with_dry_run(dry_run)).unwrap();
            assert_eq!(task_batch(&result).already_ok_count(), 1);
            assert_eq!(std::fs::read_to_string(&profile).unwrap(), annotated);
        }
        let active = format!("{annotated}export PATH=/fixture/bin\n");
        std::fs::write(&profile, &active).unwrap();
        for dry_run in [true, false] {
            let error = ConfigurePath.run(&ctx.with_dry_run(dry_run)).unwrap_err();
            assert!(format!("{error:#}").contains("between the leading and trailing"));
            assert_eq!(std::fs::read_to_string(&profile).unwrap(), active);
        }
        let repaired = annotated.replacen(
            "export EDITOR=vim",
            "export PATH=/fixture/bin\nexport EDITOR=vim",
            1,
        );
        std::fs::write(&profile, &repaired).unwrap();
        assert_eq!(
            task_batch(&ConfigurePath.run(&ctx).unwrap()).already_ok_count(),
            1
        );
        assert_eq!(std::fs::read_to_string(&profile).unwrap(), repaired);
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
