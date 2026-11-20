//! Task: install VS Code extensions.
mod install;
mod launcher;

use crate::domains::editors::resources::deferred_extensions::DeferredExtensionsResource;
use crate::engine::{
    Context, ProcessOpts, Requirement, RequirementState, Task, TaskResult, process_operation,
    process_requirement, process_resources, task_metadata,
};
use crate::infra::logging::OutputExt as _;
use crate::infra::provisioning::{DEFERRED_EXTENSIONS_IDS, resuming_extensions};
use anyhow::{Context as _, Result};
use launcher::{find_code_command, managed_insiders_unavailable};

/// Install VS Code extensions.
#[derive(Debug)]
pub struct InstallVsCodeExtensions {
    config: crate::infra::ConfigHandle<Vec<String>>,
    first_login_profile: Option<String>,
}

impl InstallVsCodeExtensions {
    /// Create a task for the selected profile's extension declarations.
    #[must_use]
    pub const fn new(config: crate::infra::ConfigHandle<Vec<String>>) -> Self {
        Self {
            config,
            first_login_profile: None,
        }
    }

    #[must_use]
    pub(crate) fn with_first_login_profile(mut self, profile: Option<String>) -> Self {
        self.first_login_profile = profile;
        self
    }
}

impl Task for InstallVsCodeExtensions {
    task_metadata! {
        name: "VS Code extensions",
        selector: "vscode-extensions",
    }

    fn should_run(&self, ctx: &Context) -> bool {
        resuming_extensions(ctx.env().as_ref()) || !self.config.get().is_empty()
    }

    fn ordering_requirements(&self) -> Vec<Requirement<'_>> {
        vec![launcher_requirement().without_evidence()]
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if crate::infra::provisioning::is_arch_chroot(ctx.env().as_ref()) {
            let profile = self.first_login_profile.as_deref().context(
                "VS Code extensions require a real user session; this profile has no configured dotfiles-first-login service",
            )?;
            if !ctx.dry_run() {
                ensure_first_login_enabled(ctx.home())?;
            }
            ctx.log().warn(
                "VS Code extensions deferred: Arch chroot provisioning has no real user session; verified Marketplace installation is scheduled for first graphical login",
            );
            return process_resources(
                ctx,
                std::iter::once(DeferredExtensionsResource::new(
                    ctx.home(),
                    profile,
                    ctx.overlay(),
                    self.config.get(),
                )?),
                &ProcessOpts::strict("defer").sequential(),
            );
        }
        let resuming = resuming_extensions(ctx.env().as_ref());
        let queued = if resuming {
            ctx.env()
                .var(DEFERRED_EXTENSIONS_IDS)
                .map(|ids| serde_json::from_str::<Vec<String>>(&ids))
                .transpose()
                .context("invalid deferred VS Code extension list")?
        } else {
            None
        };
        let extensions = queued.as_deref().unwrap_or(self.config.get());
        anyhow::ensure!(
            !resuming || !extensions.is_empty(),
            "cannot verify deferred VS Code extensions because their list is empty"
        );
        process_requirement(ctx, &launcher_requirement(), |cmd| {
            ctx.debug_fmt(|| format!("using VS Code CLI: {cmd}"));
            process_operation(
                ctx,
                &install::ExtensionInstall {
                    cmd: &cmd,
                    extensions,
                },
            )
        })
    }
}

fn ensure_first_login_enabled(home: &std::path::Path) -> Result<()> {
    use crate::infra::provisioning::{FIRST_LOGIN_UNIT, FIRST_LOGIN_UNIT_PATH};
    let installed = home.join(FIRST_LOGIN_UNIT_PATH);
    let definition = installed
        .canonicalize()
        .context("install the dotfiles-first-login service before deferring VS Code extensions")?;
    anyhow::ensure!(
        definition.is_file(),
        "the dotfiles-first-login service has no regular unit file"
    );
    let directory = installed.parent().context("missing user unit directory")?;
    for relationship in [
        "graphical-session.target.wants",
        "graphical-session.target.requires",
    ] {
        let link = directory.join(relationship).join(FIRST_LOGIN_UNIT);
        if std::fs::symlink_metadata(&link).is_ok_and(|meta| meta.is_symlink())
            && link.canonicalize().is_ok_and(|path| path == definition)
        {
            return Ok(());
        }
    }
    anyhow::bail!("enable the dotfiles-first-login service before deferring VS Code extensions")
}

fn launcher_requirement() -> Requirement<'static, String> {
    Requirement::new("vscode-launcher", |ctx, _| {
        if crate::infra::provisioning::is_arch_chroot(ctx.env().as_ref()) {
            return Ok(RequirementState::unsatisfied(
                "VS Code extensions require a real user session",
            ));
        }
        Ok(find_code_command(ctx.executor(), |path| {
            managed_insiders_unavailable(path, ctx.root(), ctx.home())
        })?
        .map_or_else(
            || RequirementState::unsatisfied("VS Code CLI not found"),
            RequirementState::Satisfied,
        ))
    })
}

#[cfg(test)]
#[path = "tests/vscode_extensions.rs"]
mod tests;
