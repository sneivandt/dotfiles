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
pub(crate) struct CurrentRegistryValue {
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
pub(crate) type RegistryTarget = (String, String);

/// Native Windows registry access via the `winreg` crate.
#[cfg(windows)]
mod native {
    use anyhow::{Context as _, Result};
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE};
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
        let key = match root.open_subkey_with_flags(subkey, KEY_QUERY_VALUE) {
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
            .create_subkey_with_flags(subkey, KEY_SET_VALUE)
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
    #[expect(
        clippy::indexing_slicing,
        reason = "each arm indexes only within a length checked above"
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
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .copied()
                    .map(u16::from_le_bytes)
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

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn native_registry_decoding_preserves_data_and_declared_types() {
            for (bytes, expected) in [
                (vec![0, 0, 0, 0], 0_u32),
                (vec![255, 255, 255, 255], u32::MAX),
                (vec![1, 0, 0, 0, 255], 1),
            ] {
                let value = winreg::RegValue {
                    bytes: bytes.into(),
                    vtype: REG_DWORD,
                };
                assert_eq!(raw_value_to_string(&value), expected.to_string());
                assert_eq!(raw_value_type(&value), Some(RegistryValueType::Dword));
            }
            for (vtype, expected_type) in [
                (REG_SZ, Some(RegistryValueType::String)),
                (REG_EXPAND_SZ, None),
            ] {
                let value = winreg::RegValue {
                    bytes: "café 🦀 %USERPROFILE%\0"
                        .encode_utf16()
                        .flat_map(u16::to_le_bytes)
                        .collect(),
                    vtype,
                };
                assert_eq!(raw_value_to_string(&value), "café 🦀 %USERPROFILE%");
                assert_eq!(raw_value_type(&value), expected_type);
            }
        }

        #[test]
        fn unsupported_registry_types_and_short_dwords_are_not_coerced() {
            for vtype in [
                REG_NONE,
                REG_BINARY,
                REG_DWORD_BIG_ENDIAN,
                REG_LINK,
                REG_MULTI_SZ,
                REG_RESOURCE_LIST,
                REG_FULL_RESOURCE_DESCRIPTOR,
                REG_RESOURCE_REQUIREMENTS_LIST,
                REG_QWORD,
            ] {
                let value = winreg::RegValue {
                    bytes: vec![1, 2, 3].into(),
                    vtype,
                };
                assert_eq!(raw_value_to_string(&value), "[1, 2, 3]");
                assert_eq!(raw_value_type(&value), None);
            }
            let short = winreg::RegValue {
                bytes: vec![1, 2, 3].into(),
                vtype: REG_DWORD,
            };
            assert_eq!(raw_value_to_string(&short), "[1, 2, 3]");
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
pub(crate) struct RegistryResource {
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
    pub(crate) const fn new(
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
    pub(crate) fn from_entry(
        entry: &crate::domains::system::config::registry::RegistryEntry,
    ) -> Self {
        Self::new(
            entry.key_path.clone(),
            entry.value_name.clone(),
            entry.value_data.clone(),
            entry.value_type,
        )
    }

    pub(crate) fn cache_key(&self) -> RegistryTarget {
        crate::domains::system::config::registry::target_identity(&self.key_path, &self.value_name)
    }

    /// Determine the resource state from a pre-fetched current value.
    ///
    /// [`batch_check_states`] reads all values before convergence starts.
    #[must_use]
    pub(crate) fn state_from_cached(
        &self,
        current_value: Option<&CurrentRegistryValue>,
    ) -> ResourceState {
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

/// Snapshot independent reads: a denied key must not hide another key's state.
pub(crate) fn batch_check_states(
    resources: &[RegistryResource],
) -> HashMap<RegistryTarget, ResourceState> {
    #[cfg(windows)]
    let read = native::read_value;
    #[cfg(not(windows))]
    let read =
        |_: &str, _: &str| anyhow::bail!("registry operations are only supported on Windows");
    batch_check_with(resources, read)
}

fn batch_check_with(
    resources: &[RegistryResource],
    mut read: impl FnMut(&str, &str) -> Result<Option<CurrentRegistryValue>>,
) -> HashMap<RegistryTarget, ResourceState> {
    resources
        .iter()
        .map(|res| {
            let state = match read(&res.key_path, &res.value_name) {
                Ok(value) => res.state_from_cached(value.as_ref()),
                Err(error) => ResourceState::Unknown {
                    reason: format!("{error:#}"),
                },
            };
            (res.cache_key(), state)
        })
        .collect()
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
    fn cache_key_case_folds_and_normalizes_separators() {
        let upper = RegistryResource::new(
            r"HKCU:\Console".to_string(),
            "FontSize".to_string(),
            "14".to_string(),
            RegistryValueType::Dword,
        );
        let mixed = RegistryResource::new(
            r"hkcu:\\console\".to_string(),
            "fontsize".to_string(),
            "14".to_string(),
            RegistryValueType::Dword,
        );
        assert_eq!(upper.cache_key(), mixed.cache_key());
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
    fn denied_registry_key_does_not_hide_independent_reads_or_become_missing() {
        let resources = ["Denied", "Pending", "Current"].map(|name| {
            RegistryResource::new(
                r"HKCU:\Example".into(),
                name.into(),
                "1".into(),
                RegistryValueType::Dword,
            )
        });
        let [denied, pending, current] = &resources;
        let mut reads = Vec::new();
        let cached = batch_check_with(&resources, |_, name| {
            reads.push(name.to_owned());
            match name {
                "Denied" => anyhow::bail!("fixture: query denied"),
                "Pending" => Ok(None),
                "Current" => Ok(Some(CurrentRegistryValue::new(
                    "1".into(),
                    Some(RegistryValueType::Dword),
                ))),
                _ => panic!("unexpected read of {name}"),
            }
        });
        assert_eq!(reads, ["Denied", "Pending", "Current"]);
        assert_eq!(
            cached[&denied.cache_key()],
            ResourceState::Unknown {
                reason: "fixture: query denied".into()
            }
        );
        assert_eq!(cached[&pending.cache_key()], ResourceState::Missing);
        assert_eq!(cached[&current.cache_key()], ResourceState::Correct);
        let retry = batch_check_with(&resources, |_, _| {
            Ok(Some(CurrentRegistryValue::new(
                "1".into(),
                Some(RegistryValueType::Dword),
            )))
        });
        assert_eq!(retry.len(), resources.len());
        assert!(retry.values().all(|state| *state == ResourceState::Correct));
    }
}
