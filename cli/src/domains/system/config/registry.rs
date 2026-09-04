//! Windows registry entry configuration loading.
use anyhow::Result;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

use crate::infra::config::Diagnostic;
use crate::infra::config::DiagnosticCode;
use crate::infra::config::toml_loader;
use crate::infra::config::validation::Validator;

/// Diagnostic code: `registry.empty-key-path`.
const REGISTRY_EMPTY_KEY_PATH: DiagnosticCode = DiagnosticCode::new("registry", "empty-key-path");
/// Diagnostic code: `registry.empty-value-name`.
const REGISTRY_EMPTY_VALUE_NAME: DiagnosticCode =
    DiagnosticCode::new("registry", "empty-value-name");
/// Diagnostic code: `registry.platform-unsupported`.
const REGISTRY_PLATFORM_UNSUPPORTED: DiagnosticCode =
    DiagnosticCode::new("registry", "platform-unsupported");
/// Diagnostic code: `registry.unsupported-hive`.
const REGISTRY_UNSUPPORTED_HIVE: DiagnosticCode =
    DiagnosticCode::new("registry", "unsupported-hive");
/// Diagnostic code: `registry.conflicting-values`.
const REGISTRY_CONFLICTING_VALUES: DiagnosticCode =
    DiagnosticCode::new("registry", "conflicting-values");

/// Declared type for a registry value.
///
/// The type is determined at config load time from the TOML value so that
/// writers never have to guess from the string form of `value_data`.  Without
/// this, a plain string like `"42"` would be silently written as `REG_DWORD`
/// and a user could not express a numeric-looking `REG_SZ` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryValueType {
    /// 32-bit unsigned integer (`REG_DWORD`).
    Dword,
    /// Null-terminated Unicode string (`REG_SZ`).
    String,
}

/// A Windows registry entry.
#[derive(Debug, Clone)]
pub struct RegistryEntry {
    /// Registry key path (e.g., "HKCU:\Console").
    pub key_path: String,
    /// Value name.
    pub value_name: String,
    /// Value data as a string (decimal for `DWORD`, literal text for strings).
    pub value_data: String,
    /// Declared registry value type.
    pub value_type: RegistryValueType,
    /// File, section, and value name of the declaration.
    pub(crate) origin: Option<String>,
}

/// TOML registry section with path and values.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistrySection {
    path: String,
    values: BTreeMap<String, toml::Value>,
}

/// Load registry settings from registry.toml.
///
/// Each top-level section contains a `path` field (registry key path)
/// and a `values` table with key-value pairs.  The TOML value's type
/// determines the registry value type:
///
/// - TOML integer / boolean → `REG_DWORD`.
/// - TOML string starting with `0x` (parseable as hex) → `REG_DWORD`.
/// - Any other TOML string → `REG_SZ` (use explicit integers for `DWORD`).
///
/// # Categories
///
/// Unlike symlinks, packages, and units, registry entries are **not**
/// category-filtered: every entry in `registry.toml` is returned regardless of
/// the active profile or platform categories. This is intentional — the file is
/// only read on Windows (its sole consumer is the Windows registry task), so a
/// platform tag would be redundant, and there is currently no need to scope
/// individual entries by profile. Callers receive the full set and apply it
/// wholesale.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be parsed.
pub fn load(path: &Path) -> Result<Vec<RegistryEntry>> {
    let config: BTreeMap<String, RegistrySection> = toml_loader::load_optional_config(path)?;

    Ok(config
        .into_iter()
        .flat_map(|(section_name, section)| {
            let key_path = section.path;
            section.values.into_iter().map(move |(name, value)| {
                let (value_data, value_type) = classify_value(&value);
                let origin = Some(format!(
                    "{} [{section_name}.values] {name:?}",
                    path.display()
                ));
                RegistryEntry {
                    key_path: key_path.clone(),
                    value_name: name,
                    value_data,
                    value_type,
                    origin,
                }
            })
        })
        .collect())
}

