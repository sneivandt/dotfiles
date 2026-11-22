//! Lifecycle for conditionally available Copilot targets.

use anyhow::Result;

use super::autopilot::{
    DesiredApmWorkflows, WorkflowRepair, apply_workflow_autopilot_fixup,
    snapshot_desired_apm_workflow_ids,
};
use super::commands::{
    ApmCommand, ApmCommandResult, ensure_experimental_target_enabled, run_apm_invocation,
};
use super::cowork::{reconcile_cowork_skills, remove_legacy_cowork_lock_deployments};
use super::targets::{ApmTargets, CopilotDeployment, CopilotTarget};
use crate::engine::Context;

/// Command outcome plus changes made by dotfiles-owned deployment adapters.
#[derive(Debug)]
pub(super) struct ManagedCommandResult {
    pub(super) outcome: ApmCommandResult,
    pub(super) changed: bool,
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

    /// Snapshot Copilot App state that must survive the APM command.
    #[must_use]
    pub(super) fn snapshot(self, ctx: &Context) -> ManagedTargetSnapshot {
        ManagedTargetSnapshot {
            copilot_app: self
                .active
                .includes(CopilotTarget::CopilotApp)
                .then(|| snapshot_desired_apm_workflow_ids(ctx)),
        }
    }

    /// Preview every target-specific convergence action.
    pub(super) fn preview(self, ctx: &Context, command: ApmCommand) -> u32 {
        let (verb, qualifier) = match command {
            ApmCommand::Install => ("sync", ""),
            ApmCommand::Update => ("redeploy", "updated "),
        };
        let mut planned = 0_u32;
        for target in self.active.active() {
            match target.deployment() {
                CopilotDeployment::NativeApm { args, .. } => {
                    let subject = format!("apm {}", args.join(" "));
                    ctx.log().action(
                        "run",
                        &subject,
                        true,
                        &format!(
                            "run {subject} to {verb} {qualifier}{}",
                            target.display_name()
                        ),
                    );
                }
                CopilotDeployment::CoworkFileReconcile { .. } => {
                    let subject = format!("{qualifier}{} skills", target.display_name());
                    ctx.log().action(
                        verb,
                        &subject,
                        true,
                        &format!(
                            "{verb} {subject} file-by-file without replacing Cowork-owned directories"
                        ),
                    );
                }
            }
            if target == CopilotTarget::CopilotApp {
                ctx.log().action(
                    "restore",
                    "dotfiles-managed Copilot App workflow automation state and schedules",
                    true,
                    "restore dotfiles-managed Copilot App workflow automation state and \
                     schedules in ~/.copilot/data.db",
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
    ) -> Result<ManagedCommandResult> {
        let mut changed = remove_legacy_cowork_lock_deployments(ctx)?;
        let primary_result = match run_apm_invocation(ctx, command, command.args())? {
            result @ ApmCommandResult::Success(_) => result,
            result @ ApmCommandResult::AuthSkipped(_) => {
                return Ok(ManagedCommandResult {
                    outcome: result,
                    changed,
                });
            }
        };

        // `apm update` can return early when remote refs are unchanged, without
        // reconciling local packages or deployment drift. Always converge the
        // resulting graph before copying skills or repairing managed targets.
        if matches!(command, ApmCommand::Update) {
            let result = run_apm_invocation(ctx, ApmCommand::Install, ApmCommand::Install.args())?;
            if !matches!(result, ApmCommandResult::Success(_)) {
                return Ok(ManagedCommandResult {
                    outcome: result,
                    changed,
                });
            }
        }

        for target in self.active.active() {
            match target.deployment() {
                CopilotDeployment::NativeApm { config_key, args } => {
                    ensure_experimental_target_enabled(ctx, target.apm_name(), config_key)?;
                    let result = run_apm_invocation(ctx, ApmCommand::Install, args)?;
                    if !matches!(result, ApmCommandResult::Success(_)) {
                        return Ok(ManagedCommandResult {
                            outcome: result,
                            changed,
                        });
                    }
                }
                CopilotDeployment::CoworkFileReconcile { config_key } => {
                    ensure_experimental_target_enabled(ctx, target.apm_name(), config_key)?;
                    changed |= reconcile_cowork_skills(ctx)?;
                }
            }
        }
        Ok(ManagedCommandResult {
            outcome: primary_result,
            changed,
        })
    }
}

/// Target state captured before APM mutates deployments.
#[derive(Debug)]
pub(super) struct ManagedTargetSnapshot {
    copilot_app: Option<DesiredApmWorkflows>,
}

impl ManagedTargetSnapshot {
    /// Re-apply the retained Copilot App autopilot policy.
    pub(super) fn finish(&self, ctx: &Context) -> Result<WorkflowRepair> {
        self.copilot_app.as_ref().map_or_else(
            || Ok(WorkflowRepair::NotApplicable),
            |pre| apply_workflow_autopilot_fixup(ctx, pre),
        )
    }
}
