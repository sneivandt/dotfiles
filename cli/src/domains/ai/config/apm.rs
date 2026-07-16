//! Validation for local Microsoft APM plugin references.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde_json::Value as JsonValue;
use serde_yaml_ng::Value as YamlValue;

use crate::infra::config::Diagnostic;
use crate::infra::config::validation::Validator;

const SOURCE: &str = "apm/config/*.yml";
const LOCAL_PLUGIN_PREFIX: &str = "~/.apm/plugins/";
const LOCAL_PLUGIN_NAME_PREFIX: &str = "dot-";

/// Validate local APM plugin references and return any diagnostics.
#[must_use]
pub(crate) fn validate(root: &Path, overlay: Option<&Path>) -> Vec<Diagnostic> {
    let mut validator = Validator::new(SOURCE);
    validate_root(&mut validator, root);
    if let Some(overlay_root) = overlay {
        validate_root(&mut validator, overlay_root);
    }
    validator.finish()
}

fn validate_root(validator: &mut Validator, root: &Path) {
    let config_dir = root.join("symlinks").join("apm").join("config");
    let fragments = match discover_yaml_files(&config_dir) {
        Ok(fragments) => fragments,
        Err(err) => {
            validator.warn(
                "apm.io-error",
                path_item(root, &config_dir),
                format!("could not inspect APM config fragments: {err}"),
            );
            return;
        }
    };

    for fragment in fragments {
        validate_fragment(validator, root, &fragment);
    }
}

