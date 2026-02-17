use anyhow::Result;
use std::path::Path;

use super::ini;

/// A symlink to create: source (in symlinks/) → target (in $HOME).
#[derive(Debug, Clone)]
pub struct Symlink {
    /// Relative path under symlinks/ directory.
    pub source: String,
}

/// Load symlinks from symlinks.ini, filtered by active categories (AND logic).
pub fn load(path: &Path, active_categories: &[String]) -> Result<Vec<Symlink>> {
    Ok(ini::load_filtered_items(path, active_categories)?
        .into_iter()
        .map(|source| Symlink { source })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::test_helpers::write_temp_ini;

    #[test]
    fn load_base_symlinks() {
        let (_dir, path) =
            write_temp_ini("[base]\nbashrc\nconfig/git/config\n\n[desktop]\nconfig/i3\n");
        let symlinks = load(&path, &["base".to_string()]).unwrap();
        assert_eq!(symlinks.len(), 2);
        assert_eq!(symlinks[0].source, "bashrc");
        assert_eq!(symlinks[1].source, "config/git/config");
    }

    #[test]
    fn load_multi_category() {
        let (_dir, path) = write_temp_ini("[base]\nbashrc\n\n[arch,desktop]\nconfig/i3\n");
        let symlinks = load(
            &path,
            &[
                "base".to_string(),
                "arch".to_string(),
                "desktop".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(symlinks.len(), 2);
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.ini");
        let symlinks = load(&path, &["base".to_string()]).unwrap();
        assert!(
            symlinks.is_empty(),
            "missing file should produce empty list"
        );
    }
}
