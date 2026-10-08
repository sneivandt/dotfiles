use super::*;

pub(super) fn global(args: &[&str]) -> crate::app::cli::GlobalOpts {
    let cli = crate::app::cli::Cli::parse_from(
        ["dotfiles", "update"]
            .into_iter()
            .chain(args.iter().copied()),
    );
    let crate::app::cli::Command::Update(opts) = cli.command else {
        panic!("expected install command");
    };
    opts.into_engine_parts().0
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
    fn re_exec_preserves_arguments_and_sets_only_its_own_restart_guards() {
        use crate::infra::env::MapEnv;

        for (guard, args) in [
            (
                SELF_UPDATE_REEXEC_GUARD_VAR,
                ["update", "--profile", "desktop"],
            ),
            (
                REPOSITORY_REEXEC_GUARD_VAR,
                ["update", "--only", "repository"],
            ),
        ] {
            let args = args.map(str::to_string);
            let command = build_reexec_command(Path::new("/repo/bin/dotfiles"), &args, guard);
            assert_eq!(command.get_program(), "/repo/bin/dotfiles");
            assert_eq!(
                command
                    .get_args()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect::<Vec<_>>(),
                args,
                "{guard}"
            );
            let env = command
                .get_envs()
                .map(|(key, value)| (key.to_str().unwrap(), value.unwrap().to_str().unwrap()))
                .collect::<std::collections::HashMap<_, _>>();
            assert_eq!(
                env,
                std::collections::HashMap::from([(REEXEC_GUARD_VAR, "1"), (guard, "1")]),
                "{guard}: repository restart must not claim that the binary was replaced"
            );

            let child_env = env
                .into_iter()
                .fold(MapEnv::new(), |child, (key, value)| child.with(key, value));
            let global = global(&[]);
            let runtime = RuntimePolicy::new(&global, false, child_env.into_handle(), true, true);
            assert!(
                !should_check_self_update(&runtime),
                "{guard}: restart must not retry the initial self-update preflight"
            );
        }
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
    use crate::infra::logging::MsgKind;
    use crate::infra::platform::{Os, Platform};
    use std::path::Path;

    use crate::test_helpers::CapturingOutput;

    #[test]
    fn repository_restart_does_not_repeat_startup_context() {
        let output = CapturingOutput::default();

        emit_startup_context(&output, "Update · profile desktop · Arch Linux", true);

        assert!(
            output.messages().is_empty(),
            "the restarted child must keep context in the run log without repeating it on the console"
        );
    }

    #[test]
    fn initial_process_emits_startup_context() {
        let output = CapturingOutput::default();
        let context = "Update · profile desktop · Arch Linux";

        emit_startup_context(&output, context, false);

        assert_eq!(
            output.messages(),
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
            output.messages(),
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