/// Classify a TOML value into its registry representation.
fn classify_value(value: &toml::Value) -> (String, RegistryValueType) {
    match value {
        toml::Value::Integer(i) => (i.to_string(), RegistryValueType::Dword),
        toml::Value::Boolean(b) => {
            let s = if *b { "1" } else { "0" };
            (s.to_string(), RegistryValueType::Dword)
        }
        toml::Value::String(s) => {
            if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))
                && !hex.is_empty()
                && hex.chars().all(|c| c.is_ascii_hexdigit())
                && u64::from_str_radix(hex, 16).is_ok()
            {
                (s.clone(), RegistryValueType::Dword)
            } else {
                (s.clone(), RegistryValueType::String)
            }
        }
        toml::Value::Float(f) => (f.to_string(), RegistryValueType::String),
        toml::Value::Datetime(_) | toml::Value::Array(_) | toml::Value::Table(_) => {
            (value.to_string(), RegistryValueType::String)
        }
    }
}

const SUPPORTED_HIVE_PREFIX: &str = r"HKCU:\";

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "registry identity requires the native, infallible UTF-16 uppercase table"
)]
fn uppercase_registry_name(name: &str) -> String {
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlUpcaseUnicodeChar(source: u16) -> u16;
    }

    let uppercase: Vec<_> = name
        .encode_utf16()
        .map(|unit| {
            // SAFETY: This takes and returns a single code unit, with no pointers
            // or preconditions. Unlike Unicode casing, it matches registry names.
            unsafe { RtlUpcaseUnicodeChar(unit) }
        })
        .collect();
    // Native casing leaves surrogate code units unchanged, preserving valid UTF-16.
    String::from_utf16_lossy(&uppercase)
}

#[cfg(not(windows))]
fn uppercase_registry_name(name: &str) -> String {
    // Registry entries are inactive off Windows; retain portable ASCII validation.
    name.to_ascii_uppercase()
}

fn has_supported_hive(key_path: &str) -> bool {
    key_path
        .get(..SUPPORTED_HIVE_PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(SUPPORTED_HIVE_PREFIX))
}

/// Parse a DWORD's unsigned, signed, or hexadecimal representation for comparison.
pub(crate) fn parse_dword_for_compare(value: &str) -> Option<u32> {
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        return u64::from_str_radix(hex, 16)
            .ok()
            .and_then(|n| u32::try_from(n).ok());
    }
    if let Ok(unsigned) = value.parse::<u32>() {
        return Some(unsigned);
    }
    if let Ok(signed) = value.parse::<i32>() {
        return Some(u32::from_ne_bytes(signed.to_ne_bytes()));
    }
    None
}

/// Find contradictory values for case-insensitive registry target identities.
pub(crate) fn validate_conflicts(entries: &[RegistryEntry]) -> Vec<Diagnostic> {
    Validator::new(REGISTRY_TOML)
        .check_conflicts(
            entries,
            REGISTRY_CONFLICTING_VALUES,
            |entry| {
                let path = entry
                    .key_path
                    .split('\\')
                    .filter(|component| !component.is_empty())
                    .collect::<Vec<_>>()
                    .join("\\");
                format!(
                    "{:?}",
                    (
                        uppercase_registry_name(&path),
                        uppercase_registry_name(&entry.value_name)
                    )
                )
            },
            |entry| {
                let data = match entry.value_type {
                    RegistryValueType::Dword => parse_dword_for_compare(&entry.value_data)
                        .map_or_else(|| entry.value_data.clone(), |value| value.to_string()),
                    RegistryValueType::String => entry.value_data.clone(),
                };
                (entry.value_type, data)
            },
            |entry| entry.origin.as_deref(),
        )
        .finish()
}

/// Validate registry entries and return any warnings.
#[must_use]
pub fn validate(
    entries: &[RegistryEntry],
    platform: crate::infra::platform::Platform,
) -> Vec<Diagnostic> {
    use crate::infra::config::validation::{check, check_error};

    let mut diagnostics = Validator::new(REGISTRY_TOML)
        .warn_if(
            !entries.is_empty() && !platform.has_registry(),
            REGISTRY_PLATFORM_UNSUPPORTED,
            "registry entries",
            "registry entries defined but platform does not support the Windows registry",
        )
        .check_each(entries, |e| &e.value_name, |e| {
            [
                check(e.key_path.trim().is_empty(), REGISTRY_EMPTY_KEY_PATH, "registry key path is empty"),
                check(e.value_name.trim().is_empty(), REGISTRY_EMPTY_VALUE_NAME, "registry value name is empty"),
                check_error(
                    !has_supported_hive(&e.key_path),
                    REGISTRY_UNSUPPORTED_HIVE,
                    r"registry key path must start with HKCU:\; other registry hives are not supported",
                ),
            ]
        })
        .finish();
    diagnostics.extend(validate_conflicts(entries));
    diagnostics
}

