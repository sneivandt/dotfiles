use anyhow::Result;
use std::path::Path;

/// A Windows registry entry.
#[derive(Debug, Clone)]
pub struct RegistryEntry {
    /// Registry key path (e.g., "HKCU:\Console").
    pub key_path: String,
    /// Value name.
    pub value_name: String,
    /// Value data.
    pub value_data: String,
}

/// Load registry settings from registry.ini, filtered by active categories.
///
/// Registry.ini uses key-value format where sections are registry paths
/// and entries may have inline comments (after #).
pub fn load(path: &Path, active_categories: &[String]) -> Result<Vec<RegistryEntry>> {
    let content = if path.exists() {
        std::fs::read_to_string(path)?
    } else {
        return Ok(Vec::new());
    };

    let mut entries = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_categories = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let inner = &trimmed[1..trimmed.len() - 1];

            // Check if this is a registry path (contains : or \) vs category header
            if inner.contains(':') || inner.contains('\\') {
                // This is a registry path, check categories
                current_path = Some(inner.to_string());
                // Registry paths don't have category tags in the current format
                // They're always under the active profile
                current_categories = vec!["base".to_string()];
            } else {
                // Category header
                current_categories = inner.split(',').map(|s| s.trim().to_lowercase()).collect();
                current_path = None;
            }
            continue;
        }

        // Check if current categories match active
        let matches = current_categories
            .iter()
            .all(|c| active_categories.contains(c));
        if !matches {
            continue;
        }

        if let Some(ref path_str) = current_path {
            // Parse key = value, stripping inline comments
            if let Some((key, value)) = trimmed.split_once('=') {
                let value = strip_inline_comment(value.trim());
                entries.push(RegistryEntry {
                    key_path: path_str.clone(),
                    value_name: key.trim().to_string(),
                    value_data: value,
                });
            }
        }
    }

    Ok(entries)
}

/// Strip inline comments (# preceded by whitespace) from a value.
fn strip_inline_comment(value: &str) -> String {
    // Find # preceded by whitespace that isn't inside the value
    if let Some(idx) = value.find(" #") {
        value[..idx].trim().to_string()
    } else if let Some(idx) = value.find("\t#") {
        value[..idx].trim().to_string()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_inline_comment_simple() {
        assert_eq!(strip_inline_comment("value # comment"), "value");
    }

    #[test]
    fn strip_inline_comment_no_comment() {
        assert_eq!(strip_inline_comment("value"), "value");
    }

    #[test]
    fn strip_inline_comment_hash_in_value() {
        // A # without preceding space is part of the value
        assert_eq!(strip_inline_comment("color#FF0000"), "color#FF0000");
    }
}
