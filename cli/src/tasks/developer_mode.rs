use anyhow::Result;

use super::{Context, Task, TaskResult};
use crate::exec;

/// Enable Windows Developer Mode (allows symlink creation without admin).
pub struct EnableDeveloperMode;

impl Task for EnableDeveloperMode {
    fn name(&self) -> &str {
        "Enable developer mode"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.is_windows()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let key = r"HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock";
        let name = "AllowDevelopmentWithoutDevLicense";

        // Check current state
        let check_script = format!(
            "try {{ $v = (Get-ItemProperty -Path '{key}' -Name '{name}' -ErrorAction Stop).'{name}'; Write-Output $v }} catch {{ Write-Output '::NOT_FOUND::' }}"
        );
        let current = exec::run_unchecked("powershell", &["-Command", &check_script])
            .map(|r| r.stdout.trim().to_string())
            .unwrap_or_default();

        if current == "1" {
            ctx.log.debug("ok: developer mode already enabled");
            ctx.log.info("already enabled");
            return Ok(TaskResult::Ok);
        }

        if ctx.dry_run {
            ctx.log
                .dry_run("would enable developer mode (AllowDevelopmentWithoutDevLicense = 1)");
            return Ok(TaskResult::DryRun);
        }

        let set_script = format!(
            "if (!(Test-Path '{key}')) {{ New-Item -Path '{key}' -Force | Out-Null }}; \
             Set-ItemProperty -Path '{key}' -Name '{name}' -Value 1 -Type DWord"
        );
        let result = exec::run_unchecked("powershell", &["-Command", &set_script])?;
        if result.success {
            ctx.log.info("enabled");
        } else {
            ctx.log.warn(&format!(
                "failed to enable developer mode: {}",
                result.stderr.trim()
            ));
        }

        Ok(TaskResult::Ok)
    }
}
