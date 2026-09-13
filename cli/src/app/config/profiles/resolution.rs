//! Built-in profile category resolution.

use crate::app::config::error::ConfigError;
use crate::infra::config::category_matcher::Category;
use crate::infra::platform::Platform;

/// A resolved profile with its active categories.
#[derive(Debug, Clone)]
pub struct Profile {
    /// The profile name.
    pub name: String,
    /// Categories that are active for this profile.
    pub active_categories: Vec<Category>,
}

/// Resolve a built-in profile by name.
///
/// # Errors
///
/// Returns an error if the profile is unknown.
pub fn resolve(name: &str, platform: Platform) -> Result<Profile, ConfigError> {
    let desktop = match name {
        "base" => false,
        "desktop" => true,
        _ => {
            return Err(ConfigError::InvalidProfile {
                name: name.to_string(),
                available: "base, desktop".to_string(),
            });
        }
    };

    let mut active = vec![Category::Base];
    if desktop {
        active.push(Category::Desktop);
    }

    for category in [
        Category::Linux,
        Category::Windows,
        Category::Arch,
        Category::Wsl,
    ] {
        if !platform.excludes_category(&category) {
            active.push(category);
        }
    }

    active.sort();
    active.dedup();

    Ok(Profile {
        name: name.to_string(),
        active_categories: active,
    })
}
