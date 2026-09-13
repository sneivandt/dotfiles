//! Native APM update-plan preview and parsing.

use anyhow::Result;

use super::commands::{ApmCommand, ApmCommandResult, run_apm_invocation};
use super::managed_targets::{ManagedTargetPreview, ManagedTargets};
use crate::engine::{Context, TaskResult};
use crate::infra::logging::OutputExt as _;

pub(super) fn preview_apm_update(ctx: &Context, targets: ManagedTargets) -> Result<TaskResult> {
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
