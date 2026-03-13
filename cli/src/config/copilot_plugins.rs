//! GitHub Copilot plugin configuration loading.

use serde::Deserialize;

use super::ValidationWarning;
use super::config_section;

/// A GitHub Copilot plugin to install from a marketplace.
#[derive(Debug, Clone)]
pub struct CopilotPlugin {
    /// Marketplace repository reference used with `gh copilot plugin marketplace add`.
    pub marketplace: String,
    /// Marketplace name used with `gh copilot plugin install <plugin>@<marketplace_name>`.
    pub marketplace_name: String,
    /// Plugin name to install from the marketplace.
    pub plugin: String,
}

/// TOML Copilot plugin entry.
#[derive(Debug, Clone, Deserialize)]
struct CopilotPluginEntry {
    marketplace: String,
    marketplace_name: String,
    plugin: String,
}

config_section! {
    field: "plugins",
    entry: CopilotPluginEntry,
    item: CopilotPlugin,
    map: |entry| CopilotPlugin {
        marketplace: entry.marketplace,
        marketplace_name: entry.marketplace_name,
        plugin: entry.plugin,
    },
}

/// Validate Copilot plugin entries and return any warnings.
#[must_use]
pub fn validate(plugins: &[CopilotPlugin]) -> Vec<ValidationWarning> {
    use super::helpers::validation::{Validator, check};

    Validator::new("copilot-plugins.toml")
        .check_each(
            plugins,
            |plugin| &plugin.plugin,
            |plugin| {
                vec![
                    check(plugin.plugin.trim().is_empty(), "plugin name is empty"),
                    check(
                        plugin.marketplace.trim().is_empty(),
                        "marketplace reference is empty",
                    ),
                    check(
                        plugin.marketplace_name.trim().is_empty(),
                        "marketplace name is empty",
                    ),
                    check(
                        !plugin.marketplace.starts_with("http://")
                            && !plugin.marketplace.starts_with("https://")
                            && !plugin.marketplace.contains('/'),
                        "marketplace should be an owner/repo reference or http(s) URL",
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
    fn load_base_plugins() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
plugins = [
  { marketplace = "dotnet/skills", marketplace_name = "dotnet-agent-skills", plugin = "dotnet-diag" },
  { marketplace = "dotnet/skills", marketplace_name = "dotnet-agent-skills", plugin = "dotnet-msbuild" },
]
"#,
        );
        let skills: Vec<CopilotPlugin> = load(&path, &[Category::Base]).unwrap();
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].marketplace, "dotnet/skills");
        assert_eq!(skills[0].plugin, "dotnet-diag");
    }

    #[test]
    fn inactive_category_excluded() {
        let (_dir, path) = write_temp_toml(
            r#"[base]
plugins = [{ marketplace = "dotnet/skills", marketplace_name = "dotnet-agent-skills", plugin = "dotnet-diag" }]

[desktop]
plugins = [{ marketplace = "dotnet/skills", marketplace_name = "dotnet-agent-skills", plugin = "dotnet-msbuild" }]
"#,
        );
        let skills: Vec<CopilotPlugin> = load(&path, &[Category::Base]).unwrap();
        assert_eq!(skills.len(), 1, "desktop section should not be loaded");
        assert_eq!(skills[0].plugin, "dotnet-diag");
    }

    #[test]
    fn load_missing_file_returns_empty() {
        assert_load_missing_returns_empty(load);
    }

    #[test]
    fn load_returns_error_on_malformed_toml() {
        let (_dir, path) = write_temp_toml("[base\nplugins = [{ plugin = \"dotnet-diag\" }");
        let result = load(&path, &[Category::Base]);
        assert!(result.is_err(), "malformed TOML should return error");
    }

    #[test]
    fn load_returns_error_on_type_mismatch() {
        let (_dir, path) = write_temp_toml("[base]\nplugins = 42\n");
        let result = load(&path, &[Category::Base]);
        assert!(
            result.is_err(),
            "integer instead of array should return error"
        );
    }

    #[test]
    fn validate_detects_empty_plugin_name() {
        let plugins = vec![CopilotPlugin {
            marketplace: "dotnet/skills".to_string(),
            marketplace_name: "dotnet-agent-skills".to_string(),
            plugin: "  ".to_string(),
        }];
        let warnings = validate(&plugins);
        assert!(
            warnings.iter().any(|w| w.message.contains("empty")),
            "should warn about empty plugin name: {warnings:?}"
        );
    }

    #[test]
    fn validate_detects_invalid_marketplace_reference() {
        let plugins = vec![CopilotPlugin {
            marketplace: "not-a-marketplace".to_string(),
            marketplace_name: "dotnet-agent-skills".to_string(),
            plugin: "dotnet-diag".to_string(),
        }];
        let warnings = validate(&plugins);
        assert!(
            warnings.iter().any(|w| w.message.contains("owner/repo")),
            "should warn about invalid marketplace reference: {warnings:?}"
        );
    }
}
