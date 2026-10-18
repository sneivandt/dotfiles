//! VS Code extension configuration loading.

use crate::infra::config::Diagnostic;
use crate::infra::config::config_section;

crate::infra::config::diagnostic_codes! {
    "vscode";
    VSCODE_EMPTY_ID => "empty-id",
    VSCODE_INVALID_ID_FORMAT => "invalid-id-format",
}

config_section! {
    field: "extensions",
    ty: String,
}

/// Validate VS Code extension entries and return any warnings.
#[must_use]
pub fn validate(extensions: &[String]) -> Vec<Diagnostic> {
    use crate::infra::config::validation::{Validator, check};

    Validator::new(VSCODE_EXTENSIONS_TOML)
        .check_each(
            extensions,
            |id| id,
            |id| {
                [
                    check(
                        id.trim().is_empty(),
                        VSCODE_EMPTY_ID,
                        "extension ID is empty",
                    ),
                    check(
                        !id.split_once('.').is_some_and(|(publisher, name)| {
                            !publisher.is_empty()
                                && !name.is_empty()
                                && !name.contains('.')
                                && !id.chars().any(char::is_whitespace)
                        }),
                        VSCODE_INVALID_ID_FORMAT,
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
        let extensions: Vec<String> = load(&path, &[Category::Base, Category::Desktop]).unwrap();
        assert_eq!(extensions.len(), 2);
        assert_eq!(extensions[0], "github.copilot-chat");
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
        let extensions: Vec<String> = load(&path, &[Category::Base]).unwrap();
        assert_eq!(extensions.len(), 1, "desktop section should not be loaded");
        assert_eq!(extensions[0], "github.copilot");
    }

    test_load_missing_returns_empty!(load);

    #[test]
    fn validate_detects_invalid_format() {
        let extensions = vec!["invalid_no_publisher".to_string()];
        let warnings = validate(&extensions);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].source, VSCODE_EXTENSIONS_TOML);
        assert_eq!(warnings[0].item, "invalid_no_publisher");
        assert_eq!(warnings[0].code.to_string(), "vscode.invalid-id-format");
        assert_eq!(
            warnings[0].message,
            "extension ID should be in format 'publisher.name'"
        );
    }

    #[test]
    fn validate_rejects_incomplete_or_whitespace_extension_ids() {
        for id in [
            ".",
            "publisher.",
            ".extension",
            "publisher.name.extra",
            " publisher.name",
            "publisher.name ",
            "publisher.na me",
            "publisher.\tname",
        ] {
            let warnings = validate(&[id.to_string()]);
            assert_eq!(warnings.len(), 1, "ID: {id:?}");
            assert_eq!(warnings[0].source, VSCODE_EXTENSIONS_TOML);
            assert_eq!(warnings[0].item, id);
            assert_eq!(warnings[0].code, VSCODE_INVALID_ID_FORMAT);
        }
    }

    #[test]
    fn validate_accepts_complete_extension_ids() {
        for id in [
            "ms-vscode.cpptools",
            "github.vscode-pull-request-github",
            "Publisher.Name",
        ] {
            assert!(validate(&[id.to_string()]).is_empty(), "ID: {id}");
        }
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
        let extensions = vec!["  ".to_string()];
        let warnings = validate(&extensions);
        let warning = warnings
            .iter()
            .find(|warning| warning.code.to_string() == "vscode.empty-id")
            .expect("empty ID diagnostic");
        assert_eq!(warning.source, VSCODE_EXTENSIONS_TOML);
        assert_eq!(warning.item, "  ");
        assert_eq!(warning.message, "extension ID is empty");
    }
}
