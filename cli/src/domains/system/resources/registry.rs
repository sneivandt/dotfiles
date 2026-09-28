//! Windows registry entry resource.
use std::collections::HashMap;

#[cfg(windows)]
use anyhow::Context as _;
use anyhow::Result;
#[cfg(any(windows, test))]
use anyhow::bail;

use crate::domains::system::config::registry::{RegistryValueType, parse_dword};
use crate::engine::{Resource, ResourceChange, ResourceResult, ResourceState};

/// Current registry value data and its native value type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentRegistryValue {
    data: String,
    value_type: Option<RegistryValueType>,
}

impl CurrentRegistryValue {
    #[cfg(any(windows, test))]
    const fn new(data: String, value_type: Option<RegistryValueType>) -> Self {
        Self { data, value_type }
    }
}

/// A key path and value name kept separate because value names may contain `\`.
pub type RegistryTarget = (String, String);

/// Native Windows registry access via the `winreg` crate.
#[cfg(windows)]
mod native {
    use anyhow::{Context as _, Result};
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::enums::{
        REG_BINARY, REG_DWORD, REG_DWORD_BIG_ENDIAN, REG_EXPAND_SZ, REG_FULL_RESOURCE_DESCRIPTOR,
        REG_LINK, REG_MULTI_SZ, REG_NONE, REG_QWORD, REG_RESOURCE_LIST,
        REG_RESOURCE_REQUIREMENTS_LIST, REG_SZ,
    };

    use super::{CurrentRegistryValue, parse_hkcu_subkey};
    use crate::domains::system::config::registry::{RegistryValueType, parse_dword};

    /// Parse a `PowerShell`-style registry path into a root key and subkey.
    fn parse_path(key_path: &str) -> Result<(RegKey, &str)> {
        let subkey = parse_hkcu_subkey(key_path)?;
        Ok((RegKey::predef(HKEY_CURRENT_USER), subkey))
    }

    /// Read a registry value and preserve both its data and native type.
    pub(super) fn read_value(
        key_path: &str,
        value_name: &str,
    ) -> Result<Option<CurrentRegistryValue>> {
        let (root, subkey) = parse_path(key_path)?;
        let key = match root.open_subkey(subkey) {
            Ok(k) => k,
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(anyhow::Error::from(e).context(format!("opening {key_path}"))),
        };
        match key.get_raw_value(value_name) {
            Ok(val) => Ok(Some(CurrentRegistryValue::new(
                raw_value_to_string(&val),
                raw_value_type(&val),
            ))),
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => {
                Err(anyhow::Error::from(e).context(format!("reading {key_path}\\{value_name}")))
            }
        }
    }

    /// Write a registry value using the declared type from the config.
    pub(super) fn write_value(
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
        }
        Ok(())
    }

    /// Convert a raw registry value to a string representation.
    #[allow(
        clippy::indexing_slicing,
        reason = "each arm indexes only within a length checked above or a chunks_exact window"
    )]
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
            REG_NONE
            | REG_BINARY
            | REG_DWORD
            | REG_DWORD_BIG_ENDIAN
            | REG_LINK
            | REG_MULTI_SZ
            | REG_RESOURCE_LIST
            | REG_FULL_RESOURCE_DESCRIPTOR
            | REG_RESOURCE_REQUIREMENTS_LIST
            | REG_QWORD => format!("{:?}", val.bytes),
        }
    }

    const fn raw_value_type(val: &winreg::RegValue) -> Option<RegistryValueType> {
        match val.vtype {
            REG_DWORD => Some(RegistryValueType::Dword),
            REG_SZ => Some(RegistryValueType::String),
            REG_NONE
            | REG_BINARY
            | REG_DWORD_BIG_ENDIAN
            | REG_EXPAND_SZ
            | REG_LINK
            | REG_MULTI_SZ
            | REG_RESOURCE_LIST
            | REG_FULL_RESOURCE_DESCRIPTOR
            | REG_RESOURCE_REQUIREMENTS_LIST
            | REG_QWORD => None,
        }
    }
}

