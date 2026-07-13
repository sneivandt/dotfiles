//! TOML configuration file parsing with category filtering.
use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::path::Path;

use super::category_matcher::{Category, matches, parse_section_key};

/// Trait for TOML config sections that follow the standard load-filter-map pattern.
///
/// Implementing this trait on a section type replaces the per-module `load()`
/// boilerplate with a single generic call to [`load_section::<S>`].
///
/// # Examples
///
/// ```ignore
/// #[derive(Debug, Deserialize)]
/// struct PluginSection { plugins: Vec<String> }
///
/// impl ConfigSection for PluginSection {
///     type Entry = String;
///     type Item = CopilotPlugin;
///     fn extract(self) -> Vec<String> { self.plugins }
///     fn map(entry: String) -> CopilotPlugin { CopilotPlugin { plugin: entry, marketplace: "owner/repo".into(), marketplace_name: "marketplace".into() } }
/// }
/// ```
pub(crate) trait ConfigSection: DeserializeOwned {
    /// The raw deserialized entry type stored in the TOML section.
    type Entry;
    /// The final domain type produced after mapping each entry.
    type Item;

    /// Extract the entry list from this section (e.g. `self.packages`).
    fn extract(self) -> Vec<Self::Entry>;

    /// Map a single raw entry to the domain type.
    fn map(entry: Self::Entry) -> Self::Item;
}

/// Load a TOML config using a [`ConfigSection`] implementation.
///
/// This replaces the repeated `load_filtered(path, extract, map, cats, mode)`
/// calls across config modules with a single generic call.
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub(crate) fn load_section<S: ConfigSection>(
    path: &Path,
    active_categories: &[Category],
) -> Result<Vec<S::Item>> {
    load_filtered(path, S::extract, S::map, active_categories)
}

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
pub(crate) fn load_optional_config<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Return empty config for missing files by deserializing empty TOML
            return toml::from_str("")
                .with_context(|| format!("Failed to create empty config: {}", path.display()));
        }
        Err(e) => {
            return Err(e)
                .with_context(|| format!("Failed to read config file: {}", path.display()));
        }
    };

    toml::from_str(&content)
        .with_context(|| format!("Failed to parse TOML config: {}", path.display()))
}

/// Load a required TOML config file.
///
/// Unlike [`load_optional_config`], this helper reports a missing file as an
/// error. Use it when the caller owns the policy decision that the file must
/// exist.
///
/// # Errors
///
/// Returns an error if the file is missing, cannot be read, or cannot be parsed.
pub(crate) fn load_required_config<T: DeserializeOwned>(path: &Path) -> Result<T> {
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
pub(crate) fn load_section_items<S, T>(
    path: &Path,
    extract: impl Fn(S) -> Vec<T>,
) -> Result<Vec<(String, Vec<T>)>>
where
    S: DeserializeOwned,
{
    let config: BTreeMap<String, S> = load_optional_config(path)?;
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
pub(crate) fn load_filtered<S, E, T>(
    path: &Path,
    extract: impl Fn(S) -> Vec<E>,
    map: impl Fn(E) -> T,
    active_categories: &[Category],
) -> Result<Vec<T>>
where
    S: DeserializeOwned,
{
    let items = load_section_items(path, extract)?;
    let entries = filter_by_categories(items, active_categories);
    Ok(entries.into_iter().map(map).collect())
}

/// Filter items from a TOML table by category matching.
///
/// A section is included only when all of its category tags are present in
/// `active_categories` (AND logic).
#[must_use]
pub(crate) fn filter_by_categories<T>(
    items: Vec<(String, Vec<T>)>,
    active_categories: &[Category],
) -> Vec<T> {
    items
        .into_iter()
        .filter(|(section_name, _)| matches(&parse_section_key(section_name), active_categories))
        .flat_map(|(_, items)| items)
        .collect()
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::runtime::config_support::test_helpers::write_temp_toml;
    use serde::Deserialize;

    // -----------------------------------------------------------------------
    // load_config
    // -----------------------------------------------------------------------

    #[test]
    fn load_config_missing_file_returns_empty_hashmap() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nonexistent.toml");
        let result: BTreeMap<String, String> = load_optional_config(&path).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn load_config_valid_toml() {
        #[derive(Deserialize)]
        struct Root {
            key: String,
        }
        let (_dir, path) = write_temp_toml("key = \"value\"\n");
        let root: Root = load_required_config(&path).unwrap();
        assert_eq!(root.key, "value");
    }

    #[test]
    fn load_config_invalid_toml_returns_error() {
        let (_dir, path) = write_temp_toml("not valid {{{{ toml");
        let result: Result<BTreeMap<String, String>> = load_optional_config(&path);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("Failed to parse TOML config"),
            "error should mention parse failure: {msg}"
        );
        assert!(
            msg.contains(path.to_str().unwrap_or("")),
            "error should include the file path: {msg}"
        );
    }

    #[test]
    fn load_required_config_missing_file_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nonexistent.toml");
        let result: Result<BTreeMap<String, String>> = load_required_config(&path);
        assert!(
            result.is_err(),
            "required config should reject missing file"
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
    fn filter_by_categories_single_match() {
        let items = vec![
            ("base".to_string(), vec!["a", "b"]),
            ("desktop".to_string(), vec!["c"]),
            ("windows".to_string(), vec!["d"]),
        ];
        let active = vec![Category::Base, Category::Desktop];
        let result = filter_by_categories(items, &active);
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
        let result = filter_by_categories(items, &active);
        // "arch-desktop" requires both arch AND desktop; only arch is active
        assert_eq!(result, vec!["b"]);
    }

    #[test]
    fn filter_by_categories_compound_key_both_active() {
        let items = vec![("arch-desktop".to_string(), vec!["x"])];
        let active = vec![Category::Arch, Category::Desktop];
        let result = filter_by_categories(items, &active);
        assert_eq!(result, vec!["x"]);
    }

    #[test]
    fn filter_by_categories_no_match_returns_empty() {
        let items = vec![("windows".to_string(), vec!["a", "b"])];
        let active = vec![Category::Linux];
        let result = filter_by_categories(items, &active);
        assert!(result.is_empty());
    }

    #[test]
    fn filter_by_categories_empty_items() {
        let items: Vec<(String, Vec<&str>)> = vec![];
        let active = vec![Category::Base];
        let result = filter_by_categories(items, &active);
        assert!(result.is_empty());
    }

    // -----------------------------------------------------------------------
    // load_section_items — error cases
    // -----------------------------------------------------------------------

    #[test]
    fn load_section_items_returns_error_on_malformed_toml() {
        let (_dir, path) = write_temp_toml("{{invalid toml");
        let result = load_section_items(&path, |s: Section| s.items);
        assert!(result.is_err(), "malformed TOML should return error");
    }

    #[test]
    fn load_section_items_returns_error_on_type_mismatch() {
        let (_dir, path) = write_temp_toml("[base]\nitems = 42\n");
        let result = load_section_items(&path, |s: Section| s.items);
        assert!(
            result.is_err(),
            "integer instead of array should return error"
        );
    }

    // -----------------------------------------------------------------------
    // load_config — type mismatch
    // -----------------------------------------------------------------------

    #[test]
    fn load_config_type_mismatch_returns_error() {
        #[derive(Deserialize)]
        struct Root {
            #[allow(dead_code, reason = "used conditionally via cfg")]
            key: Vec<String>,
        }
        let (_dir, path) = write_temp_toml("key = \"not-an-array\"\n");
        let result: Result<Root> = load_optional_config(&path);
        assert!(
            result.is_err(),
            "string-to-array mismatch should return error"
        );
    }
}
