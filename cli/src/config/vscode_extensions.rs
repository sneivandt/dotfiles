//! VS Code extension configuration loading.
use anyhow::Result;
use serde::Deserialize;
use std::path::Path;

use super::ValidationWarning;
use super::helpers::category_matcher::{Category, MatchMode};
use super::helpers::toml_loader;

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
pub fn load(path: &Path, active_categories: &[Category]) -> Result<Vec<VsCodeExtension>> {
    toml_loader::load_filtered(
        path,
        |s: ExtensionSection| s.extensions,
        |id| VsCodeExtension { id },
        active_categories,
        MatchMode::All,
    )
}

/// Validate VS Code extension entries and return any warnings.
#[must_use]
pub fn validate(extensions: &[VsCodeExtension]) -> Vec<ValidationWarning> {
    let mut warnings = Vec::new();

    for extension in extensions {
        if extension.id.trim().is_empty() {
            warnings.push(ValidationWarning::new(
                "vscode-extensions.toml",
                &extension.id,
                "extension ID is empty",
            ));
        }

        if !extension.id.contains('.') {
            warnings.push(ValidationWarning::new(
                "vscode-extensions.toml",
                &extension.id,
                "extension ID should be in format 'publisher.name'",
            ));
        }
    }

    warnings
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
            load(&path, &[Category::Base, Category::Desktop]).unwrap();
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
        let extensions: Vec<VsCodeExtension> = load(&path, &[Category::Base]).unwrap();
        assert_eq!(extensions.len(), 1, "desktop section should not be loaded");
        assert_eq!(extensions[0].id, "github.copilot");
    }

    #[test]
    fn load_missing_file_returns_empty() {
        assert_load_missing_returns_empty(load);
    }

    #[test]
    fn validate_detects_invalid_format() {
        let extensions = vec![VsCodeExtension {
            id: "invalid_no_publisher".to_string(),
        }];
        let warnings = validate(&extensions);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("publisher.name"));
    }
}
