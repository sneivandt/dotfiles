//! TOML configuration file parsing with category filtering.
use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::path::Path;

use super::category_matcher::{Category, MatchMode};

/// Load and filter TOML config sections by active categories.
///
/// Generic loader that deserializes a TOML file and extracts items from
/// sections matching the active categories. The root TOML config must
/// have sections as top-level keys.
///
/// # Type Parameters
///
/// - `T`: Target type to deserialize items into (must implement `DeserializeOwned`)
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub fn load_config<T: DeserializeOwned>(path: &Path) -> Result<T> {
    if !path.exists() {
        // Return empty config for missing files by deserializing empty TOML
        return toml::from_str("").context("Failed to create empty config");
    }

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;

    toml::from_str(&content)
        .with_context(|| format!("Failed to parse TOML config: {}", path.display()))
}

/// Load a TOML config file where each top-level section contains a single
/// repeated field, and return all items as `(section_name, Vec<T>)` pairs.
///
/// `extract` receives the deserialized section value and returns the `Vec<T>`
/// stored inside it (e.g. `|s: PackageSection| s.packages`).
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub fn load_section_items<S, T>(
    path: &Path,
    extract: impl Fn(S) -> Vec<T>,
) -> Result<Vec<(String, Vec<T>)>>
where
    S: DeserializeOwned,
{
    let config: HashMap<String, S> = load_config(path)?;
    Ok(config.into_iter().map(|(k, v)| (k, extract(v))).collect())
}

/// Load, filter, and map TOML section items in a single step.
///
/// Combines [`load_section_items`] and [`filter_by_categories`], then maps
/// each surviving entry through `map`.  This eliminates the repeated
/// three-step pattern found in most config loaders.
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub fn load_filtered<S, E, T>(
    path: &Path,
    extract: impl Fn(S) -> Vec<E>,
    map: impl Fn(E) -> T,
    active_categories: &[Category],
    mode: MatchMode,
) -> Result<Vec<T>>
where
    S: DeserializeOwned,
{
    let items = load_section_items(path, extract)?;
    let entries = filter_by_categories(items, active_categories, mode);
    Ok(entries.into_iter().map(map).collect())
}

/// Filter items from a TOML table by category matching.
///
/// This is a helper for config types that need category-based filtering.
/// The matcher uses the provided `MatchMode` to determine how categories
/// are combined (AND or OR logic).
#[must_use]
pub fn filter_by_categories<T>(
    items: Vec<(String, Vec<T>)>,
    active_categories: &[Category],
    mode: MatchMode,
) -> Vec<T> {
    use super::category_matcher::matches;

    items
        .into_iter()
        .filter(|(section_name, _)| {
            let categories: Vec<Category> = section_name
                .split('-')
                .map(|s| Category::from_tag(s.trim()))
                .collect();
            matches(&categories, active_categories, mode)
        })
        .flat_map(|(_, items)| items)
        .collect()
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::test_helpers::write_temp_toml;
    use serde::Deserialize;

    // -----------------------------------------------------------------------
    // load_config
    // -----------------------------------------------------------------------

    #[test]
    fn load_config_missing_file_returns_empty_hashmap() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nonexistent.toml");
        let result: HashMap<String, String> = load_config(&path).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn load_config_valid_toml() {
        #[derive(Deserialize)]
        struct Root {
            key: String,
        }
        let (_dir, path) = write_temp_toml("key = \"value\"\n");
        let root: Root = load_config(&path).unwrap();
        assert_eq!(root.key, "value");
    }

    #[test]
    fn load_config_invalid_toml_returns_error() {
        let (_dir, path) = write_temp_toml("not valid {{{{ toml");
        let result: Result<HashMap<String, String>> = load_config(&path);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("Failed to parse TOML config"),
            "error should mention parse failure: {msg}"
        );
    }

    // -----------------------------------------------------------------------
    // load_section_items
    // -----------------------------------------------------------------------

    #[derive(Deserialize)]
    struct Section {
        items: Vec<String>,
    }

    #[test]
    fn load_section_items_extracts_sections() {
        let toml = "\
[base]
items = [\"a\", \"b\"]

[desktop]
items = [\"c\"]
";
        let (_dir, path) = write_temp_toml(toml);
        let sections = load_section_items(&path, |s: Section| s.items).unwrap();
        let total_items: usize = sections.iter().map(|(_, v)| v.len()).sum();
        assert_eq!(total_items, 3);
    }

    #[test]
    fn load_section_items_missing_file_returns_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nonexistent.toml");
        let sections = load_section_items(&path, |s: Section| s.items).unwrap();
        assert!(sections.is_empty());
    }

    // -----------------------------------------------------------------------
    // filter_by_categories
    // -----------------------------------------------------------------------

    #[test]
    fn filter_by_categories_any_mode() {
        let items = vec![
            ("base".to_string(), vec!["a", "b"]),
            ("desktop".to_string(), vec!["c"]),
            ("windows".to_string(), vec!["d"]),
        ];
        let active = vec![Category::Base, Category::Desktop];
        let result = filter_by_categories(items, &active, MatchMode::Any);
        assert_eq!(result.len(), 3, "base + desktop items");
        assert!(result.contains(&"a"));
        assert!(result.contains(&"b"));
        assert!(result.contains(&"c"));
    }

    #[test]
    fn filter_by_categories_all_mode() {
        let items = vec![
            ("arch-desktop".to_string(), vec!["a"]),
            ("arch".to_string(), vec!["b"]),
        ];
        let active = vec![Category::Arch];
        let result = filter_by_categories(items, &active, MatchMode::All);
        // "arch-desktop" requires both arch AND desktop; only arch is active
        assert_eq!(result, vec!["b"]);
    }

    #[test]
    fn filter_by_categories_compound_key_all_mode_both_active() {
        let items = vec![("arch-desktop".to_string(), vec!["x"])];
        let active = vec![Category::Arch, Category::Desktop];
        let result = filter_by_categories(items, &active, MatchMode::All);
        assert_eq!(result, vec!["x"]);
    }

    #[test]
    fn filter_by_categories_no_match_returns_empty() {
        let items = vec![("windows".to_string(), vec!["a", "b"])];
        let active = vec![Category::Linux];
        let result = filter_by_categories(items, &active, MatchMode::Any);
        assert!(result.is_empty());
    }

    #[test]
    fn filter_by_categories_empty_items() {
        let items: Vec<(String, Vec<&str>)> = vec![];
        let active = vec![Category::Base];
        let result = filter_by_categories(items, &active, MatchMode::Any);
        assert!(result.is_empty());
    }
}
