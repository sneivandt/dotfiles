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
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub(crate) fn load_section<S: ConfigSection>(
    path: &Path,
    active_categories: &[Category],
) -> Result<Vec<S::Item>> {
    let sections = load_section_items(path, S::extract)?;
    Ok(filter_by_categories(sections, active_categories)
        .into_iter()
        .map(S::map)
        .collect())
}

/// Load every item from a TOML config using a [`ConfigSection`]
/// implementation, without category filtering.
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub(crate) fn load_section_unfiltered<S: ConfigSection>(path: &Path) -> Result<Vec<S::Item>> {
    Ok(load_section_items(path, S::extract)?
        .into_iter()
        .flat_map(|(_, entries)| entries)
        .map(S::map)
        .collect())
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
mod tests {
    use super::*;
    use crate::infra::config::test_helpers::write_temp_toml;
    use serde::Deserialize;

    #[test]
    fn missing_files_respect_required_and_optional_boundaries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nonexistent.toml");
        let result: BTreeMap<String, String> = load_optional_config(&path).unwrap();
        assert!(result.is_empty());
        assert!(
            load_section_items(&path, |s: Section| s.items)
                .unwrap()
                .is_empty()
        );
        let required_error = load_required_config::<BTreeMap<String, String>>(&path).unwrap_err();
        assert_eq!(
            required_error.to_string(),
            format!("Failed to read config file: {}", path.display())
        );
        let optional_error = load_optional_config::<Section>(&path).err().unwrap();
        assert_eq!(
            optional_error.to_string(),
            format!("Failed to create empty config: {}", path.display())
        );
    }

    #[test]
    fn load_config_valid_toml() {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Root {
            key: String,
        }
        let (_dir, path) = write_temp_toml("key = \"value\"\n");
        let root: Root = load_required_config(&path).unwrap();
        assert_eq!(root.key, "value");
    }

    #[test]
    fn loaders_preserve_parse_and_read_errors() {
        for load in [
            load_optional_config::<BTreeMap<String, Section>>,
            load_required_config::<BTreeMap<String, Section>>,
        ] {
            for content in [
                "{{invalid toml",
                "[base]\nitems = 42",
                "[base]\nitems = \"not-an-array\"",
                "[base]\nitems = []\nunknown = true",
            ] {
                let (_dir, path) = write_temp_toml(content);
                let error = load(&path).err().expect("invalid TOML must fail");
                assert_eq!(
                    error.to_string(),
                    format!("Failed to parse TOML config: {}", path.display())
                );
                assert!(error.chain().count() > 1, "preserve the parser error");
                assert!(load_section_items(&path, |s: Section| s.items).is_err());
            }
            let dir = tempfile::tempdir().unwrap();
            let error = load(dir.path())
                .err()
                .expect("reading a directory must fail");
            assert_eq!(
                error.to_string(),
                format!("Failed to read config file: {}", dir.path().display())
            );
        }
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
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
        assert_eq!(
            sections,
            vec![
                ("base".to_string(), vec!["a".to_string(), "b".to_string()]),
                ("desktop".to_string(), vec!["c".to_string()])
            ]
        );
    }

    #[test]
    fn category_filter_preserves_order_and_requires_all_tags() {
        let items = vec![
            ("base".to_string(), vec!["a", "b"]),
            ("arch-desktop".to_string(), vec!["c"]),
            ("arch".to_string(), vec!["d"]),
        ];
        for (active, expected) in [
            (vec![Category::Base], vec!["a", "b"]),
            (vec![Category::Arch], vec!["d"]),
            (vec![Category::Arch, Category::Desktop], vec!["c", "d"]),
            (vec![Category::Linux], vec![]),
            (vec![], vec![]),
        ] {
            assert_eq!(
                filter_by_categories(items.clone(), &active),
                expected,
                "{active:?}"
            );
        }
        assert!(filter_by_categories::<String>(vec![], &[Category::Base]).is_empty());
    }
}
