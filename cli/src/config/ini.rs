use anyhow::{Context as _, Result, bail};
use std::path::Path;

/// A parsed section from an INI file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Category tags for this section (e.g., `["arch", "desktop"]`).
    pub categories: Vec<String>,
    /// Items (non-empty, non-comment lines) in this section.
    pub items: Vec<String>,
}

/// A key-value section where headers are data keys (e.g., registry paths).
///
/// Unlike `Section`, headers preserve original case since they carry
/// semantic meaning (e.g., `[HKCU:\Console]` is a registry path, not a category).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KvSection {
    /// The raw section header (e.g., `"HKCU:\\Console"`).
    pub header: String,
    /// Key-value entries within this section.
    pub entries: Vec<(String, String)>,
}

/// Parse an INI file into sections with list items.
///
/// Format:
/// ```ini
/// [category1,category2]
/// item1
/// item2
/// # comment
/// ```
///
/// # Errors
///
/// Returns an error if the file cannot be read or contains items outside sections.
pub fn parse_sections(path: &Path) -> Result<Vec<Section>> {
    let content = read_file(path)?;
    parse_sections_from_str(&content)
}

/// Parse INI content from a string (for testing).
///
/// # Errors
///
/// Returns an error if the content contains items outside sections.
pub fn parse_sections_from_str(content: &str) -> Result<Vec<Section>> {
    let mut sections = Vec::new();
    let mut current: Option<Section> = None;

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(header) = parse_section_header(trimmed) {
            // Save previous section if non-empty
            if let Some(section) = current.take() {
                sections.push(section);
            }
            current = Some(Section {
                categories: header,
                items: Vec::new(),
            });
        } else if let Some(ref mut section) = current {
            section.items.push(trimmed.to_string());
        } else {
            bail!(
                "item outside of section at line {}: {}",
                line_num + 1,
                trimmed
            );
        }
    }

    if let Some(section) = current {
        sections.push(section);
    }

    Ok(sections)
}

/// Parse an INI file into key-value sections where headers are data keys.
///
/// Headers preserve original case (no lowercasing). Inline comments
/// (` #` or `\t#`) are stripped from values.
///
/// Format:
/// ```ini
/// [HKCU:\Console]
/// WindowSize = 0x00200078  # comment stripped
/// ```
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub fn parse_kv_sections(path: &Path) -> Result<Vec<KvSection>> {
    let content = read_file(path)?;
    parse_kv_sections_from_str(&content)
}

/// Parse key-value INI content from a string.
///
/// # Errors
///
/// Returns an error if the content cannot be parsed.
pub fn parse_kv_sections_from_str(content: &str) -> Result<Vec<KvSection>> {
    let mut sections = Vec::new();
    let mut current: Option<KvSection> = None;

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(header) = parse_raw_header(trimmed) {
            if let Some(section) = current.take() {
                sections.push(section);
            }
            current = Some(KvSection {
                header,
                entries: Vec::new(),
            });
        } else if let Some(ref mut section) = current {
            if let Some((key, value)) = parse_kv_line(trimmed) {
                section.entries.push((key, value));
            } else {
                bail!(
                    "invalid key-value pair at line {}: {}",
                    line_num + 1,
                    trimmed
                );
            }
        } else {
            bail!(
                "entry outside of section at line {}: {}",
                line_num + 1,
                trimmed
            );
        }
    }

    if let Some(section) = current {
        sections.push(section);
    }

    Ok(sections)
}

/// Filter sections by active categories using AND logic:
/// A section is included only if ALL of its categories are in the active set.
#[must_use]
pub fn filter_sections_and(sections: &[Section], active_categories: &[String]) -> Vec<Section> {
    sections
        .iter()
        .filter(|s| {
            s.categories
                .iter()
                .all(|cat| active_categories.contains(cat))
        })
        .cloned()
        .collect()
}

