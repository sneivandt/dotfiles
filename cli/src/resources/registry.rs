//! Windows registry entry resource.
use std::collections::HashMap;

#[cfg(windows)]
use anyhow::Context as _;
use anyhow::Result;

use super::{Applicable, Resource, ResourceChange, ResourceState};
use crate::config::registry::RegistryValueType;

/// Native Windows registry access via the `winreg` crate.
#[cfg(windows)]
mod native {
    use anyhow::{Context as _, Result, bail};
    use winreg::RegKey;
    use winreg::enums::{HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::enums::{REG_DWORD, REG_EXPAND_SZ, REG_SZ};

    use crate::config::registry::RegistryValueType;

    /// Parse a `PowerShell`-style registry path into a root key and subkey.
    fn parse_path(key_path: &str) -> Result<(RegKey, &str)> {
        let (root_str, subkey) = key_path
            .split_once(r":\")
            .ok_or_else(|| anyhow::anyhow!("invalid registry path: {key_path}"))?;
        let root = match root_str {
            "HKCU" => RegKey::predef(HKEY_CURRENT_USER),
            "HKLM" => RegKey::predef(HKEY_LOCAL_MACHINE),
            "HKCR" => RegKey::predef(HKEY_CLASSES_ROOT),
            _ => bail!("unsupported registry root: {root_str}"),
        };
        Ok((root, subkey))
    }

    /// Read a registry value and return it as a string.
    pub fn read_value(key_path: &str, value_name: &str) -> Result<Option<String>> {
        let (root, subkey) = parse_path(key_path)?;
        let key = match root.open_subkey(subkey) {
            Ok(k) => k,
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(anyhow::Error::from(e).context(format!("opening {key_path}"))),
        };
        match key.get_raw_value(value_name) {
            Ok(val) => Ok(Some(raw_value_to_string(&val))),
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => {
                Err(anyhow::Error::from(e).context(format!("reading {key_path}\\{value_name}")))
            }
        }
    }

    /// Write a registry value using the declared type from the config.
    pub fn write_value(
        key_path: &str,
        value_name: &str,
        value_data: &str,
        value_type: RegistryValueType,
    ) -> Result<()> {
        let (root, subkey) = parse_path(key_path)?;
        let (key, _) = root
            .create_subkey(subkey)
            .with_context(|| format!("creating {key_path}"))?;

        match value_type {
            RegistryValueType::Dword => {
                let dword = parse_dword(value_data).with_context(|| {
                    format!("parsing DWORD for {key_path}\\{value_name}: {value_data}")
                })?;
                key.set_value(value_name, &dword)
                    .with_context(|| format!("setting {key_path}\\{value_name}"))?;
            }
            RegistryValueType::String => {
                key.set_value(value_name, &value_data)
                    .with_context(|| format!("setting {key_path}\\{value_name}"))?;
            }
            RegistryValueType::ExpandString => {
                use winreg::RegValue;
                let mut bytes: Vec<u8> = value_data
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect();
                bytes.extend_from_slice(&[0, 0]);
                let raw = RegValue {
                    bytes,
                    vtype: REG_EXPAND_SZ,
                };
                key.set_raw_value(value_name, &raw)
                    .with_context(|| format!("setting {key_path}\\{value_name}"))?;
            }
        }
        Ok(())
    }

    /// Parse a decimal or `0x`-prefixed hex string into a `u32`.
    fn parse_dword(value_data: &str) -> Result<u32> {
        if let Some(hex) = value_data
            .strip_prefix("0x")
            .or_else(|| value_data.strip_prefix("0X"))
        {
            let n = u64::from_str_radix(hex, 16)
                .with_context(|| format!("invalid hex DWORD: {value_data}"))?;
            return u32::try_from(n)
                .with_context(|| format!("hex DWORD exceeds u32 range: {value_data}"));
        }
        value_data
            .parse::<u32>()
            .with_context(|| format!("invalid decimal DWORD: {value_data}"))
    }

    /// Convert a raw registry value to a string representation.
    #[allow(clippy::indexing_slicing)] // chunks_exact guarantees exact sizes
    fn raw_value_to_string(val: &winreg::RegValue) -> String {
        match val.vtype {
            REG_DWORD if val.bytes.len() >= 4 => {
                u32::from_le_bytes([val.bytes[0], val.bytes[1], val.bytes[2], val.bytes[3]])
                    .to_string()
            }
            REG_SZ | REG_EXPAND_SZ => {
                let wide: Vec<u16> = val
                    .bytes
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16_lossy(&wide)
                    .trim_end_matches('\0')
                    .to_string()
            }
            _ => format!("{:?}", val.bytes),
        }
    }
}

/// A Windows registry resource that can be checked and applied.
///
/// Uses the `winreg` crate for native registry access on Windows.
#[derive(Debug)]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct RegistryResource {
    /// Registry key path (e.g., "HKCU:\Console").
    pub key_path: String,
    /// Value name.
    pub value_name: String,
    /// Value data (as string).
    pub value_data: String,
    /// Declared registry value type.
    pub value_type: RegistryValueType,
}

impl RegistryResource {
    /// Create a new registry resource.
    #[must_use]
    pub const fn new(
        key_path: String,
        value_name: String,
        value_data: String,
        value_type: RegistryValueType,
    ) -> Self {
        Self {
            key_path,
            value_name,
            value_data,
            value_type,
        }
    }

