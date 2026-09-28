use super::*;

pub(super) fn global(args: &[&str]) -> crate::app::cli::GlobalOpts {
    use clap::Parser as _;
    let cli = crate::app::cli::Cli::parse_from(
        ["dotfiles", "install"]
            .into_iter()
            .chain(args.iter().copied()),
    );
    let crate::app::cli::Command::Install(opts) = cli.command else {
        panic!("expected install command");
    };
    opts.into_engine_parts(false).0
}

#[cfg(test)]
mod reexec_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn re_exec_path_uses_installed_binary_path() {
        let root = Path::new("/repo");
        let expected = if cfg!(windows) {
            "dotfiles.exe"
        } else {
            "dotfiles"
        };
        assert_eq!(re_exec_path(root), root.join("bin").join(expected));
    }

    #[test]
    fn self_update_re_exec_preserves_arguments_and_sets_both_guards() {
        let args = vec![
            "install".to_string(),
            "--profile".to_string(),
            "desktop".to_string(),
        ];
        let command = build_reexec_command(Path::new("/repo/bin/dotfiles"), &args);

        assert_eq!(command.get_program(), "/repo/bin/dotfiles");
        assert_eq!(
            command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            args
        );
        let env = command
            .get_envs()
            .map(|(key, value)| (key.to_owned(), value.map(std::ffi::OsStr::to_owned)))
            .collect::<std::collections::HashMap<_, _>>();
        for guard in [REEXEC_GUARD_VAR, SELF_UPDATE_REEXEC_GUARD_VAR] {
            assert_eq!(
                env.get(std::ffi::OsStr::new(guard)),
                Some(&Some(std::ffi::OsString::from("1"))),
                "self-update re-exec should set {guard}"
            );
        }
    }

    #[test]
    fn repository_re_exec_sets_shared_and_repository_guards() {
        use crate::infra::env::MapEnv;

        let args = vec![
            "update".to_string(),
            "--only".to_string(),
            "repository".to_string(),
        ];
        let command = build_repository_reexec_command(Path::new("/repo/bin/dotfiles"), &args);
        assert_eq!(command.get_program(), "/repo/bin/dotfiles");
        assert_eq!(
            command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            args
        );
        let env = command
            .get_envs()
            .map(|(key, value)| (key.to_owned(), value.map(std::ffi::OsStr::to_owned)))
            .collect::<std::collections::HashMap<_, _>>();

        assert_eq!(
            env.get(std::ffi::OsStr::new(REEXEC_GUARD_VAR)),
            Some(&Some(std::ffi::OsString::from("1")))
        );
        assert_eq!(
            env.get(std::ffi::OsStr::new(REPOSITORY_REEXEC_GUARD_VAR)),
            Some(&Some(std::ffi::OsString::from("1")))
        );
        assert!(
            !env.contains_key(std::ffi::OsStr::new(SELF_UPDATE_REEXEC_GUARD_VAR)),
            "repository re-exec must not claim that the binary was replaced"
        );

        let child_env = command
            .get_envs()
            .fold(MapEnv::new(), |child, (key, value)| {
                child.with(
                    key.to_str().expect("restart guard names must be Unicode"),
                    value.expect("restart guards must be set"),
                )
            });
        let global = global(&[]);
        let runtime = RuntimePolicy::new(&global, false, child_env.into_handle(), true, true);
        assert!(
            !should_check_self_update(&runtime),
            "repository restart must not retry the initial self-update preflight, regardless of its outcome"
        );
    }

    #[test]
    fn self_update_policy_handles_all_guard_combinations() {
        use crate::infra::env::MapEnv;

        let global = global(&[]);
        for (guards, expected) in [
            (vec![], true),
            (vec![REEXEC_GUARD_VAR], false),
            (vec![REPOSITORY_REEXEC_GUARD_VAR], false),
            (vec![REEXEC_GUARD_VAR, REPOSITORY_REEXEC_GUARD_VAR], false),
            (vec![SELF_UPDATE_REEXEC_GUARD_VAR], false),
            (vec![REEXEC_GUARD_VAR, SELF_UPDATE_REEXEC_GUARD_VAR], false),
            (
                vec![REPOSITORY_REEXEC_GUARD_VAR, SELF_UPDATE_REEXEC_GUARD_VAR],
                false,
            ),
            (
                vec![
                    REEXEC_GUARD_VAR,
                    REPOSITORY_REEXEC_GUARD_VAR,
                    SELF_UPDATE_REEXEC_GUARD_VAR,
                ],
                false,
            ),
        ] {
            // Empty guards still count as present, unlike the elevation marker.
            let env = guards
                .iter()
                .fold(MapEnv::new(), |env, guard| env.with(guard, ""));
            let runtime = RuntimePolicy::new(&global, false, env.clone().into_handle(), true, true);
            assert_eq!(should_check_self_update(&runtime), expected, "{guards:?}");
            let elevated = RuntimePolicy::new(
                &global,
                false,
                env.with(crate::infra::elevation::ELEVATED_CHILD_VAR, "1")
                    .into_handle(),
                true,
                true,
            );
            assert!(!should_check_self_update(&elevated), "elevated: {guards:?}");
        }
    }
}

