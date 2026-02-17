use anyhow::{Result, bail};
use std::path::Path;

/// A parsed section from an INI file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Category tags for this section (e.g., ["arch", "desktop"]).
    pub categories: Vec<String>,
    /// Items (non-empty, non-comment lines) in this section.
    pub items: Vec<String>,
}

/// A key-value entry within a section (for registry.ini style configs).
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KvSection {
    pub categories: Vec<String>,
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
pub fn parse_sections(path: &Path) -> Result<Vec<Section>> {
    let content = read_file(path)?;
    parse_sections_from_str(&content)
}

/// Parse INI content from a string (for testing).
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

/// Parse an INI file into sections with key-value entries.
///
/// Format:
/// ```ini
/// [category]
/// key = value
/// ```
#[allow(dead_code)]
pub fn parse_kv_sections(path: &Path) -> Result<Vec<KvSection>> {
    let content = read_file(path)?;
    parse_kv_sections_from_str(&content)
}

/// Parse key-value INI content from a string.
#[allow(dead_code)]
pub fn parse_kv_sections_from_str(content: &str) -> Result<Vec<KvSection>> {
    let mut sections = Vec::new();
    let mut current: Option<KvSection> = None;

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(header) = parse_section_header(trimmed) {
            if let Some(section) = current.take() {
                sections.push(section);
            }
            current = Some(KvSection {
                categories: header,
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

/// Filter key-value sections by active categories using AND logic.
#[allow(dead_code)]
#[must_use] 
pub fn filter_kv_sections_and(
    sections: &[KvSection],
    active_categories: &[String],
) -> Vec<KvSection> {
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
#[allow(dead_code)]
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

/// Parse a `[header,tags]` line into category tags.
fn parse_section_header(line: &str) -> Option<Vec<String>> {
    let inner = line.trim().strip_prefix('[')?.strip_suffix(']')?;
    let categories = inner
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    Some(categories)
}

/// Parse a `key = value` line.
fn parse_kv_line(line: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = line.splitn(2, '=').collect();
    if parts.len() == 2 {
        Some((parts[0].trim().to_string(), parts[1].trim().to_string()))
    } else {
        None
    }
}

fn read_file(path: &Path) -> Result<String> {
    if !path.exists() {
        return Ok(String::new());
    }
    std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))
}

/// Load a flat list of items from an INI file, filtered by active categories (AND logic).
/// This is a convenience for config files where each item is a single string
/// (e.g., fonts, units, vscode extensions, copilot skills, symlinks).
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
        assert!(parse_sections_from_str(content).is_err());
    }

    #[test]
    fn parse_kv_simple() {
        let content = "[base]\nkey1 = value1\nkey2 = value2\n";
        let sections = parse_kv_sections_from_str(content).unwrap();
        assert_eq!(sections.len(), 1);
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
        let content = "[base]\nkey = val=ue\n";
        let sections = parse_kv_sections_from_str(content).unwrap();
        assert_eq!(sections[0].entries[0].1, "val=ue");
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
        assert!(sections.is_empty());
    }

    #[test]
    fn comment_only_file_returns_empty() {
        let sections = parse_sections_from_str("# just a comment\n").unwrap();
        assert!(sections.is_empty());
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
        assert!(items.is_empty());
    }
}
