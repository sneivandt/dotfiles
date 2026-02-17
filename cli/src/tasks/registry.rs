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
        let entries = &ctx.config.registry;

        // Batch-check all values in a single PowerShell process
        let current_values = batch_check_registry(entries);

        let mut to_set: Vec<usize> = Vec::new();
        let mut already_ok = 0u32;

        for (i, entry) in entries.iter().enumerate() {
            let is_correct = current_values
                .get(i)
                .map(|v| value_matches(v, &entry.value_data))
                .unwrap_or(false);

            if is_correct {
                if ctx.dry_run {
                    ctx.log.debug(&format!(
                        "ok: {}\\{} = {} (already set)",
                        entry.key_path, entry.value_name, entry.value_data
                    ));
                }
                already_ok += 1;
            } else {
                if ctx.dry_run {
                    ctx.log.dry_run(&format!(
                        "would set registry: {}\\{} = {}",
                        entry.key_path, entry.value_name, entry.value_data
                    ));
                }
                to_set.push(i);
            }
        }

        if ctx.dry_run {
            ctx.log.info(&format!(
                "{} would change, {already_ok} already ok",
                to_set.len()
            ));
            return Ok(TaskResult::DryRun);
        }

        // Batch-set all changed values in a single PowerShell process
        if !to_set.is_empty() {
            let failed = batch_set_registry(entries, &to_set);
            if !failed.is_empty() {
                for idx in &failed {
                    let entry = &entries[*idx];
                    ctx.log.warn(&format!(
                        "failed to set registry: {}\\{}",
                        entry.key_path, entry.value_name
                    ));
                }
            }
        }

        ctx.log.info(&format!(
            "{} changed, {already_ok} already ok",
            to_set.len()
        ));
        Ok(TaskResult::Ok)
    }
}

/// Batch-check all registry values in a single PowerShell invocation.
/// Returns a Vec where each entry is either the current value string or
/// a sentinel indicating not-found.
fn batch_check_registry(entries: &[crate::config::registry::RegistryEntry]) -> Vec<String> {
    let sentinel = "::NOT_FOUND::";
    let separator = "::SEP::";

    let mut script = String::from("$ErrorActionPreference='SilentlyContinue'\n");
    for (i, entry) in entries.iter().enumerate() {
        let key = entry.key_path.replace('\'', "''");
        let name = entry.value_name.replace('\'', "''");
        if i > 0 {
            script.push_str(&format!("Write-Output '{separator}'\n"));
        }
        script.push_str(&format!(
            "$v = (Get-ItemProperty -Path '{key}' -Name '{name}' -ErrorAction SilentlyContinue).'{name}'\n\
             if ($null -eq $v) {{ Write-Output '{sentinel}' }} else {{ Write-Output $v }}\n"
        ));
    }

    let result = match exec::run_unchecked("powershell", &["-NoProfile", "-Command", &script]) {
        Ok(r) if r.success => r.stdout,
        _ => return vec![sentinel.to_string(); entries.len()],
    };

    result
        .split(separator)
        .map(|s| s.trim().to_string())
        .collect()
}

/// Batch-set registry values in a single PowerShell invocation.
/// Returns indices of entries that failed.
fn batch_set_registry(
    entries: &[crate::config::registry::RegistryEntry],
    indices: &[usize],
) -> Vec<usize> {
    let mut script = String::new();
    for &i in indices {
        let entry = &entries[i];
        let key = entry.key_path.replace('\'', "''");
        let name = entry.value_name.replace('\'', "''");
        let (ps_value, ps_type) = format_registry_value(&entry.value_data);

        script.push_str(&format!(
            "try {{ if (!(Test-Path '{key}')) {{ New-Item -Path '{key}' -Force | Out-Null }}; \
             Set-ItemProperty -Path '{key}' -Name '{name}' -Value {ps_value} -Type {ps_type} }} \
             catch {{ Write-Error \"FAIL:{i}\" }}\n"
        ));
    }

    let result = match exec::run_unchecked("powershell", &["-NoProfile", "-Command", &script]) {
        Ok(r) => r,
        Err(_) => return indices.to_vec(),
    };

    // Parse failures from stderr
    result
        .stderr
        .lines()
        .filter_map(|line| {
            line.find("FAIL:")
                .and_then(|pos| line[pos + 5..].trim().parse::<usize>().ok())
        })
        .collect()
}

/// Check if a queried value matches the expected data.
fn value_matches(current: &str, expected_data: &str) -> bool {
    if current == "::NOT_FOUND::" {
        return false;
    }

    if let Some(hex) = expected_data
        .strip_prefix("0x")
        .or_else(|| expected_data.strip_prefix("0X"))
        && let Ok(expected_num) = u64::from_str_radix(hex, 16)
    {
        return current.parse::<u64>().ok() == Some(expected_num);
    }
    if let Ok(expected_num) = expected_data.parse::<i64>() {
        return current.parse::<i64>().ok() == Some(expected_num);
    }
    current == expected_data
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