/// TOML filename that backs this config section.
pub(crate) const REGISTRY_TOML: &str = "registry.toml";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::config::test_helpers::assert_load_unfiltered_rejects;
    use crate::infra::config::test_load_missing_unfiltered_returns_empty;

    #[test]
    fn unknown_key_in_registry_section_is_rejected() {
        assert_load_unfiltered_rejects(
            load,
            "[console]\npath = \"HKCU:\\\\Console\"\npaths = \"typo\"\n[console.values]\nFontSize = 14\n",
            "paths",
        );
    }

    #[test]
    fn load_registry_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.toml");
        std::fs::write(
            &path,
            "[console]\npath = \"HKCU:\\\\Console\"\n[console.values]\nFontSize = 14\nCursorSize = 100\n",
        )
        .unwrap();

        let entries = load(&path).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|e| e.key_path == "HKCU:\\Console"));
        let font_size = entries
            .iter()
            .find(|e| e.value_name == "FontSize")
            .expect("FontSize entry");
        assert_eq!(font_size.value_data, "14");
        let cursor_size = entries
            .iter()
            .find(|e| e.value_name == "CursorSize")
            .expect("CursorSize entry");
        assert_eq!(cursor_size.value_data, "100");
    }

    #[test]
    fn load_multiple_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.toml");
        std::fs::write(
            &path,
            "[console]\npath = \"HKCU:\\\\Console\"\n[console.values]\nFontSize = 14\n\n[explorer]\npath = \"HKCU:\\\\Explorer\"\n[explorer.values]\nShowHidden = 1\n",
        )
        .unwrap();

        let entries = load(&path).unwrap();
        assert_eq!(entries.len(), 2);

        // Check that both entries exist (order is not guaranteed with HashMap)
        let console_entry = entries.iter().find(|e| e.key_path == "HKCU:\\Console");
        let explorer_entry = entries.iter().find(|e| e.key_path == "HKCU:\\Explorer");

        assert!(console_entry.is_some(), "should have console entry");
        assert!(explorer_entry.is_some(), "should have explorer entry");

        let console = console_entry.unwrap();
        assert_eq!(console.value_name, "FontSize");
        assert_eq!(console.value_data, "14");

        let explorer = explorer_entry.unwrap();
        assert_eq!(explorer.value_name, "ShowHidden");
        assert_eq!(explorer.value_data, "1");
    }

    #[test]
    fn load_empty_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.toml");
        std::fs::write(&path, "").unwrap();
        let entries = load(&path).unwrap();
        assert!(entries.is_empty(), "empty file should produce empty list");
    }

    test_load_missing_unfiltered_returns_empty!(load);

    #[test]
    fn conflicting_values_respect_registry_identity_and_native_types() {
        let cases = [
            ("identical", "14", "14", false),
            ("different DWORDs", "14", "15", true),
            ("decimal and hex", "14", "\"0x0E\"", false),
            ("boolean and integer", "true", "1", false),
            ("signed and unsigned", "-1", "4_294_967_295", false),
            ("signed and hex", "-1", "\"0xFFFFFFFF\"", false),
            ("same data different type", "14", "\"14\"", true),
            ("identical strings", "\"text\"", "\"text\"", false),
            ("case-sensitive strings", "\"text\"", "\"Text\"", true),
        ];
        for (name, first, second, conflict) in cases {
            let (_dir, path) = crate::infra::config::test_helpers::write_temp_toml(&format!(
                "[first]\npath = 'HKCU:\\Console'\n[first.values]\nFontSize = {first}\n\
                 [second]\npath = 'hkcu:\\console\\'\n[second.values]\nfontsize = {second}\n"
            ));
            let entries = load(&path).expect("load registry declarations");
            let diagnostics = validate(
                &entries,
                crate::infra::platform::Platform::new(crate::infra::platform::Os::Windows, false),
            );
            assert_eq!(
                diagnostics.len(),
                usize::from(conflict),
                "{name}: {diagnostics:?}"
            );
            if conflict {
                let diagnostic = &diagnostics[0];
                assert_eq!(diagnostic.code, REGISTRY_CONFLICTING_VALUES, "{name}");
                assert_eq!(
                    diagnostic.severity,
                    crate::infra::config::Severity::Error,
                    "{name}"
                );
                assert!(
                    diagnostic.message.contains(&path.display().to_string()),
                    "{name}"
                );
                assert!(
                    diagnostic.message.contains("[first.values] \"FontSize\""),
                    "{name}"
                );
                assert!(
                    diagnostic.message.contains("[second.values] \"fontsize\""),
                    "{name}"
                );
            }
        }
    }

    #[test]
    fn distinct_registry_targets_do_not_conflict() {
        let (_dir, path) = crate::infra::config::test_helpers::write_temp_toml(
            r#"
[first]
path = 'HKCU:\A'
[first.values]
'B\C' = 1
C = 2
'foo"bar' = 4
[second]
path = 'HKCU:\A\B'
[second.values]
C = 3
[third]
path = 'HKCU:\A\"foo'
[third.values]
bar = 5
"#,
        );
        assert!(validate_conflicts(&load(&path).unwrap()).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn unicode_registry_identity_uses_native_case_mapping() {
        let cases = [
            // Contextual Greek lowercasing must not decide native name identity.
            ("\u{039f}\u{03a3}", "\u{03bf}\u{03c3}", true),
            ("\u{03c2}", "\u{03c3}", false),
            // Kelvin sign and sharp S must not be folded into distinct ASCII names.
            ("K", "\u{212a}", false),
            ("\u{00df}", "SS", false),
        ];
        for (first, second, conflict) in cases {
            for in_path in [false, true] {
                let entries: Vec<_> = [(first, "1"), (second, "2")]
                    .into_iter()
                    .map(|(name, value)| RegistryEntry {
                        key_path: if in_path {
                            format!("HKCU:\\{name}")
                        } else {
                            "HKCU:\\Console".to_string()
                        },
                        value_name: if in_path { "Setting" } else { name }.to_string(),
                        value_data: value.to_string(),
                        value_type: RegistryValueType::Dword,
                        origin: None,
                    })
                    .collect();
                assert_eq!(
                    validate_conflicts(&entries).len(),
                    usize::from(conflict),
                    "{first:?} versus {second:?}, in_path={in_path}"
                );
            }
        }
    }

    #[test]
    fn validate_rejects_invalid_hive() {
        use crate::infra::platform::{Os, Platform};

        let entries = vec![RegistryEntry {
            key_path: "INVALID:\\Key".to_string(),
            value_name: "Test".to_string(),
            value_data: "1".to_string(),
            value_type: RegistryValueType::Dword,
            origin: None,
        }];
        let warnings = validate(&entries, Platform::new(Os::Windows, false));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("must start with HKCU"));
    }

    #[test]
    fn validate_detects_empty_key_path() {
        use crate::infra::platform::{Os, Platform};

        let entries = vec![RegistryEntry {
            key_path: "  ".to_string(),
            value_name: "Test".to_string(),
            value_data: "1".to_string(),
            value_type: RegistryValueType::Dword,
            origin: None,
        }];
        let warnings = validate(&entries, Platform::new(Os::Windows, false));
        assert!(
            warnings.iter().any(|w| w.message.contains("path is empty")),
            "should warn about empty key path: {warnings:?}"
        );
    }

    #[test]
    fn validate_detects_empty_value_name() {
        use crate::infra::platform::{Os, Platform};

        let entries = vec![RegistryEntry {
            key_path: "HKCU:\\Console".to_string(),
            value_name: "  ".to_string(),
            value_data: "1".to_string(),
            value_type: RegistryValueType::Dword,
            origin: None,
        }];
        let warnings = validate(&entries, Platform::new(Os::Windows, false));
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("value name is empty")),
            "should warn about empty value name: {warnings:?}"
        );
    }

    #[test]
    fn validate_rejects_non_hkcu_hive() {
        use crate::infra::platform::{Os, Platform};

        let entries = vec![RegistryEntry {
            key_path: "HKLM:\\Software\\Test".to_string(),
            value_name: "Setting".to_string(),
            value_data: "1".to_string(),
            value_type: RegistryValueType::Dword,
            origin: None,
        }];
        let warnings = validate(&entries, Platform::new(Os::Windows, false));
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("other registry hives are not supported")),
            "should reject non-HKCU hives: {warnings:?}"
        );
    }

    #[test]
    fn validate_warns_registry_on_non_windows() {
        use crate::infra::platform::{Os, Platform};

        let entries = vec![RegistryEntry {
            key_path: "HKCU:\\Console".to_string(),
            value_name: "FontSize".to_string(),
            value_data: "14".to_string(),
            value_type: RegistryValueType::Dword,
            origin: None,
        }];
        let warnings = validate(&entries, Platform::new(Os::Linux, false));
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("does not support")),
            "should warn about registry on non-Windows: {warnings:?}"
        );
    }

    #[test]
    fn validate_valid_hkcu_entry_produces_no_warnings() {
        use crate::infra::platform::{Os, Platform};

        let entries = vec![RegistryEntry {
            key_path: "HKCU:\\Console".to_string(),
            value_name: "FontSize".to_string(),
            value_data: "14".to_string(),
            value_type: RegistryValueType::Dword,
            origin: None,
        }];
        let warnings = validate(&entries, Platform::new(Os::Windows, false));
        assert!(
            warnings.is_empty(),
            "valid HKCU entry should produce no warnings: {warnings:?}"
        );
    }

    #[test]
    fn validate_empty_entries_produces_no_warnings() {
        use crate::infra::platform::{Os, Platform};

        let warnings = validate(&[], Platform::new(Os::Windows, false));
        assert!(
            warnings.is_empty(),
            "empty entries should produce no warnings"
        );
    }

    #[test]
    fn validate_case_insensitive_hive_prefix() {
        use crate::infra::platform::{Os, Platform};

        let entries = vec![RegistryEntry {
            key_path: "hkcu:\\Console".to_string(),
            value_name: "FontSize".to_string(),
            value_data: "14".to_string(),
            value_type: RegistryValueType::Dword,
            origin: None,
        }];
        let warnings = validate(&entries, Platform::new(Os::Windows, false));
        assert!(
            warnings.is_empty(),
            "lowercase hive prefix should be accepted: {warnings:?}"
        );
    }

    #[test]
    fn classify_value_covers_types() {
        assert_eq!(
            classify_value(&toml::Value::String("hello".into())),
            ("hello".to_string(), RegistryValueType::String)
        );
        assert_eq!(
            classify_value(&toml::Value::Integer(42)),
            ("42".to_string(), RegistryValueType::Dword)
        );
        assert_eq!(
            classify_value(&toml::Value::Boolean(true)),
            ("1".to_string(), RegistryValueType::Dword)
        );
        assert_eq!(
            classify_value(&toml::Value::Boolean(false)),
            ("0".to_string(), RegistryValueType::Dword)
        );
        assert_eq!(
            classify_value(&toml::Value::String("0x0E".into())),
            ("0x0E".to_string(), RegistryValueType::Dword)
        );
        // Strings that merely look numeric remain REG_SZ — use TOML integers
        // if you want a DWORD.
        assert_eq!(
            classify_value(&toml::Value::String("42".into())),
            ("42".to_string(), RegistryValueType::String)
        );
    }

    #[test]
    fn load_returns_error_on_malformed_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.toml");
        std::fs::write(&path, "[console\npath = \"HKCU:\\\\Test\"").unwrap();
        let result = load(&path);
        assert!(result.is_err(), "malformed TOML should return error");
    }

    #[test]
    fn load_returns_error_on_missing_path_field() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.toml");
        std::fs::write(&path, "[console]\n[console.values]\nKey = \"Value\"\n").unwrap();
        let result = load(&path);
        assert!(result.is_err(), "missing 'path' field should return error");
    }
}
