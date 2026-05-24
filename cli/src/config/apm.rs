//! Validation for local Microsoft APM plugin references.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde_json::Value as JsonValue;
use serde_yaml_ng::Value as YamlValue;

use super::ValidationWarning;
use super::helpers::validation::Validator;

const SOURCE: &str = "apm/config/*.yml";
const LOCAL_PLUGIN_PREFIX: &str = "~/.apm/plugins/";
const LOCAL_PLUGIN_NAME_PREFIX: &str = "dot-";

/// Validate local APM plugin references and return any warnings.
#[must_use]
pub(crate) fn validate(root: &Path, overlay: Option<&Path>) -> Vec<ValidationWarning> {
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
                path_item(root, fragment),
                format!("could not parse APM manifest fragment: {err}"),
            );
            return;
        }
    };

    let Some(apm_deps) = value
        .get("dependencies")
        .and_then(|dependencies| dependencies.get("apm"))
        .and_then(YamlValue::as_sequence)
    else {
        return;
    };

    for dependency in apm_deps {
        if let Some(plugin_name) = local_dot_plugin_name(dependency) {
            validate_local_plugin_ref(validator, root, fragment, plugin_name);
        }
    }
}

fn local_dot_plugin_name(dependency: &YamlValue) -> Option<&str> {
    let plugin_name = dependency
        .as_str()?
        .strip_prefix(LOCAL_PLUGIN_PREFIX)?
        .trim_end_matches('/');
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
            item,
            format!(
                "local APM plugin reference has no matching directory: {}",
                plugin_dir.display()
            ),
        );
        return;
    }

    validate_plugin_json(validator, &item, &plugin_dir, plugin_name);
}

fn validate_plugin_json(
    validator: &mut Validator,
    item: &str,
    plugin_dir: &Path,
    plugin_name: &str,
) {
    let manifest = plugin_dir.join("plugin.json");
    if !manifest.is_file() {
        validator.warn(
            item,
            format!(
                "local APM plugin is missing plugin.json: {}",
                manifest.display()
            ),
        );
        return;
    }

    let content = match std::fs::read_to_string(&manifest) {
        Ok(content) => content,
        Err(err) => {
            validator.warn(item, format!("could not read plugin.json: {err}"));
            return;
        }
    };
    let value: JsonValue = match serde_json::from_str(&content) {
        Ok(value) => value,
        Err(err) => {
            validator.warn(item, format!("could not parse plugin.json: {err}"));
            return;
        }
    };

    match value.get("name").and_then(JsonValue::as_str) {
        Some(name) if name == plugin_name => {}
        Some(name) if name.trim().is_empty() => {
            validator.warn(item, "plugin.json name is missing or empty");
        }
        Some(name) => {
            validator.warn(
                item,
                format!("plugin.json name '{name}' does not match local plugin '{plugin_name}'"),
            );
        }
        None => {
            validator.warn(item, "plugin.json name is missing or empty");
        }
    }
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
    }

    #[test]
    fn validate_accepts_matching_local_plugin_ref() {
        let temp_dir = tempfile::tempdir().unwrap();
        write_fragment(
            temp_dir.path(),
            "dependencies:\n  apm:\n    - ~/.apm/plugins/dot-code\n",
        );
        write_plugin(temp_dir.path(), "dot-code", r#"{ "name": "dot-code" }"#);

        assert!(validate(temp_dir.path(), None).is_empty());
    }

    #[test]
    fn validate_detects_missing_local_plugin_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        write_fragment(
            temp_dir.path(),
            "dependencies:\n  apm:\n    - ~/.apm/plugins/dot-code\n",
        );

        let warnings = validate(temp_dir.path(), None);

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("has no matching directory"));
    }

    #[test]
    fn validate_detects_missing_plugin_json() {
        let temp_dir = tempfile::tempdir().unwrap();
        write_fragment(
            temp_dir.path(),
            "dependencies:\n  apm:\n    - ~/.apm/plugins/dot-code\n",
        );
        std::fs::create_dir_all(
            temp_dir
                .path()
                .join("symlinks")
                .join("apm")
                .join("plugins")
                .join("dot-code"),
        )
        .expect("create plugin dir");

        let warnings = validate(temp_dir.path(), None);

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("missing plugin.json"));
    }

    #[test]
    fn validate_detects_plugin_name_mismatch() {
        let temp_dir = tempfile::tempdir().unwrap();
        write_fragment(
            temp_dir.path(),
            "dependencies:\n  apm:\n    - ~/.apm/plugins/dot-code\n",
        );
        write_plugin(temp_dir.path(), "dot-code", r#"{ "name": "wrong-name" }"#);

        let warnings = validate(temp_dir.path(), None);

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("does not match"));
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

        let warnings = validate(temp_dir.path(), None);

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("could not parse"));
    }
}