#[cfg(any(windows, test))]
fn parse_hkcu_subkey(key_path: &str) -> Result<&str> {
    let (root, subkey) = key_path
        .split_once(r":\")
        .ok_or_else(|| anyhow::anyhow!("invalid registry path: {key_path}"))?;
    if !root.eq_ignore_ascii_case("HKCU") {
        bail!("unsupported registry root '{root}': only HKCU is allowed");
    }
    Ok(subkey)
}

/// A Windows registry resource that can be checked and applied.
///
/// Uses the `winreg` crate for native registry access on Windows.
#[derive(Debug)]
#[cfg_attr(not(windows), allow(dead_code, reason = "used conditionally via cfg"))]
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
    pub fn from_entry(entry: &crate::domains::system::config::registry::RegistryEntry) -> Self {
        Self::new(
            entry.key_path.clone(),
            entry.value_name.clone(),
            entry.value_data.clone(),
            entry.value_type,
        )
    }

    pub(crate) fn cache_key(&self) -> RegistryTarget {
        (self.key_path.clone(), self.value_name.clone())
    }

    /// Determine the resource state from a pre-fetched current value.
    ///
    /// [`batch_check_values`] reads all values before convergence starts.
    #[must_use]
    pub fn state_from_cached(&self, current_value: Option<&CurrentRegistryValue>) -> ResourceState {
        current_value.map_or(ResourceState::Missing, |current| {
            if value_matches(current, &self.value_data, self.value_type) {
                ResourceState::Correct
            } else {
                ResourceState::Incorrect {
                    current: current.data.clone(),
                }
            }
        })
    }
}

/// Batch-check all registry values.
///
/// On Windows, reads each value directly via the `winreg` crate. Returns a map
/// from `(key_path, value_name)` to the current typed value (`None` when the key
/// or value does not exist).
///
/// # Errors
///
/// Returns an error if a registry value cannot be read.
#[cfg(windows)]
pub fn batch_check_values(
    resources: &[RegistryResource],
) -> Result<HashMap<RegistryTarget, Option<CurrentRegistryValue>>> {
    let mut map = HashMap::with_capacity(resources.len());
    for res in resources {
        let value = native::read_value(&res.key_path, &res.value_name)?;
        map.insert(res.cache_key(), value);
    }
    Ok(map)
}

/// Stub for non-Windows platforms (registry operations are Windows-only).
///
/// # Errors
///
/// This function never returns an error on non-Windows platforms.
#[cfg(not(windows))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "mirrors the fallible Windows implementation this stub replaces"
)]
pub fn batch_check_values(
    _resources: &[RegistryResource],
) -> Result<HashMap<RegistryTarget, Option<CurrentRegistryValue>>> {
    Ok(HashMap::new())
}

impl Resource for RegistryResource {
    fn description(&self) -> String {
        format!(
            "{}\\{} = {}",
            self.key_path, self.value_name, self.value_data
        )
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        #[cfg(windows)]
        {
            native::write_value(
                &self.key_path,
                &self.value_name,
                &self.value_data,
                self.value_type,
            )
            .with_context(|| {
                format!("configure registry: {}\\{}", self.key_path, self.value_name)
            })?;
            Ok(ResourceChange::Applied)
        }
        #[cfg(not(windows))]
        {
            Err(crate::engine::resource::ResourceError::not_supported(
                "registry operations are only supported on Windows",
            ))
        }
    }
}

