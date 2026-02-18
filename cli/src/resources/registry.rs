use std::collections::HashMap;
use std::fmt::Write;

use anyhow::{Context as _, Result};

use super::{Resource, ResourceChange, ResourceState};
use crate::exec;

/// A Windows registry resource that can be checked and applied.
#[derive(Debug, Clone)]
pub struct RegistryResource {
    /// Registry key path (e.g., "HKCU:\Console").
    pub key_path: String,
    /// Value name.
    pub value_name: String,
    /// Value data (as string).
    pub value_data: String,
}

impl RegistryResource {
    /// Create a new registry resource.
    #[must_use]
    pub const fn new(key_path: String, value_name: String, value_data: String) -> Self {
        Self {
            key_path,
            value_name,
            value_data,
        }
    }

    /// Create from a config entry.
    #[must_use]
    pub fn from_entry(entry: &crate::config::registry::RegistryEntry) -> Self {
        Self::new(
            entry.key_path.clone(),
            entry.value_name.clone(),
            entry.value_data.clone(),
        )
    }

    /// Determine the resource state from a pre-fetched current value.
    ///
    /// This avoids spawning a `PowerShell` process per resource when used
    /// with [`batch_check_values`].
    #[must_use]
    pub fn state_from_cached(&self, current_value: Option<&str>) -> ResourceState {
        current_value.map_or(ResourceState::Missing, |current| {
            if value_matches(current, &self.value_data) {
                ResourceState::Correct
            } else {
                ResourceState::Incorrect {
                    current: current.to_string(),
                }
            }
        })
    }
}

/// Batch-check all registry values in a single `PowerShell` invocation.
///
/// Returns a map from `"key_path\value_name"` to the current value string
/// (`None` when the key or value does not exist). This is **dramatically**
/// faster than spawning one process per registry entry.
pub fn batch_check_values(
    resources: &[RegistryResource],
) -> Result<HashMap<String, Option<String>>> {
    if resources.is_empty() {
        return Ok(HashMap::new());
    }

    let sentinel = "::NOT_FOUND::";
    let separator = "::SEP::";

    // Build a single script that checks every value and prints results on
    // separate lines, delimited by a separator token so we can parse them.
    let mut script = String::from("$ErrorActionPreference='SilentlyContinue'\n");
    for (i, res) in resources.iter().enumerate() {
        let key = res.key_path.replace('\'', "''");
        let name = res.value_name.replace('\'', "''");
        if i > 0 {
            let _ = writeln!(script, "Write-Output '{separator}'");
        }
        let _ = write!(
            script,
            "$v = (Get-ItemProperty -Path '{key}' -Name '{name}' -ErrorAction SilentlyContinue).'{name}'\n\
             if ($null -eq $v) {{ Write-Output '{sentinel}' }} else {{ Write-Output $v }}\n"
        );
    }

    let result = exec::run_unchecked("powershell", &["-NoProfile", "-Command", &script])?;

    let mut map = HashMap::with_capacity(resources.len());

    if !result.success {
        // If the whole script failed, treat every value as unknown/missing
        for res in resources {
            let key = format!("{}\\{}", res.key_path, res.value_name);
            map.insert(key, None);
        }
        return Ok(map);
    }

    // Split output by the separator token
    let chunks: Vec<&str> = result.stdout.split(separator).collect();
    for (i, res) in resources.iter().enumerate() {
        let map_key = format!("{}\\{}", res.key_path, res.value_name);
        let value = chunks.get(i).and_then(|chunk| {
            let trimmed = chunk.trim();
            if trimmed == sentinel {
                None
            } else {
                Some(trimmed.to_string())
            }
        });
        map.insert(map_key, value);
    }

    Ok(map)
}

impl Resource for RegistryResource {
    fn description(&self) -> String {
        format!(
            "{}\\{} = {}",
            self.key_path, self.value_name, self.value_data
        )
    }

    fn current_state(&self) -> Result<ResourceState> {
        // Check current registry value
        let current_value = check_registry_value(&self.key_path, &self.value_name)?;

        current_value.map_or_else(
            || Ok(ResourceState::Missing),
            |current| {
                if value_matches(&current, &self.value_data) {
                    Ok(ResourceState::Correct)
                } else {
                    Ok(ResourceState::Incorrect { current })
                }
            },
        )
    }

    fn apply(&self) -> Result<ResourceChange> {
        set_registry_value(&self.key_path, &self.value_name, &self.value_data)
            .with_context(|| format!("set registry: {}\\{}", self.key_path, self.value_name))?;

        Ok(ResourceChange::Applied)
    }
}

/// Check a single registry value using `PowerShell`.
/// Returns `Some(value)` if found, `None` if not found or on error.
fn check_registry_value(key_path: &str, value_name: &str) -> Result<Option<String>> {
    let sentinel = "::NOT_FOUND::";
    let key = key_path.replace('\'', "''");
    let name = value_name.replace('\'', "''");

    let script = format!(
        "$ErrorActionPreference='SilentlyContinue'\n\
         $v = (Get-ItemProperty -Path '{key}' -Name '{name}' -ErrorAction SilentlyContinue).'{name}'\n\
         if ($null -eq $v) {{ Write-Output '{sentinel}' }} else {{ Write-Output $v }}"
    );

    let result = exec::run_unchecked("powershell", &["-NoProfile", "-Command", &script])?;

    if !result.success {
        return Ok(None);
    }

    let output = result.stdout.trim();
    if output == sentinel {
        Ok(None)
    } else {
        Ok(Some(output.to_string()))
    }
}

