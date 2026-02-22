use anyhow::{Context as _, Result, bail};
use std::path::Path;

use super::category_matcher::{self, MatchMode};

/// A parsed section from an INI file.
///
/// # Examples
///
/// ```
/// use dotfiles_cli::config::ini::Section;
///
/// let section = Section {
///     categories: vec!["base".to_string()],
///     items: vec!["git".to_string(), "vim".to_string()],
/// };
/// assert_eq!(section.categories, ["base"]);
/// assert_eq!(section.items.len(), 2);
/// ```
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
///
/// # Examples
///
/// ```
/// use dotfiles_cli::config::ini::KvSection;
///
/// let section = KvSection {
///     header: "HKCU:\\Console".to_string(),
///     entries: vec![("FontSize".to_string(), "14".to_string())],
/// };
/// assert_eq!(section.header, "HKCU:\\Console");
/// assert_eq!(section.entries[0].0, "FontSize");
/// ```
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
/// # Examples
///
/// ```
/// use dotfiles_cli::config::ini::parse_sections_from_str;
///
/// let sections = parse_sections_from_str("[base]\ngit\nvim\n").unwrap();
/// assert_eq!(sections.len(), 1);
/// assert_eq!(sections[0].categories, ["base"]);
/// assert_eq!(sections[0].items, ["git", "vim"]);
/// ```
///
/// Multi-category sections use comma-separated tags:
///
/// ```
/// use dotfiles_cli::config::ini::parse_sections_from_str;
///
/// let sections = parse_sections_from_str("[arch,desktop]\npicom\n").unwrap();
/// assert_eq!(sections[0].categories, ["arch", "desktop"]);
/// ```
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
/// # Examples
///
/// ```
/// use dotfiles_cli::config::ini::parse_kv_sections_from_str;
///
/// let sections = parse_kv_sections_from_str(
///     "[HKCU:\\Console]\nFontSize = 14\nCursorSize = 100\n"
/// ).unwrap();
/// assert_eq!(sections[0].header, "HKCU:\\Console");
/// assert_eq!(sections[0].entries[0], ("FontSize".to_string(), "14".to_string()));
/// ```
///
/// Inline comments (` #` or `\t#`) are stripped from values:
///
/// ```
/// use dotfiles_cli::config::ini::parse_kv_sections_from_str;
///
/// let sections = parse_kv_sections_from_str(
///     "[section]\nkey = value # comment\n"
/// ).unwrap();
/// assert_eq!(sections[0].entries[0].1, "value");
/// ```
///
/// # Errors
///
/// Returns an error if:
/// - A key-value pair is malformed (missing `=` or invalid format)
/// - An entry appears outside of a section header
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

/// Filter sections using the specified match mode.
///
/// Returns owned clones of the sections that match the given categories
/// under the chosen logic.
///
/// # Examples
///
/// ```
/// use dotfiles_cli::config::ini::{Section, filter_sections};
/// use dotfiles_cli::config::category_matcher::MatchMode;
///
/// let sections = vec![
///     Section { categories: vec!["arch".into(), "desktop".into()], items: vec!["picom".into()] },
/// ];
/// let active = vec!["arch".into()];
///
/// // All mode requires both "arch" AND "desktop"
/// assert_eq!(filter_sections(&sections, &active, MatchMode::All).len(), 0);
///
/// // Any mode requires just "arch" OR "desktop"
/// assert_eq!(filter_sections(&sections, &active, MatchMode::Any).len(), 1);
/// ```
#[must_use]
pub fn filter_sections(
    sections: &[Section],
    active_categories: &[String],
    mode: MatchMode,
) -> Vec<Section> {
    sections
        .iter()
        .filter(|s| category_matcher::matches(&s.categories, active_categories, mode))
        .cloned()
        .collect()
}

/// Load items from a categorized INI file.
///
/// Convenience wrapper that combines [`parse_sections`], [`filter_sections`],
/// and flattens all matched items into a single `Vec<String>`.
///
/// This eliminates the repeated pattern of:
/// ```text
/// parse_sections → filter_sections → flat_map items
/// ```
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub fn load_flat_items(path: &Path, active_categories: &[String]) -> Result<Vec<String>> {
    let sections = parse_sections(path)?;
    Ok(
        filter_sections(&sections, active_categories, MatchMode::All)
            .into_iter()
            .flat_map(|s| s.items)
            .collect(),
    )
}

