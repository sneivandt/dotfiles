use anyhow::Result;

use super::{Context, Task, TaskResult};
use crate::exec;

/// Apply Windows registry settings.
pub struct ApplyRegistry;

impl Task for ApplyRegistry {
    fn name(&self) -> &str {
        "Apply registry settings"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.is_windows() && !ctx.config.registry.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let mut count = 0u32;

        for entry in &ctx.config.registry {
            if ctx.dry_run {
                ctx.log.dry_run(&format!(
                    "registry: {}\\{} = {}",
                    entry.key_path, entry.value_name, entry.value_data
                ));
                count += 1;
                continue;
            }

            // Detect value type: hex (0x...) or plain integer → DWord, else String
            let (ps_value, ps_type) = format_registry_value(&entry.value_data);

            // Escape single quotes in all interpolated values for PowerShell
            let key_escaped = entry.key_path.replace('\'', "''");
            let name_escaped = entry.value_name.replace('\'', "''");

            let script = format!(
                "if (!(Test-Path '{key_escaped}')) {{ New-Item -Path '{key_escaped}' -Force | Out-Null }}; \
                 Set-ItemProperty -Path '{key_escaped}' -Name '{name_escaped}' -Value {ps_value} -Type {ps_type}",
            );

            let result = exec::run_unchecked("powershell", &["-Command", &script])?;
            if result.success {
                count += 1;
            } else {
                ctx.log.warn(&format!(
                    "failed to set registry: {}\\{}: {}",
                    entry.key_path,
                    entry.value_name,
                    result.stderr.trim()
                ));
            }
        }

        if ctx.dry_run {
            return Ok(TaskResult::DryRun);
        }

        ctx.log.info(&format!("{count} registry entries applied"));
        Ok(TaskResult::Ok)
    }
}

/// Format a registry value for PowerShell, returning (value_expr, type_name).
fn format_registry_value(data: &str) -> (String, &'static str) {
    // Hex integer: 0x...
    if let Some(hex) = data.strip_prefix("0x").or_else(|| data.strip_prefix("0X"))
        && let Ok(n) = u64::from_str_radix(hex, 16)
    {
        return (n.to_string(), "DWord");
    }
    // Plain integer
    if data.parse::<i64>().is_ok() {
        return (data.to_string(), "DWord");
    }
    // String value — escape single quotes for PowerShell
    let escaped = data.replace('\'', "''");
    (format!("'{escaped}'"), "String")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_hex_value() {
        let (val, typ) = format_registry_value("0x00200078");
        assert_eq!(typ, "DWord");
        assert_eq!(val, "2097272"); // 0x00200078 as decimal
    }

    #[test]
    fn format_plain_integer() {
        let (val, typ) = format_registry_value("100");
        assert_eq!(typ, "DWord");
        assert_eq!(val, "100");
    }

    #[test]
    fn format_string_value() {
        let (val, typ) = format_registry_value("Cascadia Mono");
        assert_eq!(typ, "String");
        assert_eq!(val, "'Cascadia Mono'");
    }

    #[test]
    fn format_string_with_quotes() {
        let (val, typ) = format_registry_value("it's a test");
        assert_eq!(typ, "String");
        assert_eq!(val, "'it''s a test'");
    }
}
