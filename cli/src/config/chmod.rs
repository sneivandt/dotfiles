use anyhow::{Result, bail};
use std::path::Path;

use super::category_matcher::MatchMode;
use super::ini;

/// A file permission directive.
#[derive(Debug, Clone)]
#[allow(dead_code)] // fields used on unix only (tasks/chmod.rs)
pub struct ChmodEntry {
    /// Permission mode (e.g., "600", "755").
    pub mode: String,
    /// Relative path under $HOME.
    pub path: String,
}

/// Load chmod entries from chmod.ini, filtered by active categories.
///
/// Format: `<mode> <relative-path>`
///
/// # Errors
///
/// Returns an error if the file cannot be parsed.
pub fn load(path: &Path, active_categories: &[String]) -> Result<Vec<ChmodEntry>> {
    let sections = ini::parse_sections(path)?;
    let filtered = ini::filter_sections(&sections, active_categories, MatchMode::All);

    let mut entries = Vec::new();
    for section in filtered {
        for item in section.items {
            let Some((mode, path)) = item.split_once(' ') else {
                bail!("invalid chmod entry: {item}");
            };
            entries.push(ChmodEntry {
                mode: mode.to_string(),
                path: path.to_string(),
            });
        }
    }

    Ok(entries)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::test_helpers::{assert_load_missing_returns_empty, write_temp_ini};

    #[test]
    fn parse_chmod_entry() {
        let (_dir, path) = write_temp_ini("[base]\n600 ssh/config\n755 config/git/ai-pr.sh\n");
        let entries = load(&path, &["base".to_string()]).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].mode, "600");
        assert_eq!(entries[0].path, "ssh/config");
        assert_eq!(entries[1].mode, "755");
    }

    #[test]
    fn invalid_entry_fails() {
        let (_dir, path) = write_temp_ini("[base]\ninvalid_no_space\n");
        assert!(load(&path, &["base".to_string()]).is_err());
    }

    #[test]
    fn load_missing_file_returns_empty() {
        assert_load_missing_returns_empty(load);
    }
}
