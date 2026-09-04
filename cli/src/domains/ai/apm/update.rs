//! APM update task: delegate locked dependency advancement to APM.

use anyhow::Result;

use super::ApmFragmentSource;
use super::commands::{ApmCommand, ApmCommandResult, run_apm_invocation};
use super::install::apm_task_should_run;
use super::managed_targets::{ManagedTargetPreview, ManagedTargets};
use super::manifest::{describe_lock_changes, read_lock_snapshot};
use super::skip;
use super::targets::missing_apm_reason;
use crate::engine::{Context, Task, TaskResult, TaskStats, task_metadata};
use crate::infra::ConfigHandle;
use crate::infra::logging::OutputExt as _;

/// Advance pinned APM dependency versions under `install --update-pins`.
///
/// The catalog dependency on [`super::InstallApmPackages`] ensures the generated
/// manifest is converged before APM advances matching refs.
#[derive(Debug)]
pub struct UpdateApmPackages {
    fragments: ConfigHandle<Vec<ApmFragmentSource>>,
}

impl UpdateApmPackages {
    /// Create the update task with the managed symlink configuration that
    /// supplies APM fragments.
    #[must_use]
    pub const fn new(fragments: ConfigHandle<Vec<ApmFragmentSource>>) -> Self {
        Self { fragments }
    }
}

impl Task for UpdateApmPackages {
    task_metadata! {
        name: "APM package updates",
        selector: "apm-update",
        update_only: true,
    }

    fn should_run(&self, ctx: &Context) -> bool {
        apm_task_should_run(ctx, &self.fragments.read())
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if !ctx.which("apm") {
            return Ok(skip(missing_apm_reason(ctx)));
        }

        let targets = ManagedTargets::detect(ctx)?;
        if ctx.dry_run() {
            return preview_apm_update(ctx, targets);
        }
        advance_apm_dependencies(ctx, targets)
    }
}

fn preview_apm_update(ctx: &Context, targets: ManagedTargets) -> Result<TaskResult> {
    match run_apm_invocation(ctx, ApmCommand::Update, &["update", "-g", "--dry-run"])? {
        ApmCommandResult::Success(result) => {
            let plan = describe_update_plan(&result.stdout);
            if plan.is_empty() {
                ctx.log().dry_run(
                    "use APM's update plan to advance dependencies to their latest matching refs",
                );
            } else {
                for detail in plan {
                    ctx.log().dry_run(detail);
                }
            }
            targets.preview(ctx, ManagedTargetPreview::Update);
            Ok(TaskResult::DryRun)
        }
        ApmCommandResult::AuthSkipped(reason) => Ok(TaskResult::unmet(reason)),
    }
}

fn advance_apm_dependencies(ctx: &Context, targets: ManagedTargets) -> Result<TaskResult> {
    let lock_path = ctx.home().join(".apm").join("apm.lock.yaml");
    let lock_before = read_lock_snapshot(&lock_path)?;
    let target_snapshot = targets.snapshot(ctx);

    match targets.run_apm_command(ctx, ApmCommand::Update)? {
        ApmCommandResult::AuthSkipped(reason) => Ok(TaskResult::unmet(reason)),
        ApmCommandResult::Success(_) => {
            let lock_after = read_lock_snapshot(&lock_path)?;
            let autopilot_changed = targets.finish(ctx, &target_snapshot);
            let lock_changed = lock_before != lock_after;
            let dependency_changes =
                describe_lock_changes(lock_before.as_deref(), lock_after.as_deref());
            for detail in &dependency_changes {
                ctx.log().info(detail);
            }
            if lock_changed && dependency_changes.is_empty() {
                ctx.log().info("updated: APM lock state");
            }

            if lock_changed || autopilot_changed {
                let message = if dependency_changes.is_empty() {
                    "updated APM deployments".to_string()
                } else {
                    updated_dependency_summary(dependency_changes.len())
                };
                ctx.log().trace(format!("APM update summary: {message}"));
                Ok(TaskStats::changed_with_message(message).finish())
            } else {
                ctx.log().debug("APM dependencies already at latest refs");
                Ok(TaskResult::Ok)
            }
        }
    }
}

fn updated_dependency_summary(count: usize) -> String {
    if count == 1 {
        "updated 1 APM dependency".to_string()
    } else {
        format!("updated {count} APM dependencies")
    }
}

fn describe_update_plan(stdout: &str) -> Vec<String> {
    let mut in_plan = false;
    let mut details: Vec<String> = Vec::new();
    for line in stdout.lines().map(str::trim) {
        if line == "[i] Update plan for apm.yml" {
            in_plan = true;
            continue;
        }
        if !in_plan {
            continue;
        }

        if let Some(reference) = line.strip_prefix("ref: ") {
            if let Some(detail) = details.last_mut() {
                detail.push_str(" · ref ");
                detail.push_str(reference);
            }
            continue;
        }

        let action = [
            ("[+] ", "would install: "),
            ("[-] ", "would remove: "),
            ("[~] ", "would update: "),
        ]
        .into_iter()
        .find_map(|(prefix, action)| line.strip_prefix(prefix).map(|name| (action, name)));
        let Some((action, name)) = action else {
            continue;
        };
        if matches!(name, "updated" | "installed" | "removed") {
            continue;
        }
        details.push(format!("{action}{name}"));
    }
    details
}

#[cfg(test)]
mod tests {
    use super::describe_update_plan;

    #[test]
    fn update_plan_promotes_only_package_actions() {
        let stdout = "\
[>] Checking upstream for revision-pin freshness...
[i] Update plan for apm.yml

  [~] cursor/plugins/pstack/skills/unslop
      ref: - -> main (efa2a53 -> 93b00b8)
      files: .agents/skills/unslop/SKILL.md
  [+] example/new-plugin
  [-] example/old-plugin

  3 updated
  [~] updated

[i] Dry run: no changes applied.
";

        assert_eq!(
            describe_update_plan(stdout),
            [
                "would update: cursor/plugins/pstack/skills/unslop · ref - -> main (efa2a53 -> \
                 93b00b8)",
                "would install: example/new-plugin",
                "would remove: example/old-plugin",
            ]
        );
    }

    #[test]
    fn update_plan_returns_no_details_without_native_plan_header() {
        assert!(describe_update_plan("All dependencies are current.\n").is_empty());
    }
}