/// Load items from a categorized INI file, converting each item via [`From<String>`].
///
/// Convenience wrapper around [`load_flat_items`] for config types whose
/// `load()` function is simply `load_flat_items` + `map`.
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub fn load_flat<T: From<String>>(path: &Path, active_categories: &[String]) -> Result<Vec<T>> {
    Ok(load_flat_items(path, active_categories)?
        .into_iter()
        .map(T::from)
        .collect())
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
///
/// # Examples
///
/// - `"FontSize = 14 # comment"` → `("FontSize", "14")`
/// - `"CursorSize = 100"` → `("CursorSize", "100")`
fn parse_kv_line(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once('=')?;
    Some((
        key.trim().to_string(),
        strip_inline_comment(value.trim()).to_string(),
    ))
}

/// Strip inline comments (`#` preceded by whitespace) from a value.
fn strip_inline_comment(value: &str) -> &str {
    value
        .find(" #")
        .or_else(|| value.find("\t#"))
        .map_or(value, |idx| value[..idx].trim_end())
}

fn read_file(path: &Path) -> Result<String> {
    if !path.exists() {
        return Ok(String::new());
    }
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// OR-exclusion filter: include sections unless ANY of their categories
    /// are in the excluded set.  Only needed by these tests (the production
    /// equivalent lives in `manifest::load`).
    fn filter_sections_or_exclude<'a>(
        sections: &'a [Section],
        excluded_categories: &[String],
    ) -> Vec<&'a Section> {
        sections
            .iter()
            .filter(|s| {
                !category_matcher::matches(&s.categories, excluded_categories, MatchMode::Any)
            })
            .collect()
    }

    #[test]
    fn parse_simple_section() {
        let content = "[base]\nitem1\nitem2\n";
        let sections = parse_sections_from_str(content).expect("test data should parse");
        assert_eq!(sections.len(), 1);
        assert_eq!(
            sections.first().expect("section 0 should exist").categories,
            vec!["base"]
        );
        assert_eq!(
            sections.first().expect("section 0 should exist").items,
            vec!["item1", "item2"]
        );
    }

    #[test]
    fn parse_multiple_sections() {
        let content = "[base]\nitem1\n\n[arch]\nitem2\nitem3\n";
        let sections = parse_sections_from_str(content).expect("test data should parse");
        assert_eq!(sections.len(), 2);
        assert_eq!(
            sections.first().expect("section 0 should exist").items,
            vec!["item1"]
        );
        assert_eq!(
            sections.get(1).expect("section 1 should exist").categories,
            vec!["arch"]
        );
        assert_eq!(
            sections.get(1).expect("section 1 should exist").items,
            vec!["item2", "item3"]
        );
    }

    #[test]
    fn parse_multi_category_section() {
        let content = "[arch,desktop]\nitem1\n";
        let sections = parse_sections_from_str(content).expect("test data should parse");
        assert_eq!(
            sections.first().expect("section 0 should exist").categories,
            vec!["arch", "desktop"]
        );
    }

    #[test]
    fn parse_comments_ignored() {
        let content = "[base]\n# comment\nitem1\n";
        let sections = parse_sections_from_str(content).expect("test data should parse");
        assert_eq!(
            sections.first().expect("section 0 should exist").items,
            vec!["item1"]
        );
    }

    #[test]
    fn parse_empty_lines_ignored() {
        let content = "[base]\n\n\nitem1\n\n";
        let sections = parse_sections_from_str(content).expect("test data should parse");
        assert_eq!(
            sections.first().expect("section 0 should exist").items,
            vec!["item1"]
        );
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
        let sections = parse_kv_sections_from_str(content).expect("test data should parse");
        assert_eq!(sections.len(), 1);
        assert_eq!(
            sections.first().expect("section 0 should exist").header,
            "section"
        );
        assert_eq!(
            sections.first().expect("section 0 should exist").entries,
            vec![
                ("key1".to_string(), "value1".to_string()),
                ("key2".to_string(), "value2".to_string()),
            ]
        );
    }

    #[test]
    fn parse_kv_with_equals_in_value() {
        let content = "[section]\nkey = val=ue\n";
        let sections = parse_kv_sections_from_str(content).expect("test data should parse");
        assert_eq!(
            sections
                .first()
                .expect("section 0 should exist")
                .entries
                .first()
                .expect("entry 0 should exist")
                .1,
            "val=ue"
        );
    }

    #[test]
    fn parse_kv_preserves_header_case() {
        let content = "[HKCU:\\Console]\nFontSize = 14\n";
        let sections = parse_kv_sections_from_str(content).expect("test data should parse");
        assert_eq!(
            sections.first().expect("section 0 should exist").header,
            "HKCU:\\Console"
        );
    }

    #[test]
    fn parse_kv_strips_inline_comments() {
        let content = "[section]\nkey = value # comment\n";
        let sections = parse_kv_sections_from_str(content).expect("test data should parse");
        assert_eq!(
            sections
                .first()
                .expect("section 0 should exist")
                .entries
                .first()
                .expect("entry 0 should exist")
                .1,
            "value"
        );
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
        let filtered = filter_sections(&sections, &active, MatchMode::All);
        assert_eq!(filtered.len(), 2);
        assert_eq!(
            filtered.first().expect("filtered 0 should exist").items,
            vec!["a"]
        );
        assert_eq!(
            filtered.get(1).expect("filtered 1 should exist").items,
            vec!["c"]
        );
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
        assert_eq!(
            filtered.first().expect("filtered 0 should exist").items,
            vec!["a"]
        );
        assert_eq!(
            filtered.get(1).expect("filtered 1 should exist").items,
            vec!["b"]
        );
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
        let sections = parse_sections_from_str(content).expect("test data should parse");
        assert_eq!(
            sections.first().expect("section 0 should exist").categories,
            vec!["base"]
        );
    }

    #[test]
    fn parse_category_whitespace_trimmed() {
        let content = "[ arch , desktop ]\nitem1\n";
        let sections = parse_sections_from_str(content).expect("test data should parse");
        assert_eq!(
            sections.first().expect("section 0 should exist").categories,
            vec!["arch", "desktop"]
        );
    }

    #[test]
    fn empty_file_returns_empty() {
        let sections = parse_sections_from_str("").expect("empty input should parse");
        assert!(
            sections.is_empty(),
            "empty input should produce no sections"
        );
    }

    #[test]
    fn comment_only_file_returns_empty() {
        let sections =
            parse_sections_from_str("# just a comment\n").expect("comment-only input should parse");
        assert!(
            sections.is_empty(),
            "comment-only input should produce no sections"
        );
    }
}
