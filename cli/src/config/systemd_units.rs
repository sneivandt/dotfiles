use anyhow::Result;
use std::path::Path;

use super::ini;

/// A systemd user unit to enable.
#[derive(Debug, Clone)]
pub struct SystemdUnit {
    pub name: String,
}

/// Load systemd units from systemd-units.ini, filtered by active categories.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be parsed.
pub fn load(path: &Path, active_categories: &[String]) -> Result<Vec<SystemdUnit>> {
    Ok(ini::load_filtered_items(path, active_categories)?
        .into_iter()
        .map(|name| SystemdUnit { name })
        .collect())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::test_helpers::write_temp_ini;

    #[test]
    fn load_base_units() {
        let (_dir, path) =
            write_temp_ini("[base]\nclean-home-tmp.timer\n\n[arch,desktop]\ndunst.service\n");
        let units = load(&path, &["base".to_string()]).unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].name, "clean-home-tmp.timer");
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.ini");
        let units = load(&path, &["base".to_string()]).unwrap();
        assert!(units.is_empty(), "missing file should produce empty list");
    }
}
