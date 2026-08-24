//! Dotfiles-specific cross-file validation for local APM plugin references.
//!
//! Native APM validates fragment YAML, dependency schemas, MCP declarations,
//! and package layout. This validator only checks the relationship APM cannot
//! infer from a source fragment: a local `dot-*` reference must have a matching
//! source directory in the same repository or overlay.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde_yaml_ng::Value as YamlValue;

use crate::infra::config::validation::Validator;
use crate::infra::config::{Diagnostic, DiagnosticCode};

const SOURCE: &str = "apm/config/*.yml";
const LOCAL_PLUGIN_PREFIX: &str = "~/.apm/plugins/";
const LOCAL_PLUGIN_NAME_PREFIX: &str = "dot-";

/// Diagnostic code: `apm.io-error`.
const APM_IO_ERROR: DiagnosticCode = DiagnosticCode::new("apm", "io-error");
/// Diagnostic code: `apm.plugin-dir-missing`.
const APM_PLUGIN_DIR_MISSING: DiagnosticCode = DiagnosticCode::new("apm", "plugin-dir-missing");

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
                APM_IO_ERROR,
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
        if is_yaml_fragment(&path) && std::fs::metadata(&path)?.is_file() {
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
                APM_IO_ERROR,
                path_item(root, fragment),
                format!("could not read APM manifest fragment: {err}"),
            );
            return;
        }
    };
    let Ok(value) = serde_yaml_ng::from_str::<YamlValue>(&content) else {
        return;
    };

    for section in ["dependencies", "devDependencies"] {
        let Some(apm_deps) = value
            .get(section)
            .and_then(|dependencies| dependencies.get("apm"))
            .and_then(YamlValue::as_sequence)
        else {
            continue;
        };
        for dependency in apm_deps {
            let Some(plugin_name) = local_dot_plugin_name(dependency) else {
                continue;
            };
            validate_local_ref(validator, root, fragment, &plugin_name);
        }
    }
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

fn validate_local_ref(validator: &mut Validator, root: &Path, fragment: &Path, plugin_name: &str) {
    let plugin_dir = root
        .join("symlinks")
        .join("apm")
        .join("plugins")
        .join(plugin_name);
    if plugin_dir.is_dir() {
        return;
    }
    validator.warn(
        APM_PLUGIN_DIR_MISSING,
        format!(
            "{}: {LOCAL_PLUGIN_PREFIX}{plugin_name}",
            path_item(root, fragment)
        ),
        format!(
            "local APM plugin reference has no matching directory: {}",
            plugin_dir.display()
        ),
    );
}

fn path_item(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_fragment(root: &Path, content: &str) {
        let config_dir = root.join("symlinks").join("apm").join("config");
        std::fs::create_dir_all(&config_dir).expect("create APM config dir");
        std::fs::write(config_dir.join("base.yml"), content).expect("write APM fragment");
    }

    fn write_plugin(root: &Path, name: &str) {
        std::fs::create_dir_all(root.join("symlinks").join("apm").join("plugins").join(name))
            .expect("create plugin source");
    }

    #[test]
    fn accepts_matching_local_plugin_ref() {
        let dir = tempfile::tempdir().expect("create temp dir");
        write_fragment(
            dir.path(),
            "dependencies:\n  apm:\n    - ~/.apm/plugins/dot-code\n",
        );
        write_plugin(dir.path(), "dot-code");

        assert!(validate(dir.path(), None).is_empty());
    }

    #[test]
    fn reports_missing_local_plugin_source() {
        let dir = tempfile::tempdir().expect("create temp dir");
        write_fragment(
            dir.path(),
            "dependencies:\n  apm:\n    - ~/.apm/plugins/dot-code\n",
        );

        let diagnostics = validate(dir.path(), None);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, APM_PLUGIN_DIR_MISSING);
    }

    #[test]
    fn validates_overlay_plugin_against_overlay_source() {
        let root = tempfile::tempdir().expect("create root");
        let overlay = tempfile::tempdir().expect("create overlay");
        write_fragment(
            overlay.path(),
            "devDependencies:\n  apm:\n    - ~/.apm/plugins/dot-work\n",
        );
        write_plugin(overlay.path(), "dot-work");

        assert!(validate(root.path(), Some(overlay.path())).is_empty());
    }

    #[test]
    fn leaves_manifest_schema_validation_to_apm() {
        let dir = tempfile::tempdir().expect("create temp dir");
        write_fragment(dir.path(), "dependencies: [");

        assert!(validate(dir.path(), None).is_empty());
    }
}
