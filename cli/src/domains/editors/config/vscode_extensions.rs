//! VS Code extension configuration loading.

use crate::infra::config::Diagnostic;
use crate::infra::config::config_section;

/// A VS Code extension to install.
#[derive(Debug, Clone)]
pub struct VsCodeExtension {
    /// Extension identifier in `publisher.name` format (e.g., `"github.copilot-chat"`).
    pub id: String,
}

config_section! {
    field: "extensions",
    entry: String,
    item: VsCodeExtension,
    map: |id| VsCodeExtension { id },
}

/// Validate VS Code extension entries and return any warnings.
#[must_use]
pub fn validate(extensions: &[VsCodeExtension]) -> Vec<Diagnostic> {
    use crate::infra::config::validation::{Validator, check};

    Validator::new(VSCODE_EXTENSIONS_TOML)
        .check_each(
            extensions,
            |ext| &ext.id,
            |ext| {
                [
                    check(
                        ext.id.trim().is_empty(),
                        "vscode.empty-id",
                        "extension ID is empty",
                    ),
                    check(
                        !ext.id.contains('.'),
                        "vscode.invalid-id-format",
                        "extension ID should be in format 'publisher.name'",
                    ),
                ]
            },
        )
        .finish()
}

/// TOML filename that backs this config section.
pub(crate) const VSCODE_EXTENSIONS_TOML: &str = "vscode-extensions.toml";

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::infra::config::category_matcher::Category;
    use crate::infra::config::test_helpers::write_temp_toml;
    use crate::infra::config::test_load_missing_returns_empty;

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

    test_load_missing_returns_empty!(load);

    #[test]
    fn validate_detects_invalid_format() {
        let extensions = vec![VsCodeExtension {
            id: "invalid_no_publisher".to_string(),
        }];
        let warnings = validate(&extensions);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("publisher.name"));
    }

    #[test]
    fn load_returns_error_on_malformed_toml() {
        let (_dir, path) = write_temp_toml("[base\nextensions = [\"ext\"");
        let result = load(&path, &[Category::Base]);
        assert!(result.is_err(), "malformed TOML should return error");
    }

    #[test]
    fn load_returns_error_on_type_mismatch() {
        let (_dir, path) = write_temp_toml("[base]\nextensions = 42\n");
        let result = load(&path, &[Category::Base]);
        assert!(
            result.is_err(),
            "integer instead of array should return error"
        );
    }

    #[test]
    fn validate_detects_empty_extension_id() {
        let extensions = vec![VsCodeExtension {
            id: "  ".to_string(),
        }];
        let warnings = validate(&extensions);
        assert!(
            warnings.iter().any(|w| w.message.contains("empty")),
            "should warn about empty extension ID: {warnings:?}"
        );
    }
}
