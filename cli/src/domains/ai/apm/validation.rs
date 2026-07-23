//! Validation for APM config fragments and local plugin sources.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde_yaml_ng::Value as YamlValue;

use crate::infra::config::Diagnostic;
use crate::infra::config::validation::Validator;

mod mcp;
mod plugin;

const SOURCE: &str = "apm/config/*.yml";

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
            if let Some(plugin_name) = plugin::local_dot_plugin_name(dependency) {
                plugin::validate_local_ref(validator, root, fragment, &plugin_name);
            }
        }
    }

    validate_git_dependency_keys(validator, root, fragment, &value);
    mcp::validate_dependencies(validator, root, fragment, &value);
}

fn validate_git_dependency_keys(
    validator: &mut Validator,
    root: &Path,
    fragment: &Path,
    value: &YamlValue,
) {
    for section in ["dependencies", "devDependencies"] {
        let Some(apm_deps) = value
            .get(section)
            .and_then(|dependencies| dependencies.get("apm"))
            .and_then(YamlValue::as_sequence)
        else {
            continue;
        };

        for (index, dependency) in apm_deps.iter().enumerate() {
            if dependency.get("git").is_none() {
                continue;
            }
            let item = format!("{}:{section}.apm[{index}]", path_item(root, fragment));
            if dependency.get("version").is_some() {
                validator.warn(
                    "apm.git-version",
                    item.clone(),
                    "Git dependencies do not support `version`; use `ref` to select a tag, branch, or commit",
                );
            }
            if dependency.get("name").is_some() {
                validator.warn(
                    "apm.git-name",
                    item,
                    "Git dependencies do not support `name`; use `alias` to set a local dependency name",
                );
            }
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

    fn write_prompt(plugin_dir: &Path, prompt_name: &str) {
        let prompts_dir = plugin_dir.join(".apm").join("prompts");
        std::fs::create_dir_all(&prompts_dir).expect("create prompts dir");
        std::fs::write(
            prompts_dir.join(format!("{prompt_name}.prompt.md")),
            "---\nname: example\n---\n",
        )
        .expect("write prompt");
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
    fn validate_accepts_native_prompt_only_plugin_ref() {
        let temp_dir = tempfile::tempdir().unwrap();
        write_dot_code_fragment(temp_dir.path());
        let plugin_dir = temp_dir
            .path()
            .join("symlinks")
            .join("apm")
            .join("plugins")
            .join("dot-code");
        std::fs::create_dir_all(&plugin_dir).expect("create plugin dir");
        std::fs::write(
            plugin_dir.join("apm.yml"),
            "name: dot-code\nversion: 1.0.0\n",
        )
        .expect("write apm manifest");
        write_prompt(&plugin_dir, "example");

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

    #[test]
    fn validate_accepts_supported_git_dependency_keys() {
        let temp_dir = tempfile::tempdir().unwrap();
        write_fragment(
            temp_dir.path(),
            "dependencies:\n  apm:\n    - git: github.com/example/plugin\n      path: plugins/example\n      ref: v1.2.3\n      alias: example\n      type: git\n      allow_insecure: false\n      skills: [review]\n      targets: [copilot]\n",
        );

        assert!(validate(temp_dir.path(), None).is_empty());
    }

    #[test]
    fn validate_rejects_name_and_version_on_git_dependencies() {
        let temp_dir = tempfile::tempdir().unwrap();
        write_fragment(
            temp_dir.path(),
            "dependencies:\n  apm:\n    - git: github.com/example/plugin\n      name: example\n      version: 1.2.3\n",
        );

        let diagnostics = validate(temp_dir.path(), None);
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].code, "apm.git-version");
        assert_eq!(diagnostics[1].code, "apm.git-name");
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.item.contains("dependencies.apm[0]"))
        );
    }

    #[test]
    fn validate_checks_git_keys_in_dev_dependencies() {
        let temp_dir = tempfile::tempdir().unwrap();
        write_fragment(
            temp_dir.path(),
            "devDependencies:\n  apm:\n    - git: github.com/example/plugin\n      version: main\n",
        );

        let diagnostics = validate(temp_dir.path(), None);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "apm.git-version");
        assert!(diagnostics[0].item.contains("devDependencies.apm[0]"));
    }

    #[test]
    fn validate_allows_name_and_version_on_registry_dependencies() {
        let temp_dir = tempfile::tempdir().unwrap();
        write_fragment(
            temp_dir.path(),
            "dependencies:\n  apm:\n    - name: example/plugin\n      version: 1.2.3\n",
        );

        assert!(validate(temp_dir.path(), None).is_empty());
    }

    #[test]
    fn validate_checks_git_keys_in_overlay_fragments() {
        let root = tempfile::tempdir().unwrap();
        let overlay = tempfile::tempdir().unwrap();
        write_fragment(
            overlay.path(),
            "dependencies:\n  apm:\n    - git: github.com/example/plugin\n      name: example\n",
        );

        let diagnostics = validate(root.path(), Some(overlay.path()));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "apm.git-name");
    }
}
