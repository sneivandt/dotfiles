//! VS Code extension configuration loading.
use anyhow::Result;
use serde::Deserialize;
use std::path::Path;

use super::category_matcher::MatchMode;
use super::toml_loader;

/// A VS Code extension to install.
#[derive(Debug, Clone)]
pub struct VsCodeExtension {
    /// Extension identifier in `publisher.name` format (e.g., `"github.copilot-chat"`).
    pub id: String,
}

/// TOML section containing VS Code extensions.
#[derive(Debug, Deserialize)]
struct ExtensionSection {
    extensions: Vec<String>,
}

/// Load VS Code extensions from vscode-extensions.toml, filtered by active categories.
///
/// # Errors
///
/// Returns an error if the file cannot be parsed.
pub fn load(path: &Path, active_categories: &[String]) -> Result<Vec<VsCodeExtension>> {
    let items = toml_loader::load_section_items(path, |s: ExtensionSection| s.extensions)?;

    let ids: Vec<String> =
        toml_loader::filter_by_categories(items, active_categories, MatchMode::All);

    Ok(ids.into_iter().map(|id| VsCodeExtension { id }).collect())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::test_helpers::{assert_load_missing_returns_empty, write_temp_toml};

    #[test]
    fn load_desktop_extensions() {
        let (_dir, path) = write_temp_toml(
            r#"[desktop]
extensions = ["github.copilot-chat", "ms-python.python"]
"#,
        );
        let extensions: Vec<VsCodeExtension> =
            load(&path, &["base".to_string(), "desktop".to_string()]).unwrap();
        assert_eq!(extensions.len(), 2);
        assert_eq!(extensions[0].id, "github.copilot-chat");
    }

    #[test]
    fn inactive_category_excluded() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
extensions = ["github.copilot"]

[desktop]
extensions = ["github.copilot-chat"]
"#,
        );
        let extensions: Vec<VsCodeExtension> = load(&path, &["base".to_string()]).unwrap();
        assert_eq!(extensions.len(), 1, "desktop section should not be loaded");
        assert_eq!(extensions[0].id, "github.copilot");
    }

    #[test]
    fn load_missing_file_returns_empty() {
        assert_load_missing_returns_empty(load);
    }
}
