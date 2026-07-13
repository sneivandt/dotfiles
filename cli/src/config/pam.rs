//! PAM service configuration loading.

use serde::Deserialize;

use super::Diagnostic;
use super::config_section;

/// A PAM service file to manage under `/etc/pam.d`.
#[derive(Debug, Clone, Deserialize)]
pub struct PamService {
    /// PAM service name, e.g. `"hyprlock"`.
    pub name: String,
    /// Exact desired file content.
    pub content: String,
}

config_section! {
    field: "services",
    ty: PamService,
}

/// Validate PAM service entries and return any warnings.
#[must_use]
pub fn validate(services: &[PamService], platform: crate::platform::Platform) -> Vec<Diagnostic> {
    use super::helpers::validation::{Validator, check, check_error};

    Validator::new(super::PAM_TOML)
        .warn_if(
            !services.is_empty() && !platform.is_linux(),
            "pam.platform-unsupported",
            "PAM services",
            "PAM services defined but platform does not support PAM",
        )
        .check_each(
            services,
            |service| &service.name,
            |service| {
                [
                    check(
                        service.name.trim().is_empty(),
                        "pam.empty-name",
                        "service name is empty",
                    ),
                    check_error(
                        service.name.chars().any(|c| matches!(c, '/' | '\\' | '\0')),
                        "pam.name-contains-separator",
                        "service name must be a file name, not a path",
                    ),
                    check_error(
                        matches!(service.name.as_str(), "." | ".."),
                        "pam.name-is-dot",
                        "service name cannot be '.' or '..'",
                    ),
                    check(
                        service.content.is_empty(),
                        "pam.empty-content",
                        "service content must not be empty",
                    ),
                    check(
                        !service.content.ends_with('\n'),
                        "pam.content-missing-newline",
                        "service content should end with a newline",
                    ),
                ]
            },
        )
        .finish()
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::config::category_matcher::Category;
    use crate::config::test_helpers::write_temp_toml;
    use crate::config::test_load_missing_returns_empty;
    use crate::platform::{Os, Platform};

    #[test]
    fn load_base_services() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
services = [{ name = "hyprlock", content = "auth include login\n" }]
"#,
        );

        let services = load(&path, &[Category::Base]).unwrap();

        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "hyprlock");
        assert_eq!(services[0].content, "auth include login\n");
    }

    test_load_missing_returns_empty!(load);

    #[test]
    fn validate_detects_path_like_service_name() {
        let services = vec![PamService {
            name: "../hyprlock".to_string(),
            content: "auth include login\n".to_string(),
        }];

        let warnings = validate(&services, Platform::new(Os::Linux, false));

        assert!(
            warnings
                .iter()
                .any(|warning| warning.message.contains("not a path")),
            "should warn about path-like service names: {warnings:?}"
        );
    }

    #[test]
    fn validate_detects_missing_trailing_newline() {
        let services = vec![PamService {
            name: "hyprlock".to_string(),
            content: "auth include login".to_string(),
        }];

        let warnings = validate(&services, Platform::new(Os::Linux, false));

        assert!(
            warnings
                .iter()
                .any(|warning| warning.message.contains("newline")),
            "should warn about missing trailing newline: {warnings:?}"
        );
    }
}
