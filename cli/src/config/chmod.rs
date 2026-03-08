//! Chmod entry configuration loading.
use serde::Deserialize;
use std::path::Path;

use super::ValidationWarning;
use super::config_section;

/// A file permission directive.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // fields used on unix only (tasks/chmod.rs)
pub struct ChmodEntry {
    /// Permission mode (e.g., "600", "755").
    pub mode: String,
    /// Relative path under $HOME.
    pub path: String,
}

config_section!(field: "permissions", ty: ChmodEntry);

/// Validate chmod entries and return any warnings.
#[must_use]
pub fn validate(
    entries: &[ChmodEntry],
    platform: crate::platform::Platform,
) -> Vec<ValidationWarning> {
    use super::helpers::validation::{Validator, check};
    use crate::resources::chmod::OctalMode;

    let mut v = Validator::new("chmod.toml");
    v.warn_if(
        !entries.is_empty() && !platform.supports_chmod(),
        "chmod entries",
        "chmod entries defined but platform does not support chmod",
    );
    v.check_each(
        entries,
        |e| &e.path,
        |e| {
            vec![
                OctalMode::parse(&e.mode).err(),
                check(
                    Path::new(&e.path).is_absolute() || e.path.starts_with('/'),
                    "path should be relative to $HOME directory",
                ),
                check(
                    Path::new(&e.path)
                        .components()
                        .any(|c| c == std::path::Component::ParentDir),
                    "path must not contain '..' components",
                ),
            ]
        },
    )
    .finish()
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

    #[test]
    fn validate_detects_invalid_mode() {
        use crate::platform::{Os, Platform};

        let entries = vec![ChmodEntry {
            mode: "999".to_string(),
            path: ".ssh/config".to_string(),
        }];
        let warnings = validate(&entries, Platform::new(Os::Linux, false));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("invalid octal digit"));
    }

    #[test]
    fn validate_detects_invalid_mode_length() {
        use crate::platform::{Os, Platform};

        let entries = vec![ChmodEntry {
            mode: "12".to_string(),
            path: ".ssh/config".to_string(),
        }];
        let warnings = validate(&entries, Platform::new(Os::Linux, false));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("must be 3 or 4 digits"));
    }

    #[test]
    fn validate_detects_path_traversal() {
        use crate::platform::{Os, Platform};

        let entries = vec![ChmodEntry {
            mode: "600".to_string(),
            path: "../../etc/shadow".to_string(),
        }];
        let warnings = validate(&entries, Platform::new(Os::Linux, false));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("'..'"));
    }
}
