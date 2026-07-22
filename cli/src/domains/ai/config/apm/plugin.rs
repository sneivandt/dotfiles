//! Validation for local APM plugin references and their source layout.

use std::io::ErrorKind;
use std::path::Path;

use serde_json::Value as JsonValue;
use serde_yaml_ng::Value as YamlValue;

use super::path_item;
use crate::infra::config::validation::Validator;

const LOCAL_PLUGIN_PREFIX: &str = "~/.apm/plugins/";
const LOCAL_PLUGIN_NAME_PREFIX: &str = "dot-";

pub(super) fn local_dot_plugin_name(dependency: &YamlValue) -> Option<String> {
    let normalized = dependency.as_str()?.replace('\\', "/");
    let plugin_name = normalized
        .strip_prefix(LOCAL_PLUGIN_PREFIX)?
        .trim_end_matches('/')
        .to_owned();
    plugin_name
        .starts_with(LOCAL_PLUGIN_NAME_PREFIX)
        .then_some(plugin_name)
}

pub(super) fn validate_local_ref(
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

    validate_manifest_and_sources(validator, &item, &plugin_dir, plugin_name);
}

fn validate_manifest(validator: &mut Validator, item: &str, plugin_dir: &Path, plugin_name: &str) {
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
            if !has_native_prompt_sources(plugin_dir) {
                validator.warn(
                    "apm.skill-dir-missing",
                    item,
                    format!(
                        "native APM plugin is missing .apm/skills directory: {}",
                        skills_dir.display()
                    ),
                );
            }
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

fn has_native_prompt_sources(plugin_dir: &Path) -> bool {
    let prompts_dir = plugin_dir.join(".apm").join("prompts");
    std::fs::read_dir(prompts_dir).is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            entry.path().is_file()
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.ends_with(".prompt.md"))
        })
    })
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

fn validate_sources(validator: &mut Validator, item: &str, plugin_dir: &Path) {
    if plugin_dir.join("apm.yml").is_file() {
        validate_native_skill_sources(validator, item, plugin_dir);
    } else if plugin_dir.join("plugin.json").is_file() {
        validate_legacy_skill_sources(validator, item, plugin_dir);
    }
}

fn validate_manifest_and_sources(
    validator: &mut Validator,
    item: &str,
    plugin_dir: &Path,
    plugin_name: &str,
) {
    validate_manifest(validator, item, plugin_dir, plugin_name);
    validate_sources(validator, item, plugin_dir);
}