#[cfg(test)]
mod startup_log_tests {
    use super::runner::{emit_config_summary, emit_startup_context, startup_context_line};
    use crate::infra::logging::{MsgKind, Output};
    use crate::infra::platform::{Os, Platform};
    use std::borrow::Cow;
    use std::path::Path;
    use std::sync::Mutex;

    #[derive(Default)]
    struct CapturingOutput {
        messages: Mutex<Vec<(MsgKind, String)>>,
    }

    impl Output for CapturingOutput {
        fn emit(&self, kind: MsgKind, message: Cow<'_, str>) {
            self.messages
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((kind, message.into_owned()));
        }
    }

    #[test]
    fn repository_restart_does_not_repeat_startup_context() {
        let output = CapturingOutput::default();

        emit_startup_context(&output, "Update · profile desktop · Arch Linux", true);

        assert!(
            output
                .messages
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty(),
            "the restarted child must keep context in the run log without repeating it on the console"
        );
    }

    #[test]
    fn initial_process_emits_startup_context() {
        let output = CapturingOutput::default();
        let context = "Update · profile desktop · Arch Linux";

        emit_startup_context(&output, context, false);

        assert_eq!(
            *output
                .messages
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            vec![(MsgKind::Startup, context.to_string())]
        );
    }

    #[test]
    fn configuration_summary_uses_a_header_and_one_line_per_nonempty_section() {
        let output = CapturingOutput::default();
        let mut config = crate::test_helpers::empty_config("/repo".into());
        config.vscode_extensions = vec!["one".to_string(), "two".to_string()];

        emit_config_summary(&output, &config);

        assert_eq!(
            *output
                .messages
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            vec![
                (MsgKind::Context, "Loaded configuration".to_string()),
                (MsgKind::Context, "  2 vscode extensions".to_string()),
            ]
        );
    }

    #[test]
    fn startup_context_uses_command_profile_platform_and_dry_run() {
        assert_eq!(
            startup_context_line(
                "Install",
                "workstation",
                Platform::new(Os::Linux, false),
                true,
                None,
            ),
            "Install · dry run · profile workstation · Linux"
        );
    }

    #[test]
    fn overlay_is_the_optional_last_startup_section() {
        assert_eq!(
            startup_context_line(
                "Install",
                "workstation",
                Platform::new(Os::Linux, false),
                false,
                Some(Path::new("/private/overlay")),
            ),
            "Install · profile workstation · Linux · overlay /private/overlay",
            "overlay must be appended to the startup header, not emitted on its own line"
        );
    }

    #[test]
    fn dry_run_follows_the_command_and_overlay_stays_last() {
        assert_eq!(
            startup_context_line(
                "Install",
                "workstation",
                Platform::new(Os::Linux, false),
                true,
                Some(Path::new("/private/overlay")),
            ),
            "Install · dry run · profile workstation · Linux · overlay /private/overlay"
        );
    }
}
