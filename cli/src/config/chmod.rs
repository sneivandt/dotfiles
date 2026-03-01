//! Chmod entry configuration loading.
use anyhow::Result;
use serde::Deserialize;
use std::path::Path;

use super::category_matcher::{Category, MatchMode};
use super::toml_loader;

/// A file permission directive.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // fields used on unix only (tasks/chmod.rs)
pub struct ChmodEntry {
    /// Permission mode (e.g., "600", "755").
    pub mode: String,
    /// Relative path under $HOME.
    pub path: String,
}

/// TOML section containing chmod entries.
#[derive(Debug, Deserialize)]
struct ChmodSection {
    permissions: Vec<ChmodEntry>,
}

/// Load chmod entries from chmod.toml, filtered by active categories.
///
/// # Errors
///
/// Returns an error if the file cannot be parsed.
pub fn load(path: &Path, active_categories: &[Category]) -> Result<Vec<ChmodEntry>> {
    let items = toml_loader::load_section_items(path, |s: ChmodSection| s.permissions)?;
    Ok(toml_loader::filter_by_categories(
        items,
        active_categories,
        MatchMode::All,
    ))
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::category_matcher::Category;
    use crate::config::test_helpers::{assert_load_missing_returns_empty, write_temp_toml};

    #[test]
    fn parse_chmod_entry() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
permissions = [
  { mode = "600", path = "ssh/config" },
  { mode = "755", path = "config/git/ai-pr.sh" },
]
"#,
        );
        let entries = load(&path, &[Category::Base]).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].mode, "600");
        assert_eq!(entries[0].path, "ssh/config");
        assert_eq!(entries[1].mode, "755");
    }

    #[test]
    fn load_missing_file_returns_empty() {
        assert_load_missing_returns_empty(load);
    }
}