/// Filter sections by excluded categories using OR logic (for manifest):
/// A section is excluded if ANY of its categories are in the excluded set.
#[cfg(test)]
#[must_use]
pub fn filter_sections_or_exclude(
    sections: &[Section],
    excluded_categories: &[String],
) -> Vec<Section> {
    sections
        .iter()
        .filter(|s| {
            !s.categories
                .iter()
                .any(|cat| excluded_categories.contains(cat))
        })
        .cloned()
        .collect()
}

/// Parse a `[header,tags]` line into lowercased category tags.
fn parse_section_header(line: &str) -> Option<Vec<String>> {
    let inner = line.trim().strip_prefix('[')?.strip_suffix(']')?;
    let categories = inner
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    Some(categories)
}

/// Parse a `[header]` line preserving original case (for KV sections).
fn parse_raw_header(line: &str) -> Option<String> {
    let inner = line.trim().strip_prefix('[')?.strip_suffix(']')?;
    let trimmed = inner.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

/// Parse a `key = value` line, stripping inline comments from the value.
fn parse_kv_line(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once('=')?;
    Some((key.trim().to_string(), strip_inline_comment(value.trim())))
}

/// Strip inline comments (`#` preceded by whitespace) from a value.
fn strip_inline_comment(value: &str) -> String {
    value
        .find(" #")
        .or_else(|| value.find("\t#"))
        .map_or_else(|| value.to_string(), |idx| value[..idx].trim().to_string())
}

fn read_file(path: &Path) -> Result<String> {
    if !path.exists() {
        return Ok(String::new());
    }
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

/// Load a flat list of items from an INI file, filtered by active categories (AND logic).
///
/// This is a convenience for config files where each item is a single string
/// (e.g., fonts, systemd-units, vscode extensions, copilot skills, symlinks).
///
/// # Errors
///
/// Returns an error if the file exists but cannot be parsed.
pub fn load_filtered_items(path: &Path, active_categories: &[String]) -> Result<Vec<String>> {
    let sections = parse_sections(path)?;
    let filtered = filter_sections_and(&sections, active_categories);
    Ok(filtered
        .iter()
        .flat_map(|s| s.items.iter().cloned())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_section() {
        let content = "[base]\nitem1\nitem2\n";
        let sections = parse_sections_from_str(content).unwrap();
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].categories, vec!["base"]);
        assert_eq!(sections[0].items, vec!["item1", "item2"]);
    }

    #[test]
    fn parse_multiple_sections() {
        let content = "[base]\nitem1\n\n[arch]\nitem2\nitem3\n";
        let sections = parse_sections_from_str(content).unwrap();
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].items, vec!["item1"]);
        assert_eq!(sections[1].categories, vec!["arch"]);
        assert_eq!(sections[1].items, vec!["item2", "item3"]);
    }

    #[test]
    fn parse_multi_category_section() {
        let content = "[arch,desktop]\nitem1\n";
        let sections = parse_sections_from_str(content).unwrap();
        assert_eq!(sections[0].categories, vec!["arch", "desktop"]);
    }

    #[test]
    fn parse_comments_ignored() {
        let content = "[base]\n# comment\nitem1\n";
        let sections = parse_sections_from_str(content).unwrap();
        assert_eq!(sections[0].items, vec!["item1"]);
    }

    #[test]
    fn parse_empty_lines_ignored() {
        let content = "[base]\n\n\nitem1\n\n";
        let sections = parse_sections_from_str(content).unwrap();
        assert_eq!(sections[0].items, vec!["item1"]);
    }

    #[test]
    fn parse_item_outside_section_fails() {
        let content = "orphan_item\n";
        assert!(
            parse_sections_from_str(content).is_err(),
            "items outside a section should produce an error"
        );
    }

    #[test]
    fn parse_kv_simple() {
        let content = "[section]\nkey1 = value1\nkey2 = value2\n";
        let sections = parse_kv_sections_from_str(content).unwrap();
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].header, "section");
        assert_eq!(
            sections[0].entries,
            vec![
                ("key1".to_string(), "value1".to_string()),
                ("key2".to_string(), "value2".to_string()),
            ]
        );
    }

    #[test]
    fn parse_kv_with_equals_in_value() {
        let content = "[section]\nkey = val=ue\n";
        let sections = parse_kv_sections_from_str(content).unwrap();
        assert_eq!(sections[0].entries[0].1, "val=ue");
    }

    #[test]
    fn parse_kv_preserves_header_case() {
        let content = "[HKCU:\\Console]\nFontSize = 14\n";
        let sections = parse_kv_sections_from_str(content).unwrap();
        assert_eq!(sections[0].header, "HKCU:\\Console");
    }

    #[test]
    fn parse_kv_strips_inline_comments() {
        let content = "[section]\nkey = value # comment\n";
        let sections = parse_kv_sections_from_str(content).unwrap();
        assert_eq!(sections[0].entries[0].1, "value");
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

    #[test]
    fn filter_and_logic() {
        let sections = vec![
            Section {
                categories: vec!["base".to_string()],
                items: vec!["a".to_string()],
            },
            Section {
                categories: vec!["arch".to_string(), "desktop".to_string()],
                items: vec!["b".to_string()],
            },
            Section {
                categories: vec!["arch".to_string()],
                items: vec!["c".to_string()],
            },
        ];

        let active = vec!["base".to_string(), "arch".to_string()];
        let filtered = filter_sections_and(&sections, &active);
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].items, vec!["a"]);
        assert_eq!(filtered[1].items, vec!["c"]);
    }

    #[test]
    fn filter_or_exclude_logic() {
        let sections = vec![
            Section {
                categories: vec!["base".to_string()],
                items: vec!["a".to_string()],
            },
            Section {
                categories: vec!["arch".to_string(), "desktop".to_string()],
                items: vec!["b".to_string()],
            },
            Section {
                categories: vec!["windows".to_string()],
                items: vec!["c".to_string()],
            },
        ];

        // Exclude 'windows' → section with 'windows' is excluded
        let excluded = vec!["windows".to_string()];
        let filtered = filter_sections_or_exclude(&sections, &excluded);
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].items, vec!["a"]);
        assert_eq!(filtered[1].items, vec!["b"]);
    }

    #[test]
    fn filter_or_exclude_multi_category() {
        let sections = vec![Section {
            categories: vec!["arch".to_string(), "desktop".to_string()],
            items: vec!["a".to_string()],
        }];

        // Excluding 'arch' should exclude this section (OR logic)
        let excluded = vec!["arch".to_string()];
        let filtered = filter_sections_or_exclude(&sections, &excluded);
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn parse_category_case_insensitive() {
        let content = "[Base]\nitem1\n";
        let sections = parse_sections_from_str(content).unwrap();
        assert_eq!(sections[0].categories, vec!["base"]);
    }

    #[test]
    fn parse_category_whitespace_trimmed() {
        let content = "[ arch , desktop ]\nitem1\n";
        let sections = parse_sections_from_str(content).unwrap();
        assert_eq!(sections[0].categories, vec!["arch", "desktop"]);
    }

    #[test]
    fn empty_file_returns_empty() {
        let sections = parse_sections_from_str("").unwrap();
        assert!(
            sections.is_empty(),
            "empty input should produce no sections"
        );
    }

    #[test]
    fn comment_only_file_returns_empty() {
        let sections = parse_sections_from_str("# just a comment\n").unwrap();
        assert!(
            sections.is_empty(),
            "comment-only input should produce no sections"
        );
    }

    #[test]
    fn load_filtered_items_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.ini");
        std::fs::write(&path, "[base]\nfoo\nbar\n\n[arch]\nbaz\n").unwrap();

        let items = load_filtered_items(&path, &["base".to_string()]).unwrap();
        assert_eq!(items, vec!["foo", "bar"]);

        let items = load_filtered_items(&path, &["base".to_string(), "arch".to_string()]).unwrap();
        assert_eq!(items, vec!["foo", "bar", "baz"]);
    }

    #[test]
    fn load_filtered_items_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let items =
            load_filtered_items(&dir.path().join("nope.ini"), &["base".to_string()]).unwrap();
        assert!(items.is_empty(), "missing file should produce empty list");
    }
}
