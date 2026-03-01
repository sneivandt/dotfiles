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
            let categories: Vec<Category> =
                section_name.split('-').map(Category::from_tag).collect();
            matches(&categories, active_categories, mode)
        })
        .flat_map(|(_, items)| items)
        .collect()
}