fn discover_yaml_files(config_dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(config_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };

    let mut files = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if !is_yaml_fragment(&path) {
            continue;
        }
        if std::fs::metadata(&path)?.is_file() {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn is_yaml_fragment(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("yml") || ext.eq_ignore_ascii_case("yaml"))
}

fn validate_fragment(validator: &mut Validator, root: &Path, fragment: &Path) {
    let content = match std::fs::read_to_string(fragment) {
        Ok(content) => content,
        Err(err) => {
            validator.warn(
                "apm.io-error",
                path_item(root, fragment),
                format!("could not read APM manifest fragment: {err}"),
            );
            return;
        }
    };
    if content.trim().is_empty() {
        return;
    }

    let value: YamlValue = match serde_yaml_ng::from_str(&content) {
        Ok(value) => value,
        Err(err) => {
            validator.warn(
                "apm.yaml-parse-error",
                path_item(root, fragment),
                format!("could not parse APM manifest fragment: {err}"),
            );
            return;
        }
    };

    if let Some(apm_deps) = value
        .get("dependencies")
        .and_then(|dependencies| dependencies.get("apm"))
        .and_then(YamlValue::as_sequence)
    {
        for dependency in apm_deps {
            if let Some(plugin_name) = local_dot_plugin_name(dependency) {
                validate_local_plugin_ref(validator, root, fragment, &plugin_name);
            }
        }
    }

    validate_mcp_deps(validator, root, fragment, &value);
}

/// Validate `dependencies.mcp` entries declared directly in a fragment.
///
/// APM merges these self-defined servers into `~/.apm/apm.yml`, so a malformed
/// entry (missing `name`, or missing both `command` and `url`) silently
/// produces a broken `mcp-config.json` at install time. Surface it earlier.
fn validate_mcp_deps(validator: &mut Validator, root: &Path, fragment: &Path, value: &YamlValue) {
    let Some(mcp_deps) = value
        .get("dependencies")
        .and_then(|dependencies| dependencies.get("mcp"))
        .and_then(YamlValue::as_sequence)
    else {
        return;
    };

    for (index, entry) in mcp_deps.iter().enumerate() {
        if entry.as_str().is_some() {
            // String shorthand (registry reference) needs no further checks.
            continue;
        }
        if !entry.is_mapping() {
            validator.warn(
                "apm.mcp-invalid-entry",
                mcp_item(root, fragment, index, entry),
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
                mcp_item(root, fragment, index, entry),
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
                mcp_item(root, fragment, index, entry),
                "dependencies.mcp entry must define a 'command' (stdio) or 'url' (http) field",
            );
        }
    }
}

/// Build a stable item label for an `mcp` warning, preferring the entry's
/// declared `name` and falling back to the sequence index.
fn mcp_item(root: &Path, fragment: &Path, index: usize, entry: &YamlValue) -> String {
    let name = entry
        .get("name")
        .and_then(YamlValue::as_str)
        .filter(|name| !name.trim().is_empty());
    name.map_or_else(
        || format!("{}: mcp[{index}]", path_item(root, fragment)),
        |name| format!("{}: mcp '{name}'", path_item(root, fragment)),
    )
}

fn local_dot_plugin_name(dependency: &YamlValue) -> Option<String> {
    let normalized = dependency.as_str()?.replace('\\', "/");
    let plugin_name = normalized
        .strip_prefix(LOCAL_PLUGIN_PREFIX)?
        .trim_end_matches('/')
        .to_owned();
    plugin_name
        .starts_with(LOCAL_PLUGIN_NAME_PREFIX)
        .then_some(plugin_name)
}

fn validate_local_plugin_ref(
    validator: &mut Validator,
    root: &Path,
    fragment: &Path,
    plugin_name: &str,
) {
    let item = format!(
        "{}: {LOCAL_PLUGIN_PREFIX}{plugin_name}",
        path_item(root, fragment)
    );
    let plugin_dir = root
        .join("symlinks")
        .join("apm")
        .join("plugins")
        .join(plugin_name);
    if !plugin_dir.is_dir() {
        validator.warn(
            "apm.plugin-dir-missing",
            item,
            format!(
                "local APM plugin reference has no matching directory: {}",
                plugin_dir.display()
            ),
        );
        return;
    }

    validate_local_plugin_manifest_and_sources(validator, &item, &plugin_dir, plugin_name);
}

fn validate_local_plugin_manifest(
    validator: &mut Validator,
    item: &str,
    plugin_dir: &Path,
    plugin_name: &str,
) {
    let apm_manifest = plugin_dir.join("apm.yml");
    if apm_manifest.is_file() {
        validate_apm_yml(validator, item, &apm_manifest, plugin_name);
        return;
    }

    let plugin_manifest = plugin_dir.join("plugin.json");
    if plugin_manifest.is_file() {
        validate_plugin_json(validator, item, &plugin_manifest, plugin_name);
        return;
    }

    validator.warn(
        "apm.plugin-missing-manifest",
        item,
        format!(
            "local APM plugin is missing apm.yml or plugin.json: {}",
            apm_manifest.display()
        ),
    );
}

fn validate_apm_yml(validator: &mut Validator, item: &str, manifest: &Path, plugin_name: &str) {
    let content = match std::fs::read_to_string(manifest) {
        Ok(content) => content,
        Err(err) => {
            validator.warn(
                "apm.io-error",
                item,
                format!("could not read apm.yml: {err}"),
            );
            return;
        }
    };
    let value: YamlValue = match serde_yaml_ng::from_str(&content) {
        Ok(value) => value,
        Err(err) => {
            validator.warn(
                "apm.yaml-parse-error",
                item,
                format!("could not parse apm.yml: {err}"),
            );
            return;
        }
    };

    validate_manifest_name(
        validator,
        item,
        value.get("name").and_then(YamlValue::as_str),
        "apm.yml",
        plugin_name,
    );
}

fn validate_plugin_json(validator: &mut Validator, item: &str, manifest: &Path, plugin_name: &str) {
    let content = match std::fs::read_to_string(manifest) {
        Ok(content) => content,
        Err(err) => {
            validator.warn(
                "apm.io-error",
                item,
                format!("could not read plugin.json: {err}"),
            );
            return;
        }
    };
    let value: JsonValue = match serde_json::from_str(&content) {
        Ok(value) => value,
        Err(err) => {
            validator.warn(
                "apm.json-parse-error",
                item,
                format!("could not parse plugin.json: {err}"),
            );
            return;
        }
    };

    validate_manifest_name(
        validator,
        item,
        value.get("name").and_then(JsonValue::as_str),
        "plugin.json",
        plugin_name,
    );
}

fn validate_manifest_name(
    validator: &mut Validator,
    item: &str,
    name: Option<&str>,
    manifest_name: &str,
    plugin_name: &str,
) {
    match name {
        Some(name) if name == plugin_name => {}
        Some(name) if name.trim().is_empty() => {
            validator.warn(
                "apm.plugin-empty-name",
                item,
                format!("{manifest_name} name is missing or empty"),
            );
        }
        Some(name) => {
            validator.warn(
                "apm.plugin-name-mismatch",
                item,
                format!(
                    "{manifest_name} name '{name}' does not match local plugin '{plugin_name}'"
                ),
            );
        }
        None => {
            validator.warn(
                "apm.plugin-empty-name",
                item,
                format!("{manifest_name} name is missing or empty"),
            );
        }
    }
}

fn validate_native_skill_sources(validator: &mut Validator, item: &str, plugin_dir: &Path) {
    let skills_dir = plugin_dir.join(".apm").join("skills");
    match std::fs::read_dir(&skills_dir) {
        Ok(entries) => {
            let has_skill = entries.filter_map(Result::ok).any(|entry| {
                let path = entry.path();
                path.is_dir() && path.join("SKILL.md").is_file()
            });
            if !has_skill {
                validator.warn(
                    "apm.skill-missing",
                    item,
                    format!(
                        "native APM plugin has no .apm/skills/*/SKILL.md entries: {}",
                        skills_dir.display()
                    ),
                );
            }
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {
            validator.warn(
                "apm.skill-dir-missing",
                item,
                format!(
                    "native APM plugin is missing .apm/skills directory: {}",
                    skills_dir.display()
                ),
            );
        }
        Err(err) => {
            validator.warn(
                "apm.io-error",
                item,
                format!("could not inspect .apm/skills: {err}"),
            );
        }
    }
}

fn validate_legacy_skill_sources(validator: &mut Validator, item: &str, plugin_dir: &Path) {
    let skills_dir = plugin_dir.join("skills");
    match std::fs::read_dir(&skills_dir) {
        Ok(entries) => {
            let has_skill = entries.filter_map(Result::ok).any(|entry| {
                let path = entry.path();
                path.is_dir() && path.join("SKILL.md").is_file()
            });
            if !has_skill {
                validator.warn(
                    "apm.skill-missing",
                    item,
                    format!(
                        "legacy APM plugin has no skills/*/SKILL.md entries: {}",
                        skills_dir.display()
                    ),
                );
            }
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {
            validator.warn(
                "apm.skill-dir-missing",
                item,
                format!(
                    "legacy APM plugin is missing skills directory: {}",
                    skills_dir.display()
                ),
            );
        }
        Err(err) => {
            validator.warn(
                "apm.io-error",
                item,
                format!("could not inspect skills directory: {err}"),
            );
        }
    }
}

fn validate_plugin_sources(validator: &mut Validator, item: &str, plugin_dir: &Path) {
    if plugin_dir.join("apm.yml").is_file() {
        validate_native_skill_sources(validator, item, plugin_dir);
    } else if plugin_dir.join("plugin.json").is_file() {
        validate_legacy_skill_sources(validator, item, plugin_dir);
    }
}

fn validate_local_plugin_manifest_and_sources(
    validator: &mut Validator,
    item: &str,
    plugin_dir: &Path,
    plugin_name: &str,
) {
    validate_local_plugin_manifest(validator, item, plugin_dir, plugin_name);
    validate_plugin_sources(validator, item, plugin_dir);
}

fn path_item(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;

    const DOT_CODE_FRAGMENT: &str = "dependencies:\n  apm:\n    - ~/.apm/plugins/dot-code\n";

    fn write_fragment(root: &Path, content: &str) {
        let config_dir = root.join("symlinks").join("apm").join("config");
        std::fs::create_dir_all(&config_dir).expect("create apm config dir");
        std::fs::write(config_dir.join("base.yml"), content).expect("write apm fragment");
    }

    fn write_plugin(root: &Path, plugin_name: &str, manifest: &str) {
        let plugin_dir = root
            .join("symlinks")
            .join("apm")
            .join("plugins")
            .join(plugin_name);
        std::fs::create_dir_all(&plugin_dir).expect("create plugin dir");
        std::fs::write(plugin_dir.join("plugin.json"), manifest).expect("write plugin manifest");
        write_skill(&plugin_dir.join("skills"), "example");
    }

    fn write_native_plugin(root: &Path, plugin_name: &str, manifest: &str) {
        let plugin_dir = root
            .join("symlinks")
            .join("apm")
            .join("plugins")
            .join(plugin_name);
        std::fs::create_dir_all(&plugin_dir).expect("create plugin dir");
        std::fs::write(plugin_dir.join("apm.yml"), manifest).expect("write apm manifest");
        write_skill(&plugin_dir.join(".apm").join("skills"), "example");
    }

    fn write_skill(skills_root: &Path, skill_name: &str) {
        let skill_dir = skills_root.join(skill_name);
        std::fs::create_dir_all(&skill_dir).expect("create skill dir");
        std::fs::write(skill_dir.join("SKILL.md"), "---\nname: example\n---\n")
            .expect("write skill");
    }

    fn write_dot_code_fragment(root: &Path) {
        write_fragment(root, DOT_CODE_FRAGMENT);
    }

    fn assert_single_warning_contains(root: &Path, expected: &str) {
        let warnings = validate(root, None);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains(expected));
    }

    #[test]
    fn validate_accepts_matching_local_plugin_ref() {
        let temp_dir = tempfile::tempdir().unwrap();
        write_dot_code_fragment(temp_dir.path());
        write_plugin(temp_dir.path(), "dot-code", r#"{ "name": "dot-code" }"#);

        assert!(validate(temp_dir.path(), None).is_empty());
    }

    #[test]
    fn validate_accepts_native_local_plugin_ref() {
        let temp_dir = tempfile::tempdir().unwrap();
        write_dot_code_fragment(temp_dir.path());
        write_native_plugin(
            temp_dir.path(),
            "dot-code",
            "name: dot-code\nversion: 1.0.0\n",
        );

        assert!(validate(temp_dir.path(), None).is_empty());
    }

    #[test]
    fn validate_normalizes_backslash_local_plugin_ref() {
        let temp_dir = tempfile::tempdir().unwrap();
        write_fragment(
            temp_dir.path(),
            "dependencies:\n  apm:\n    - '~\\.apm\\plugins\\dot-code'\n",
        );

        assert_single_warning_contains(temp_dir.path(), "has no matching directory");
    }

    #[test]
    fn validate_accepts_well_formed_mcp_entry() {
        let temp_dir = tempfile::tempdir().unwrap();
        write_fragment(
            temp_dir.path(),
            "dependencies:\n  mcp:\n    - name: kusto\n      command: agency\n",
        );

        assert!(validate(temp_dir.path(), None).is_empty());
    }

    #[test]
    fn validate_detects_mcp_entry_missing_name() {
        let temp_dir = tempfile::tempdir().unwrap();
        write_fragment(
            temp_dir.path(),
            "dependencies:\n  mcp:\n    - command: agency\n",
        );

        assert_single_warning_contains(temp_dir.path(), "missing a non-empty 'name'");
    }

    #[test]
    fn validate_detects_mcp_entry_missing_command_and_url() {
        let temp_dir = tempfile::tempdir().unwrap();
        write_fragment(
            temp_dir.path(),
            "dependencies:\n  mcp:\n    - name: kusto\n",
        );

        assert_single_warning_contains(temp_dir.path(), "must define a 'command'");
    }

    #[test]
    fn validate_accepts_mcp_registry_string_shorthand() {
        let temp_dir = tempfile::tempdir().unwrap();
        write_fragment(
            temp_dir.path(),
            "dependencies:\n  mcp:\n    - some/registry-server\n",
        );

        assert!(validate(temp_dir.path(), None).is_empty());
    }

    #[test]
    fn validate_detects_missing_local_plugin_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        write_dot_code_fragment(temp_dir.path());

        assert_single_warning_contains(temp_dir.path(), "has no matching directory");
    }

    #[test]
    fn validate_detects_missing_local_plugin_manifest() {
        let temp_dir = tempfile::tempdir().unwrap();
        write_dot_code_fragment(temp_dir.path());
        std::fs::create_dir_all(
            temp_dir
                .path()
                .join("symlinks")
                .join("apm")
                .join("plugins")
                .join("dot-code"),
        )
        .expect("create plugin dir");

        assert_single_warning_contains(temp_dir.path(), "missing apm.yml or plugin.json");
    }

    #[test]
    fn validate_detects_plugin_name_mismatch() {
        let temp_dir = tempfile::tempdir().unwrap();
        write_dot_code_fragment(temp_dir.path());
        write_plugin(temp_dir.path(), "dot-code", r#"{ "name": "wrong-name" }"#);

        assert_single_warning_contains(temp_dir.path(), "does not match");
    }

    #[test]
    fn validate_detects_native_plugin_name_mismatch() {
        let temp_dir = tempfile::tempdir().unwrap();
        write_dot_code_fragment(temp_dir.path());
        write_native_plugin(
            temp_dir.path(),
            "dot-code",
            "name: wrong-name\nversion: 1.0.0\n",
        );

        assert_single_warning_contains(temp_dir.path(), "does not match");
    }

    #[test]
    fn validate_checks_overlay_fragments_against_overlay_plugins() {
        let root = tempfile::tempdir().unwrap();
        let overlay = tempfile::tempdir().unwrap();
        write_fragment(
            overlay.path(),
            "dependencies:\n  apm:\n    - ~/.apm/plugins/dot-work\n",
        );
        write_plugin(overlay.path(), "dot-work", r#"{ "name": "dot-work" }"#);

        assert!(validate(root.path(), Some(overlay.path())).is_empty());
    }

    #[test]
    fn validate_warns_on_invalid_fragment_yaml() {
        let temp_dir = tempfile::tempdir().unwrap();
        write_fragment(temp_dir.path(), "dependencies: [");

        assert_single_warning_contains(temp_dir.path(), "could not parse");
    }
}
