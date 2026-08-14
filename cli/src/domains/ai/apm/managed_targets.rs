//! Convergence lifecycle for Copilot targets that need work beyond the primary
//! manifest-resolved APM command.

use anyhow::Result;

use super::autopilot::{
    DesiredApmWorkflows, apply_workflow_autopilot_fixup, detect_workflow_autopilot_drift,
    snapshot_desired_apm_workflow_ids,
};
use super::commands::{
    ApmCommand, ApmCommandResult, ensure_experimental_target_enabled, run_apm_invocation,
};
use super::cowork::{
    cowork_skills_are_current, reconcile_cowork_skills, remove_legacy_cowork_lock_deployments,
};
use super::targets::{ApmTargets, CopilotDeployment, CopilotTarget};
use crate::engine::Context;
use crate::infra::logging::OutputExt as _;

/// User-facing context for previewing post-command managed-target work.
#[derive(Debug, Clone, Copy)]
pub(super) enum ManagedTargetPreview {
    Install,
    Update,
}

/// Lifecycle owner for managed Copilot targets.
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
    pub(super) const fn active(self) -> ApmTargets {
        self.active
    }

    #[must_use]
    const fn includes(self, target: CopilotTarget) -> bool {
        self.active.includes(target)
    }

    /// Detect target-specific drift that can be repaired without rerunning APM.
    ///
    /// # Errors
    ///
    /// Returns an error when Cowork's managed skill state cannot be inspected.
    pub(super) fn detect_drift(self, ctx: &Context) -> Result<ManagedTargetDrift> {
        let copilot_app = self
            .includes(CopilotTarget::CopilotApp)
            .then(|| detect_workflow_autopilot_drift(ctx))
            .flatten();
        let cowork = self.includes(CopilotTarget::Cowork) && !cowork_skills_are_current(ctx)?;
        Ok(ManagedTargetDrift {
            copilot_app,
            cowork,
        })
    }

    /// Snapshot target state that must survive the primary APM command.
    #[must_use]
    pub(super) fn snapshot(self, ctx: &Context) -> ManagedTargetSnapshot {
        ManagedTargetSnapshot {
            copilot_app: self
                .includes(CopilotTarget::CopilotApp)
                .then(|| snapshot_desired_apm_workflow_ids(ctx)),
        }
    }

    /// Preview every target-specific convergence action after the primary APM
    /// command and return the number of planned actions.
    pub(super) fn preview(self, ctx: &Context, preview: ManagedTargetPreview) -> u32 {
        let mut planned = 0_u32;
        for target in self.active.active() {
            match (target.deployment(), preview) {
                (
                    CopilotDeployment::ExperimentalInstall { args, .. },
                    ManagedTargetPreview::Install,
                ) => ctx.log().dry_run(format!(
                    "run apm {} to sync {} workflows separately, then re-assert them to autopilot \
                     + enabled in ~/.copilot/data.db",
                    args.join(" "),
                    target.display_name()
                )),
                (
                    CopilotDeployment::ExperimentalInstall { args, .. },
                    ManagedTargetPreview::Update,
                ) => ctx.log().dry_run(format!(
                    "run apm {} to redeploy updated {} workflows separately, then re-assert them \
                     to autopilot + enabled in ~/.copilot/data.db",
                    args.join(" "),
                    target.display_name()
                )),
                (
                    CopilotDeployment::CoworkReconcile,
                    ManagedTargetPreview::Install,
                ) => ctx.log().dry_run(
                    "reconcile Microsoft 365 Copilot Cowork skills from the shared APM deployment \
                     without replacing Cowork-owned directories",
                ),
                (
                    CopilotDeployment::CoworkReconcile,
                    ManagedTargetPreview::Update,
                ) => ctx.log().dry_run(
                    "reconcile updated Microsoft 365 Copilot Cowork skills from the shared APM \
                     deployment without replacing Cowork-owned directories",
                ),
            }
            planned = planned.saturating_add(1);
        }
        planned
    }

    /// Run the primary APM command and all target-specific convergence actions.
    ///
    /// The primary command deliberately omits `--target` so the merged manifest
    /// remains authoritative. Copilot App receives a separate experimental
    /// install, while Cowork is reconciled file-by-file from the shared skill
    /// deployment because its `OneDrive` ACL rejects directory replacement.
    ///
    /// # Errors
    ///
    /// Returns an error when APM or a target-specific convergence action fails.
    pub(super) fn run_apm_command(
        self,
        ctx: &Context,
        command: ApmCommand,
    ) -> Result<ApmCommandResult> {
        if self.includes(CopilotTarget::Cowork) {
            remove_legacy_cowork_lock_deployments(ctx.home())?;
        }
        match run_apm_invocation(ctx, command, command.args())? {
            ApmCommandResult::Success => {}
            result @ ApmCommandResult::AuthSkipped(_) => return Ok(result),
        }

        for target in self.active.active() {
            match target.deployment() {
                CopilotDeployment::ExperimentalInstall { config_key, args } => {
                    ensure_experimental_target_enabled(ctx, target.apm_name(), config_key);
                    let result = run_apm_invocation(ctx, ApmCommand::Install, args)?;
                    if !matches!(result, ApmCommandResult::Success) {
                        return Ok(result);
                    }
                }
                CopilotDeployment::CoworkReconcile => reconcile_cowork_skills(ctx)?,
            }
        }
        Ok(ApmCommandResult::Success)
    }

    /// Apply post-command target fixups using the pre-command snapshot.
    pub(super) fn finish(self, ctx: &Context, snapshot: &ManagedTargetSnapshot) {
        if self.includes(CopilotTarget::CopilotApp)
            && let Some(pre) = &snapshot.copilot_app
        {
            apply_workflow_autopilot_fixup(ctx, pre);
        }
    }
}