/// Compare registry values without losing the native registry type.
#[cfg_attr(not(windows), allow(dead_code, reason = "used conditionally via cfg"))]
fn value_matches(
    current: &CurrentRegistryValue,
    expected_data: &str,
    expected_type: RegistryValueType,
) -> bool {
    if current.value_type != Some(expected_type) {
        return false;
    }

    match expected_type {
        RegistryValueType::Dword => {
            let current = parse_dword(&current.data).ok();
            let expected = parse_dword(expected_data).ok();
            current.is_some() && current == expected
        }
        RegistryValueType::String => current.data == expected_data,
    }
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
            RegistryValueType::Dword,
        );
        assert_eq!(resource.description(), "HKCU:\\Console\\FontSize = 14");
    }

    #[test]
    fn parse_hkcu_subkey_accepts_current_user_paths_case_insensitively() {
        assert_eq!(parse_hkcu_subkey(r"HKCU:\Console").unwrap(), "Console");
        assert_eq!(parse_hkcu_subkey(r"hkcu:\Console").unwrap(), "Console");
    }

    #[test]
    fn parse_hkcu_subkey_rejects_other_hives() {
        let error = parse_hkcu_subkey(r"HKLM:\Software\Test").unwrap_err();
        assert!(
            error.to_string().contains("only HKCU is allowed"),
            "unexpected registry scope error: {error}"
        );
    }

    #[test]
    fn cached_state_preserves_native_types_and_numeric_identity() {
        use RegistryValueType::{Dword, String as Text};

        let max_dword = u32::MAX.to_string();
        for (label, desired, desired_type, current, current_type, correct) in [
            ("decimal", "14", Dword, "14", Some(Dword), true),
            ("hexadecimal", "0x0E", Dword, "14", Some(Dword), true),
            (
                "signed bit pattern",
                "-1",
                Dword,
                max_dword.as_str(),
                Some(Dword),
                true,
            ),
            ("different number", "14", Dword, "20", Some(Dword), false),
            (
                "invalid desired",
                "not-a-number",
                Dword,
                "14",
                Some(Dword),
                false,
            ),
            (
                "invalid current",
                "14",
                Dword,
                "not-a-number",
                Some(Dword),
                false,
            ),
            (
                "two invalid numbers",
                "bad",
                Dword,
                "bad",
                Some(Dword),
                false,
            ),
            ("matching text", "test", Text, "test", Some(Text), true),
            ("different text", "test", Text, "other", Some(Text), false),
            ("text case", "Test", Text, "test", Some(Text), false),
            (
                "numeric text is literal",
                "0x0E",
                Text,
                "14",
                Some(Text),
                false,
            ),
            (
                "string instead of DWORD",
                "14",
                Dword,
                "14",
                Some(Text),
                false,
            ),
            (
                "DWORD instead of string",
                "14",
                Text,
                "14",
                Some(Dword),
                false,
            ),
            ("unsupported native type", "14", Dword, "14", None, false),
        ] {
            let entry = crate::domains::system::config::registry::RegistryEntry {
                key_path: r"HKCU:\Test".into(),
                value_name: "Setting".into(),
                value_data: desired.into(),
                value_type: desired_type,
                origin: None,
            };
            let resource = RegistryResource::from_entry(&entry);
            let observed = CurrentRegistryValue::new(current.into(), current_type);
            let expected = if correct {
                ResourceState::Correct
            } else {
                ResourceState::Incorrect {
                    current: current.into(),
                }
            };
            assert_eq!(
                resource.state_from_cached(Some(&observed)),
                expected,
                "{label}"
            );
            assert_eq!(
                resource.state_from_cached(None),
                ResourceState::Missing,
                "{label}"
            );
        }
    }

    #[test]
    fn cached_values_distinguish_backslashes_in_value_names_from_subkeys() {
        let resources = [
            RegistryResource::new(
                r"HKCU:\A".to_string(),
                r"B\C".to_string(),
                "1".to_string(),
                RegistryValueType::Dword,
            ),
            RegistryResource::new(
                r"HKCU:\A\B".to_string(),
                "C".to_string(),
                "2".to_string(),
                RegistryValueType::Dword,
            ),
        ];
        let cached: HashMap<_, _> = resources
            .iter()
            .map(|resource| {
                (
                    resource.cache_key(),
                    Some(CurrentRegistryValue::new(
                        resource.value_data.clone(),
                        Some(resource.value_type),
                    )),
                )
            })
            .collect();

        assert_eq!(cached.len(), 2);
        for resource in &resources {
            assert_eq!(
                resource
                    .state_from_cached(cached.get(&resource.cache_key()).and_then(Option::as_ref)),
                ResourceState::Correct,
                "{:?}",
                resource.cache_key()
            );
        }
    }

    #[test]
    fn batch_check_values_empty() {
        let result = batch_check_values(&[]).unwrap();
        assert!(result.is_empty());
    }
}
