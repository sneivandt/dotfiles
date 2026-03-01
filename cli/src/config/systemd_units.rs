//! Systemd unit configuration loading.
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

use super::category_matcher::{Category, MatchMode};
use super::toml_loader;

/// A systemd unit to enable.
#[derive(Debug, Clone)]
pub struct SystemdUnit {
    /// Unit name including extension (e.g., `"clean-home-tmp.timer"`).
    pub name: String,
    /// Systemd scope: `"user"` or `"system"` (default: `"user"`).
    pub scope: String,
}

/// A single entry in a units section — either a plain name string or a
/// structured `{ name, scope }` pair for an explicit scope override.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum UnitEntry {
    /// Plain string: `"dunst.service"` — defaults to user scope.
    Simple(String),
    /// Structured: `{ name = "foo.service", scope = "system" }`.
    WithScope { name: String, scope: String },
}

/// TOML section containing systemd units.
#[derive(Debug, Deserialize)]
struct SystemdSection {
    units: Vec<UnitEntry>,
}

/// Load systemd units from systemd-units.toml, filtered by active categories.
///
/// # Errors
///
/// Returns an error if the file cannot be parsed.
pub fn load(path: &Path, active_categories: &[Category]) -> Result<Vec<SystemdUnit>> {
    let config: HashMap<String, SystemdSection> = toml_loader::load_config(path)?;

    let items: Vec<(String, Vec<UnitEntry>)> =
        config.into_iter().map(|(k, v)| (k, v.units)).collect();

    let entries: Vec<UnitEntry> =
        toml_loader::filter_by_categories(items, active_categories, MatchMode::All);

    Ok(entries
        .into_iter()
        .map(|entry| match entry {
            UnitEntry::Simple(name) => SystemdUnit {
                name,
                scope: "user".to_string(),
            },
            UnitEntry::WithScope { name, scope } => SystemdUnit { name, scope },
        })
        .collect())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::category_matcher::Category;
    use crate::config::test_helpers::{assert_load_missing_returns_empty, write_temp_toml};

    #[test]
    fn load_base_units() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
units = ["clean-home-tmp.timer"]

["arch-desktop"]
units = ["dunst.service"]
"#,
        );
        let units: Vec<SystemdUnit> = load(&path, &[Category::Base]).unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].name, "clean-home-tmp.timer");
        assert_eq!(units[0].scope, "user");
    }

    #[test]
    fn load_plain_string_defaults_to_user_scope() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
units = ["clean-home-tmp.timer"]
"#,
        );
        let units = load(&path, &[Category::Base]).unwrap();
        assert_eq!(units[0].scope, "user");
    }

    #[test]
    fn load_scope_override() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
units = [{ name = "some-daemon.service", scope = "system" }]
"#,
        );
        let units = load(&path, &[Category::Base]).unwrap();
        assert_eq!(units[0].name, "some-daemon.service");
        assert_eq!(units[0].scope, "system");
    }

    #[test]
    fn load_missing_file_returns_empty() {
        assert_load_missing_returns_empty(load);
    }
}
