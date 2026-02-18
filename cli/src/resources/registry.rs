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
}
