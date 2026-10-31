//! Task: install VS Code extensions.
use crate::domains::editors::resources::deferred_extensions::DeferredExtensionsResource;
use crate::domains::editors::resources::vscode_extension::{
    VsCodeExtensionResource, find_code_command, get_installed_extensions,
};
use crate::engine::{
    Context, ProcessOpts, Requirement, RequirementState, Task, TaskResult, process_requirement,
    process_resources, process_resources_with_cache, task_metadata,
};
use crate::infra::logging::OutputExt as _;
use anyhow::Result;
use std::collections::HashSet;

crate::engine::config_task! {
    /// Install VS Code extensions.
    pub struct InstallVsCodeExtensions(Vec<String>);
}

impl Task for InstallVsCodeExtensions {
    task_metadata! {
        name: "VS Code extensions",
        selector: "vscode-extensions",
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        !self.config.get().is_empty()
    }

    fn ordering_requirements(&self) -> Vec<Requirement<'_>> {
        vec![launcher_requirement().without_evidence()]
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if crate::infra::provisioning::is_arch_chroot(ctx.env().as_ref()) {
            ctx.log().warn(
                "VS Code extensions deferred: Arch chroot provisioning has no real user session; verified Marketplace installation is scheduled for first graphical login",
            );
            return process_resources(
                ctx,
                std::iter::once(DeferredExtensionsResource::new(ctx.home())),
                &ProcessOpts::install_missing("defer").sequential(),
            );
        }
        process_requirement(ctx, &launcher_requirement(), |cmd| {
            ctx.debug_fmt(|| format!("using VS Code CLI: {cmd}"));
            let extensions = self.config.get();
            ctx.trace_fmt(|| {
                format!(
                    "batch-checking {} extensions with a single query",
                    extensions.len()
                )
            });
            let installed = get_installed_extensions(&cmd, ctx.executor())?;

            let mut seen = HashSet::new();
            let resources = extensions
                .iter()
                .filter(|id| seen.insert(id.to_lowercase()))
                .map(|id| {
                    VsCodeExtensionResource::new(id.clone(), cmd.clone(), ctx.executor_arc())
                });
            process_resources_with_cache(
                ctx,
                resources,
                &installed,
                |resource: &VsCodeExtensionResource, installed: &HashSet<String>| {
                    Ok(resource.state_from_installed(installed))
                },
                &ProcessOpts::install_missing("install"),
            )
        })
    }
}

