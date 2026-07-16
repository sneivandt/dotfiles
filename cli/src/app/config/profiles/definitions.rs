//! Profile definition loading.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use serde::Deserialize;

use crate::app::config::error::ConfigError;

#[derive(Debug, Clone, Deserialize)]
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
