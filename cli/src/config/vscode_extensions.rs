use anyhow::Result;
use std::path::Path;

use super::ini;

/// A VS Code extension to install.
#[derive(Debug, Clone)]
pub struct VsCodeExtension {
    /// Extension identifier in `publisher.name` format (e.g., `"github.copilot-chat"`).
    pub id: String,
}

/// Load VS Code extensions from vscode-extensions.ini, filtered by active categories.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be parsed.
pub fn load(path: &Path, active_categories: &[String]) -> Result<Vec<VsCodeExtension>> {
    Ok(ini::load_filtered_items(path, active_categories)?
        .into_iter()
        .map(|id| VsCodeExtension { id })
        .collect())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::test_helpers::write_temp_ini;

    #[test]
    fn load_desktop_extensions() {
        let (_dir, path) = write_temp_ini("[desktop]\ngithub.copilot-chat\nms-python.python\n");
        let extensions = load(&path, &["base".to_string(), "desktop".to_string()]).unwrap();
        assert_eq!(extensions.len(), 2);
        assert_eq!(extensions[0].id, "github.copilot-chat");
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.ini");
        let extensions = load(&path, &["base".to_string()]).unwrap();
        assert!(
            extensions.is_empty(),
            "missing file should produce empty list"
        );
    }
}