fn launcher_requirement() -> Requirement<'static, String> {
    Requirement::new("vscode-launcher", |ctx, _| {
        if crate::infra::provisioning::is_arch_chroot(ctx.env().as_ref()) {
            return Ok(RequirementState::unsatisfied(
                "VS Code extensions require a real user session",
            ));
        }
        Ok(find_code_command(ctx.executor())?.map_or_else(
            || RequirementState::unsatisfied("VS Code CLI not found"),
            RequirementState::Satisfied,
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::ConfigHandle;
    use crate::infra::env::MapEnv;
    use crate::infra::exec::{ExecResult, MockExecutor};
    use crate::infra::platform::{Os, Platform};
    use crate::test_helpers::{make_context, make_linux_context, task_batch};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    fn ext() -> String {
        "github.copilot".to_string()
    }

    fn expect_code(
        mock: &mut MockExecutor,
        sequence: &mut mockall::Sequence,
        launcher: &Path,
        args: &[&str],
        output: &'static str,
    ) {
        let launcher = launcher.to_path_buf();
        let args: Vec<_> = args.iter().map(|arg| (*arg).to_string()).collect();
        mock.expect_execute()
            .once()
            .in_sequence(sequence)
            .withf(move |spec| {
                #[cfg(windows)]
                let command_matches = spec.windows_command_line()
                    == Some(format!("\"\"{}\" {}\"", launcher.display(), args.join(" ")).as_str());
                #[cfg(not(windows))]
                let command_matches = spec.program() == launcher.as_os_str()
                    && spec
                        .arguments()
                        .iter()
                        .map(|arg| arg.to_string_lossy())
                        .eq(args.iter().map(String::as_str));
                command_matches && !spec.is_checked() && spec.working_dir().is_none()
            })
            .returning(move |_| Ok(ExecResult::success(output)));
    }

    #[test]
    fn bulk_inventory_deduplicates_case_variants_and_converges() {
        for dry_run in [false, true] {
            let root = tempfile::tempdir_in(".").unwrap();
            let launcher = root.path().join("Code Insiders").join(if cfg!(windows) {
                "code-insiders.cmd"
            } else {
                "code-insiders"
            });
            let mut mock = MockExecutor::new();
            let command = if cfg!(windows) {
                "code-insiders.cmd"
            } else {
                "code-insiders"
            };
            let expected_launcher = launcher.clone();
            mock.expect_which()
                .times(if dry_run { 1 } else { 2 })
                .withf(move |program| program == command)
                .return_const(true);
            mock.expect_which_path()
                .times(if dry_run { 1 } else { 2 })
                .withf(move |program| program == command)
                .returning(move |_| Ok(expected_launcher.clone()));
            let mut sequence = mockall::Sequence::new();
            expect_code(
                &mut mock,
                &mut sequence,
                &launcher,
                &["--list-extensions"],
                "GITHUB.COPILOT\n",
            );
            if !dry_run {
                expect_code(
                    &mut mock,
                    &mut sequence,
                    &launcher,
                    &["--install-extension", "ms-python.python", "--force"],
                    "installed",
                );
                expect_code(
                    &mut mock,
                    &mut sequence,
                    &launcher,
                    &["--list-extensions"],
                    "github.copilot\nMS-PYTHON.PYTHON\n",
                );
            }
            let ctx = make_context(
                root.path().to_path_buf(),
                None,
                Platform::new(Os::Linux, false),
                Arc::new(mock),
            )
            .with_dry_run(dry_run)
            .with_parallel(true);
            let task = InstallVsCodeExtensions::new(ConfigHandle::new(vec![
                "GitHub.Copilot".to_string(),
                "github.copilot".to_string(),
                "ms-python.python".to_string(),
                "MS-PYTHON.PYTHON".to_string(),
            ]));

            let first = task.run(&ctx).unwrap();
            assert_eq!(task_batch(&first).changed_count(), 1, "dry_run={dry_run}");
            assert_eq!(
                task_batch(&first).already_ok_count(),
                1,
                "dry_run={dry_run}"
            );
            if !dry_run {
                let repeated = task.run(&ctx).unwrap();
                assert_eq!(task_batch(&repeated).changed_count(), 0);
                assert_eq!(task_batch(&repeated).already_ok_count(), 2);
            }
        }
    }

    #[test]
    fn should_run_false_when_no_extensions_configured() {
        let ctx = make_linux_context(PathBuf::from("/tmp"), None);
        assert!(!InstallVsCodeExtensions::new(ConfigHandle::new(vec![])).should_run(&ctx));
    }

    #[test]
    fn should_run_true_when_extensions_configured() {
        let ctx = make_linux_context(PathBuf::from("/tmp"), None);
        let task = InstallVsCodeExtensions::new(ConfigHandle::new(vec![ext()]));
        assert!(task.should_run(&ctx));
    }

    #[test]
    fn run_skips_when_vscode_cli_not_found() {
        // Default make_linux_context uses TestExecutor with which_result=false,
        // so find_code_command returns None for both "code-insiders" and "code".
        let ctx = make_linux_context(PathBuf::from("/tmp"), None);
        let task = InstallVsCodeExtensions::new(ConfigHandle::new(vec![ext()]));
        let result = task.run(&ctx).unwrap();
        assert!(
            matches!(
                result,
                TaskResult::Skipped { ref reason, .. }
                    if reason.contains("VS Code CLI not found")
            ),
            "expected 'VS Code CLI not found' skip, got {result:?}"
        );
    }

    #[test]
    fn arch_chroot_provisioning_creates_then_preserves_the_first_session_marker() {
        let home = tempfile::tempdir().unwrap();
        let env = MapEnv::new()
            .with(
                crate::infra::provisioning::ENV_VAR,
                crate::infra::provisioning::ARCH_CHROOT,
            )
            .into_handle();
        let ctx = make_context(
            home.path().to_path_buf(),
            None,
            Platform::new(Os::Linux, false),
            Arc::new(MockExecutor::new()),
        )
        .with_home(home.path().to_path_buf())
        .with_env(env);
        let task = InstallVsCodeExtensions::new(ConfigHandle::new(vec![ext()]));
        let first = task.run(&ctx).unwrap();
        assert!(
            matches!(first, TaskResult::Batch(ref stats) if stats.changed_count() == 1),
            "first provisioning run should create the marker: {first:?}"
        );
        let marker = home
            .path()
            .join(crate::domains::editors::resources::deferred_extensions::MARKER_RELATIVE_PATH);
        assert!(
            std::fs::read_to_string(&marker)
                .unwrap()
                .contains("dotfiles-first-login"),
            "the durable marker must identify its consumer"
        );
        let preserved_content = "pending extensions; preserve first-session notes\n";
        std::fs::write(&marker, preserved_content).unwrap();

        let result = task.run(&ctx).unwrap();

        assert!(
            matches!(
                result,
                TaskResult::Batch(ref stats)
                    if stats.already_ok_count() == 1 && stats.changed_count() == 0
            ),
            "rerunning provisioning should preserve the existing deferral: {result:?}"
        );
        assert_eq!(std::fs::read_to_string(marker).unwrap(), preserved_content);
    }

    #[test]
    fn arch_chroot_preview_neither_probes_cli_nor_creates_a_marker() {
        let home = tempfile::tempdir_in(".").unwrap();
        let ctx = make_context(
            home.path().to_path_buf(),
            None,
            Platform::new(Os::Linux, false),
            Arc::new(MockExecutor::new()),
        )
        .with_home(home.path().to_path_buf())
        .with_dry_run(true)
        .with_env(
            MapEnv::new()
                .with(
                    crate::infra::provisioning::ENV_VAR,
                    crate::infra::provisioning::ARCH_CHROOT,
                )
                .into_handle(),
        );
        let task = InstallVsCodeExtensions::new(ConfigHandle::new(vec![ext()]));

        assert_eq!(task_batch(&task.run(&ctx).unwrap()).changed_count(), 1);
        assert!(
            !home.path().join(".local").exists(),
            "preview must not create marker parents"
        );
    }
    #[test]
    fn early_extension_readiness_requires_a_launcher_and_real_session() {
        for (chroot, available) in [(false, false), (false, true), (true, true)] {
            let root = tempfile::tempdir().unwrap();
            let mut mock = MockExecutor::new();
            mock.expect_which()
                .times(if chroot {
                    0
                } else if available {
                    1
                } else {
                    2
                })
                .return_const(available);
            mock.expect_which_path()
                .times(usize::from(!chroot && available))
                .returning(|program| Ok(PathBuf::from(program)));
            let env = MapEnv::new().with(
                crate::infra::provisioning::ENV_VAR,
                if chroot {
                    crate::infra::provisioning::ARCH_CHROOT
                } else {
                    ""
                },
            );
            let ctx = make_context(
                root.path().into(),
                None,
                Platform::new(Os::Linux, false),
                Arc::new(mock),
            )
            .with_env(env.into_handle());
            let task = InstallVsCodeExtensions::new(ConfigHandle::new(vec![ext()]));
            assert_eq!(
                crate::engine::requirement::ordering_requirements_satisfied(
                    &ctx,
                    &task.ordering_requirements(),
                ),
                !chroot && available,
                "chroot={chroot}, available={available}"
            );
        }
    }
}
