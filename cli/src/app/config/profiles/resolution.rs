//! Profile category resolution.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;

use crate::app::config::error::ConfigError;
use crate::infra::config::category_matcher::Category;
use crate::infra::platform::Platform;

use super::definitions::{ProfileDef, load_definitions};

/// A resolved profile with its active and excluded categories.
#[derive(Debug, Clone)]
pub struct Profile {
    /// The profile name.
    pub name: String,
    /// Categories that are active for this profile.
    pub active_categories: Vec<Category>,
    /// Categories that are excluded for this profile.
    pub excluded_categories: Vec<Category>,
}

/// Resolve a profile by name.
///
/// # Errors
///
/// Returns an error if the profile is unknown or its definitions cannot be loaded.
#[cfg(any(test, feature = "internal-api", doctest))]
pub fn resolve(name: &str, conf_dir: &Path, platform: Platform) -> Result<Profile, ConfigError> {
    let definitions = load_definitions(&conf_dir.join("profiles.toml"))?;
    resolve_with_defs(name, &definitions, platform)
}

pub(super) fn resolve_with_defs(
    name: &str,
    definitions: &HashMap<String, ProfileDef>,
    platform: Platform,
) -> Result<Profile, ConfigError> {
    let mut available_names: Vec<&str> = definitions.keys().map(String::as_str).collect();
    available_names.sort_unstable();
    let available = available_names.join(", ");
    let definition = definitions
        .get(name)
        .ok_or_else(|| ConfigError::InvalidProfile {
            name: name.to_string(),
            available,
        })?;

    let mut active = vec![Category::Base];
    active.extend(definition.include.iter().map(|tag| Category::from_tag(tag)));
    let mut excluded: Vec<Category> = definition
        .exclude
        .iter()
        .map(|tag| Category::from_tag(tag))
        .collect();

    for category in [Category::Linux, Category::Windows, Category::Arch] {
        if platform.excludes_category(&category) {
            if !excluded.contains(&category) {
                excluded.push(category);
            }
        } else {
            active.push(category);
        }
    }

    active.retain(|category| !excluded.contains(category));
    active.sort();
    active.dedup();
    excluded.sort();
    excluded.dedup();

    Ok(Profile {
        name: name.to_string(),
        active_categories: active,
        excluded_categories: excluded,
    })
}