    /// Create from a config entry.
    #[must_use]
    pub fn from_entry(entry: &crate::config::registry::RegistryEntry) -> Self {
        Self::new(
            entry.key_path.clone(),
            entry.value_name.clone(),
            entry.value_data.clone(),
            entry.value_type,
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

/// Batch-check all registry values.
///
/// On Windows, reads each value directly via the `winreg` crate. Returns a map
/// from `"key_path\value_name"` to the current value string (`None` when the
/// key or value does not exist).
///
/// # Errors
///
/// Returns an error if a registry value cannot be read.
#[cfg(windows)]
pub fn batch_check_values(
    resources: &[RegistryResource],
) -> Result<HashMap<String, Option<String>>> {
    let mut map = HashMap::with_capacity(resources.len());
    for res in resources {
        let key = format!("{}\\{}", res.key_path, res.value_name);
        let value = native::read_value(&res.key_path, &res.value_name)?;
        map.insert(key, value);
    }
    Ok(map)
}

/// Stub for non-Windows platforms (registry operations are Windows-only).
///
/// # Errors
///
/// This function never returns an error on non-Windows platforms.
#[cfg(not(windows))]
#[allow(clippy::unnecessary_wraps)]
pub fn batch_check_values(
    _resources: &[RegistryResource],
) -> Result<HashMap<String, Option<String>>> {
    Ok(HashMap::new())
}

impl Applicable for RegistryResource {
    fn description(&self) -> String {
        format!(
            "{}\\{} = {}",
            self.key_path, self.value_name, self.value_data
        )
    }

    fn apply(&self) -> Result<ResourceChange> {
        #[cfg(windows)]
        {
            native::write_value(
                &self.key_path,
                &self.value_name,
                &self.value_data,
                self.value_type,
            )
            .with_context(|| format!("set registry: {}\\{}", self.key_path, self.value_name))?;
            Ok(ResourceChange::Applied)
        }
        #[cfg(not(windows))]
        {
            Err(crate::error::ResourceError::not_supported(
                "registry operations are only supported on Windows",
            )
            .into())
        }
    }
}

impl Resource for RegistryResource {
    fn current_state(&self) -> Result<ResourceState> {
        #[cfg(windows)]
        {
            let current_value = native::read_value(&self.key_path, &self.value_name)?;
            Ok(self.state_from_cached(current_value.as_deref()))
        }
        #[cfg(not(windows))]
        {
            Ok(ResourceState::Missing)
        }
    }
}

/// Compare registry values, handling numeric values specially.
#[cfg_attr(not(windows), allow(dead_code))]
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

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn registry_resource_description() {
        let resource = RegistryResource::new(
            "HKCU:\\Console".to_string(),
            "FontSize".to_string(),
            "14".to_string(),
            RegistryValueType::Dword,
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
    fn from_entry_creates_resource() {
        let entry = crate::config::registry::RegistryEntry {
            key_path: "HKCU:\\Test".to_string(),
            value_name: "TestValue".to_string(),
            value_data: "123".to_string(),
            value_type: RegistryValueType::Dword,
        };

        let resource = RegistryResource::from_entry(&entry);
        assert_eq!(resource.key_path, "HKCU:\\Test");
        assert_eq!(resource.value_name, "TestValue");
        assert_eq!(resource.value_data, "123");
        assert_eq!(resource.value_type, RegistryValueType::Dword);
    }

    #[test]
    fn state_from_cached_correct() {
        let resource = RegistryResource::new(
            "HKCU:\\Console".to_string(),
            "FontSize".to_string(),
            "14".to_string(),
            RegistryValueType::Dword,
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
            RegistryValueType::Dword,
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
            RegistryValueType::Dword,
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
            RegistryValueType::Dword,
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