/// Set a registry value using `PowerShell`.
fn set_registry_value(key_path: &str, value_name: &str, value_data: &str) -> Result<()> {
    let key = key_path.replace('\'', "''");
    let name = value_name.replace('\'', "''");
    let (ps_value, ps_type) = format_registry_value(value_data);

    let script = format!(
        "if (!(Test-Path '{key}')) {{ New-Item -Path '{key}' -Force | Out-Null }}\n\
         Set-ItemProperty -Path '{key}' -Name '{name}' -Value {ps_value} -Type {ps_type}"
    );

    let result = exec::run("powershell", &["-NoProfile", "-Command", &script])?;

    if !result.success {
        anyhow::bail!("PowerShell command failed: {}", result.stderr);
    }

    Ok(())
}

/// Compare registry values, handling numeric values specially.
fn value_matches(current: &str, expected_data: &str) -> bool {
    // Handle hex values
    if let Some(hex) = expected_data
        .strip_prefix("0x")
        .or_else(|| expected_data.strip_prefix("0X"))
        && let Ok(expected_num) = u64::from_str_radix(hex, 16)
    {
        return current.parse::<u64>().ok() == Some(expected_num);
    }

    // Try numeric comparison
    if let Ok(expected_num) = expected_data.parse::<i64>() {
        return current.parse::<i64>().ok() == Some(expected_num);
    }

    // Fall back to string comparison
    current == expected_data
}

/// Format a value string for `PowerShell` `Set-ItemProperty`.
/// Returns (`value_expression`, `type_name`).
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

    // String value - needs quoting and escaping
    let escaped = data.replace('\'', "''");
    (format!("'{escaped}'"), "String")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_resource_description() {
        let resource = RegistryResource::new(
            "HKCU:\\Console".to_string(),
            "FontSize".to_string(),
            "14".to_string(),
        );
        assert_eq!(resource.description(), "HKCU:\\Console\\FontSize = 14");
    }

    #[test]
    fn value_matches_numeric() {
        assert!(value_matches("14", "14"));
        assert!(value_matches("14", "0x0E")); // 0x0E = 14 decimal
        assert!(!value_matches("14", "15"));
    }

    #[test]
    fn value_matches_string() {
        assert!(value_matches("test", "test"));
        assert!(!value_matches("test", "other"));
    }

    #[test]
    fn format_decimal_integer() {
        let (value, type_name) = format_registry_value("14");
        assert_eq!(value, "14");
        assert_eq!(type_name, "DWord");
    }

    #[test]
    fn format_hex_integer() {
        let (value, type_name) = format_registry_value("0x0E");
        assert_eq!(value, "14"); // Hex 0x0E = decimal 14
        assert_eq!(type_name, "DWord");
    }

    #[test]
    fn format_string_value() {
        let (value, type_name) = format_registry_value("test value");
        assert_eq!(value, "'test value'");
        assert_eq!(type_name, "String");
    }

    #[test]
    fn format_string_with_quotes() {
        let (value, type_name) = format_registry_value("test's value");
        assert_eq!(value, "'test''s value'");
        assert_eq!(type_name, "String");
    }

    #[test]
    fn from_entry_creates_resource() {
        let entry = crate::config::registry::RegistryEntry {
            key_path: "HKCU:\\Test".to_string(),
            value_name: "TestValue".to_string(),
            value_data: "123".to_string(),
        };

        let resource = RegistryResource::from_entry(&entry);
        assert_eq!(resource.key_path, "HKCU:\\Test");
        assert_eq!(resource.value_name, "TestValue");
        assert_eq!(resource.value_data, "123");
    }

    #[test]
    fn state_from_cached_correct() {
        let resource = RegistryResource::new(
            "HKCU:\\Console".to_string(),
            "FontSize".to_string(),
            "14".to_string(),
        );
        let state = resource.state_from_cached(Some("14"));
        assert_eq!(state, ResourceState::Correct);
    }

    #[test]
    fn state_from_cached_incorrect() {
        let resource = RegistryResource::new(
            "HKCU:\\Console".to_string(),
            "FontSize".to_string(),
            "14".to_string(),
        );
        let state = resource.state_from_cached(Some("20"));
        assert!(matches!(state, ResourceState::Incorrect { .. }));
    }

    #[test]
    fn state_from_cached_missing() {
        let resource = RegistryResource::new(
            "HKCU:\\Console".to_string(),
            "FontSize".to_string(),
            "14".to_string(),
        );
        let state = resource.state_from_cached(None);
        assert_eq!(state, ResourceState::Missing);
    }

    #[test]
    fn state_from_cached_hex_match() {
        let resource = RegistryResource::new(
            "HKCU:\\Console".to_string(),
            "FontSize".to_string(),
            "0x0E".to_string(),
        );
        // 0x0E = 14 decimal
        let state = resource.state_from_cached(Some("14"));
        assert_eq!(state, ResourceState::Correct);
    }

    #[test]
    fn batch_check_values_empty() {
        let result = batch_check_values(&[]).unwrap();
        assert!(result.is_empty());
    }
}
