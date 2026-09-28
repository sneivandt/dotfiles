//! Task: install VS Code extensions.
use crate::domains::editors::resources::deferred_extensions::DeferredExtensionsResource;
use crate::domains::editors::resources::vscode_extension::{
    VsCodeExtensionResource, find_code_command, get_installed_extensions,
};
use crate::engine::{
    Context, ProcessOpts, Task, TaskResult, process_resources, process_resources_with_cache,
    task_metadata,
};
use crate::infra::ConfigHandle;
use crate::infra::logging::OutputExt as _;
use anyhow::Result;
use std::collections::HashSet;

/// Install VS Code extensions.
#[derive(Debug)]
pub struct InstallVsCodeExtensions {
    config: ConfigHandle<Vec<String>>,
}

impl InstallVsCodeExtensions {
    /// Create the task with a handle to the extension configuration.
    #[must_use]
    pub const fn new(config: ConfigHandle<Vec<String>>) -> Self {
        Self { config }
    }
}

impl Task for InstallVsCodeExtensions {
    task_metadata! {
        name: "VS Code extensions",
        selector: "vscode-extensions",
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        !self.config.read().is_empty()
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
        let Some(cmd) = find_code_command(ctx.executor()) else {
            ctx.log().debug("no VS Code CLI launcher found in PATH");
            return Ok(TaskResult::unmet("VS Code CLI not found"));
        };

        ctx.debug_fmt(|| format!("using VS Code CLI: {cmd}"));
        let extensions: Vec<_> = self.config.read().to_vec();
        ctx.trace_fmt(|| {
            format!(
                "batch-checking {} extensions with a single query",
                extensions.len()
            )
        });
        let installed = get_installed_extensions(&cmd, ctx.executor())?;

        let resources = extensions
            .iter()
            .map(|id| VsCodeExtensionResource::new(id.clone(), cmd.clone(), ctx.executor_arc()));
        process_resources_with_cache(
            ctx,
            resources,
            &installed,
            |resource: &VsCodeExtensionResource, installed: &HashSet<String>| {
                Ok(resource.state_from_installed(installed))
            },
            &ProcessOpts::install_missing("install"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::ConfigHandle;
    use crate::infra::env::MapEnv;
    use crate::infra::exec::{ExecResult, MockExecutor};
    use crate::infra::platform::{Os, Platform};
    use crate::test_helpers::{empty_config, make_context, make_linux_context, task_batch};
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
                    == Some(
                        format!(
                            "\"\"{}\" {}\"",
                            launcher.display(),
                            args.iter()
                                .map(|arg| format!("\"{arg}\""))
                                .collect::<Vec<_>>()
                                .join(" ")
                        )
                        .as_str(),
                    );
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
    fn bulk_inventory_installs_only_missing_extensions_and_converges() {
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
                empty_config(root.path().to_path_buf()),
                Platform::new(Os::Linux, false),
                Arc::new(mock),
            )
            .with_dry_run(dry_run);
            let task = InstallVsCodeExtensions::new(ConfigHandle::new(vec![
                "GitHub.Copilot".to_string(),
                "ms-python.python".to_string(),
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
        let ctx = make_linux_context(empty_config(PathBuf::from("/tmp")));
        assert!(!InstallVsCodeExtensions::new(ConfigHandle::new(vec![])).should_run(&ctx));
    }

    #[test]
    fn should_run_true_when_extensions_configured() {
        let ctx = make_linux_context(empty_config(PathBuf::from("/tmp")));
        let task = InstallVsCodeExtensions::new(ConfigHandle::new(vec![ext()]));
        assert!(task.should_run(&ctx));
    }

    #[test]
    fn run_skips_when_vscode_cli_not_found() {
        // Default make_linux_context uses TestExecutor with which_result=false,
        // so find_code_command returns None for both "code-insiders" and "code".
        let ctx = make_linux_context(empty_config(PathBuf::from("/tmp")));
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
    fn arch_chroot_provisioning_creates_a_first_session_marker() {
        let home = tempfile::tempdir().unwrap();
        let ctx = make_linux_context(empty_config(PathBuf::from("/tmp")))
            .with_home(home.path().to_path_buf())
            .with_env(
                MapEnv::new()
                    .with(
                        crate::infra::provisioning::ENV_VAR,
                        crate::infra::provisioning::ARCH_CHROOT,
                    )
                    .into_handle(),
            );
        let task = InstallVsCodeExtensions::new(ConfigHandle::new(vec![ext()]));

        let result = task.run(&ctx).unwrap();

        assert!(
            matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() == 1),
            "first provisioning run should report the durable deferral: {result:?}"
        );
        assert!(
            home.path()
                .join(
                    crate::domains::editors::resources::deferred_extensions::MARKER_RELATIVE_PATH,
                )
                .is_file(),
            "the first-session service marker should be created"
        );
    }

    #[test]
    fn arch_chroot_provisioning_reuses_an_existing_marker() {
        let home = tempfile::tempdir().unwrap();
        let env = MapEnv::new()
            .with(
                crate::infra::provisioning::ENV_VAR,
                crate::infra::provisioning::ARCH_CHROOT,
            )
            .into_handle();
        let ctx = make_linux_context(empty_config(PathBuf::from("/tmp")))
            .with_home(home.path().to_path_buf())
            .with_env(env);
        let task = InstallVsCodeExtensions::new(ConfigHandle::new(vec![ext()]));
        let first = task.run(&ctx).unwrap();
        assert!(
            matches!(first, TaskResult::Batch(ref stats) if stats.changed_count() == 1),
            "first provisioning run should create the marker: {first:?}"
        );

        let result = task.run(&ctx).unwrap();

        assert!(
            matches!(result, TaskResult::Batch(ref stats) if stats.already_ok_count() == 1),
            "rerunning provisioning should preserve the existing deferral: {result:?}"
        );
    }

    #[test]
    fn arch_chroot_preview_neither_probes_cli_nor_creates_a_marker() {
        let home = tempfile::tempdir_in(".").unwrap();
        let ctx = make_context(
            empty_config(home.path().to_path_buf()),
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
}