/// Target state captured before the primary APM command mutates deployments.
#[derive(Debug)]
pub(super) struct ManagedTargetSnapshot {
    copilot_app: Option<DesiredApmWorkflows>,
}

/// Target-specific drift repairable without a primary APM command.
#[derive(Debug, Default)]
pub(super) struct ManagedTargetDrift {
    copilot_app: Option<DesiredApmWorkflows>,
    cowork: bool,
}

impl ManagedTargetDrift {
    #[must_use]
    pub(super) const fn is_empty(&self) -> bool {
        self.copilot_app.is_none() && !self.cowork
    }

    /// Preview target-only drift repair and return the number of planned actions.
    pub(super) fn preview(&self, ctx: &Context) -> u32 {
        let mut planned = 0_u32;
        if self.copilot_app.is_some() {
            ctx.log().dry_run(
                "re-assert dotfiles-managed Copilot App workflows to autopilot + enabled in \
                 ~/.copilot/data.db",
            );
            planned = planned.saturating_add(1);
        }
        if self.cowork {
            ctx.log().dry_run(
                "reconcile Microsoft 365 Copilot Cowork skill files from the shared APM \
                 deployment without replacing Cowork-owned directories",
            );
            planned = planned.saturating_add(1);
        }
        planned
    }

    /// Repair target-only drift and report whether target state changed.
    ///
    /// # Errors
    ///
    /// Returns an error when Cowork reconciliation fails. Copilot App workflow
    /// repair retains its existing best-effort behavior.
    pub(super) fn apply(&self, ctx: &Context) -> Result<bool> {
        let mut changed = false;
        if let Some(pre) = &self.copilot_app {
            changed |= apply_workflow_autopilot_fixup(ctx, pre);
        }
        if self.cowork {
            reconcile_cowork_skills(ctx)?;
            changed = true;
        }
        Ok(changed)
    }

    #[must_use]
    pub(super) const fn change_message(&self) -> &'static str {
        match (self.copilot_app.is_some(), self.cowork) {
            (true, true) => "reconciled managed Copilot targets",
            (true, false) => "re-armed Copilot App workflows",
            (false, true) => "reconciled Microsoft 365 Copilot Cowork skills",
            (false, false) => "managed Copilot targets already current",
        }
    }
}
