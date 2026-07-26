//! Profile definition loading.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use serde::Deserialize;

use crate::app::config::error::ConfigError;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProfileDef {
    #[serde(default)]
    pub(super) description: Option<String>,
    #[serde(default)]
    pub(super) include: Vec<String>,
    #[serde(default)]
    pub(super) exclude: Vec<String>,
}

pub(super) fn load_definitions(path: &Path) -> Result<HashMap<String, ProfileDef>, ConfigError> {
    if !path.exists() {
        return Ok(default_definitions());
    }

    let content = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.display().to_string(),
        source,
    })?;

    toml::from_str(&content).map_err(|source| ConfigError::TomlParse {
        path: path.display().to_string(),
        source,
    })
}

pub(super) fn default_definitions() -> HashMap<String, ProfileDef> {
    HashMap::from([
        (
            "base".to_string(),
            ProfileDef {
                description: Some("Core shell environment, no desktop GUI".to_string()),
                include: vec![],
                exclude: vec!["desktop".to_string()],
            },
        ),
        (
            "desktop".to_string(),
            ProfileDef {
                description: Some("Full graphical desktop (Arch + Hyprland/Wayland)".to_string()),
                include: vec!["desktop".to_string()],
                exclude: vec![],
            },
        ),
    ])
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::infra::config::test_helpers::write_temp_toml;

    #[test]
    fn load_parses_include_and_exclude() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
description = "Core"
include = []
exclude = ["desktop"]
"#,
        );
        let definitions = load_definitions(&path).expect("definitions should parse");
        let base = definitions.get("base").expect("base profile");
        assert_eq!(
            base.exclude,
            vec!["desktop".to_string()],
            "exclude list should round-trip from the file"
        );
    }

    #[test]
    fn load_defaults_omitted_lists_to_empty() {
        let (_dir, path) = write_temp_toml("[minimal]\ndescription = \"only a description\"\n");
        let definitions = load_definitions(&path).expect("definitions should parse");
        let minimal = definitions.get("minimal").expect("minimal profile");
        assert!(
            minimal.include.is_empty() && minimal.exclude.is_empty(),
            "omitted lists should default to empty"
        );
    }

    #[test]
    fn misspelled_exclude_key_is_rejected() {
        // Regression: with `#[serde(default)]` on every field and no
        // `deny_unknown_fields`, `excludee` silently produced an empty
        // `exclude` list, so a `base` profile would stop excluding the
        // `desktop` category without any error.
        let (_dir, path) = write_temp_toml(
            r#"[base]
include = []
excludee = ["desktop"]
"#,
        );
        let error = load_definitions(&path).expect_err("a misspelled key must be rejected");
        let message = format!("{error:#}");
        assert!(
            message.contains("excludee"),
            "error should name the unknown key, got: {message}"
        );
    }

    #[test]
    fn missing_file_falls_back_to_builtin_definitions() {
        let dir = tempfile::tempdir().expect("temp dir");
        let definitions = load_definitions(&dir.path().join("absent.toml"))
            .expect("missing file is not an error");
        assert_eq!(
            definitions.len(),
            default_definitions().len(),
            "a missing file should fall back to the built-in definitions"
        );
    }
}
