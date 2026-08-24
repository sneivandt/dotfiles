//! Lifecycle for conditionally available Copilot targets.

use anyhow::Result;

use super::autopilot::{
    DesiredApmWorkflows, apply_workflow_autopilot_fixup, snapshot_desired_apm_workflow_ids,
};
use super::commands::{
    ApmCommand, ApmCommandResult, ensure_experimental_target_enabled, run_apm_invocation,
};
use super::cowork::{reconcile_cowork_skills, remove_legacy_cowork_lock_deployments};
use super::targets::{ApmTargets, CopilotDeployment, CopilotTarget};
use crate::engine::Context;
use crate::infra::logging::OutputExt as _;

/// User-facing context for previewing target-specific work.
#[derive(Debug, Clone, Copy)]
pub(super) enum ManagedTargetPreview {
    Install,
    Update,
}

/// Lifecycle owner for conditionally available Copilot targets.
#[derive(Debug, Clone, Copy)]
pub(super) struct ManagedTargets {
    active: ApmTargets,
}

impl ManagedTargets {
    /// Detect the managed Copilot targets available on this machine.
    ///
    /// # Errors
    ///
    /// Returns an error when a target availability probe fails.
    pub(super) fn detect(ctx: &Context) -> Result<Self> {
        Ok(Self {
            active: ApmTargets::detect(ctx)?,
        })
    }

    #[must_use]
    const fn includes(self, target: CopilotTarget) -> bool {
        self.active.includes(target)
    }

    /// Snapshot Copilot App state that must survive the APM command.
    #[must_use]
    pub(super) fn snapshot(self, ctx: &Context) -> ManagedTargetSnapshot {
        ManagedTargetSnapshot {
            copilot_app: self
                .includes(CopilotTarget::CopilotApp)
                .then(|| snapshot_desired_apm_workflow_ids(ctx)),
        }
    }

    /// Preview every target-specific convergence action.
    pub(super) fn preview(self, ctx: &Context, preview: ManagedTargetPreview) -> u32 {
        let mut planned = 0_u32;
        for target in self.active.active() {
            let action = match preview {
                ManagedTargetPreview::Install => "sync",
                ManagedTargetPreview::Update => "redeploy updated",
            };
            match target.deployment() {
                CopilotDeployment::NativeApm { args, .. } => ctx.log().dry_run(format!(
                    "run apm {} to {action} {}",
                    args.join(" "),
                    target.display_name()
                )),
                CopilotDeployment::CoworkFileReconcile { .. } => ctx.log().dry_run(format!(
                    "{action} {} skills file-by-file without replacing Cowork-owned directories",
                    target.display_name()
                )),
            }
            if target == CopilotTarget::CopilotApp {
                ctx.log().dry_run(
                    "re-assert dotfiles-managed Copilot App workflows to autopilot + enabled in \
                     ~/.copilot/data.db",
                );
            }
            planned = planned.saturating_add(1);
        }
        planned
    }

    /// Run the primary APM command and each conditionally available target.
    ///
    /// # Errors
    ///
    /// Returns an error when APM or Cowork reconciliation fails.
    pub(super) fn run_apm_command(
        self,
        ctx: &Context,
        command: ApmCommand,
    ) -> Result<ApmCommandResult> {
        remove_legacy_cowork_lock_deployments(ctx.home())?;
        match run_apm_invocation(ctx, command, command.args())? {
            ApmCommandResult::Success => {}
            result @ ApmCommandResult::AuthSkipped(_) => return Ok(result),
        }

        for target in self.active.active() {
            match target.deployment() {
                CopilotDeployment::NativeApm { config_key, args } => {
                    ensure_experimental_target_enabled(ctx, target.apm_name(), config_key);
                    let result = run_apm_invocation(ctx, ApmCommand::Install, args)?;
                    if !matches!(result, ApmCommandResult::Success) {
                        return Ok(result);
                    }
                }
                CopilotDeployment::CoworkFileReconcile { config_key } => {
                    ensure_experimental_target_enabled(ctx, target.apm_name(), config_key);
                    reconcile_cowork_skills(ctx)?;
                }
            }
        }
        Ok(ApmCommandResult::Success)
    }

    /// Re-apply the retained Copilot App autopilot policy.
    #[must_use]
    pub(super) fn finish(self, ctx: &Context, snapshot: &ManagedTargetSnapshot) -> bool {
        self.includes(CopilotTarget::CopilotApp)
            && snapshot
                .copilot_app
                .as_ref()
                .is_some_and(|pre| apply_workflow_autopilot_fixup(ctx, pre))
    }
}

/// Target state captured before APM mutates deployments.
#[derive(Debug)]
pub(super) struct ManagedTargetSnapshot {
    copilot_app: Option<DesiredApmWorkflows>,
}
