//! Validation for MCP dependencies declared in APM fragments.

use std::path::Path;

use serde_yaml_ng::Value as YamlValue;

use super::path_item;
use crate::infra::config::validation::Validator;

/// Validate `dependencies.mcp` entries declared directly in a fragment.
///
/// APM merges these self-defined servers into `~/.apm/apm.yml`, so a malformed
/// entry silently produces a broken `mcp-config.json` at install time.
pub(super) fn validate_dependencies(
    validator: &mut Validator,
    root: &Path,
    fragment: &Path,
    value: &YamlValue,
) {
    let Some(mcp_deps) = value
        .get("dependencies")
        .and_then(|dependencies| dependencies.get("mcp"))
        .and_then(YamlValue::as_sequence)
    else {
        return;
    };

    for (index, entry) in mcp_deps.iter().enumerate() {
        if entry.as_str().is_some() {
            continue;
        }
        if !entry.is_mapping() {
            validator.warn(
                "apm.mcp-invalid-entry",
                item(root, fragment, index, entry),
                "dependencies.mcp entry is neither a registry string nor a mapping",
            );
            continue;
        }

        if entry
            .get("name")
            .and_then(YamlValue::as_str)
            .is_none_or(|name| name.trim().is_empty())
        {
            validator.warn(
                "apm.mcp-missing-name",
                item(root, fragment, index, entry),
                "dependencies.mcp entry is missing a non-empty 'name'",
            );
        }

        let has_command = entry
            .get("command")
            .and_then(YamlValue::as_str)
            .is_some_and(|command| !command.trim().is_empty());
        let has_url = entry
            .get("url")
            .and_then(YamlValue::as_str)
            .is_some_and(|url| !url.trim().is_empty());
        if !has_command && !has_url {
            validator.warn(
                "apm.mcp-missing-endpoint",
                item(root, fragment, index, entry),
                "dependencies.mcp entry must define a 'command' (stdio) or 'url' (http) field",
            );
        }
    }
}

/// Build a stable warning label, preferring the declared name to the index.
fn item(root: &Path, fragment: &Path, index: usize, entry: &YamlValue) -> String {
    let name = entry
        .get("name")
        .and_then(YamlValue::as_str)
        .filter(|name| !name.trim().is_empty());
    name.map_or_else(
        || format!("{}: mcp[{index}]", path_item(root, fragment)),
        |name| format!("{}: mcp '{name}'", path_item(root, fragment)),
    )
}
