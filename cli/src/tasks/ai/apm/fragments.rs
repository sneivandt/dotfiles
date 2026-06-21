//! Discovery, merging, and de-duplication of APM YAML config fragments.

use anyhow::{Context as _, Result};
use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::GENERATED_HEADER;

/// Discover `*.yml` and `*.yaml` files in `~/.apm/config/`.
///
/// Returns an empty vector if the directory does not exist.  Results are
/// sorted by path so the merged manifest is deterministic regardless of the
/// filesystem's enumeration order.
pub(super) fn discover_fragment_files(home: &Path) -> Result<Vec<PathBuf>> {
    discover_yaml_files(&home.join(".apm").join("config"))
}

/// Discover YAML files in an APM fragment directory.
pub(super) fn discover_yaml_files(config_dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(config_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(err)
                .with_context(|| format!("reading APM config directory {}", config_dir.display()));
        }
    };

    let mut files = Vec::new();
    for entry in entries {
        let entry = entry
            .with_context(|| format!("reading directory entry in {}", config_dir.display()))?;
        let path = entry.path();
        if !is_yaml_fragment(&path) {
            continue;
        }
        let metadata = std::fs::metadata(&path).with_context(|| {
            format!("reading metadata for manifest fragment {}", path.display())
        })?;
        if metadata.is_file() {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// Return whether a path has a YAML extension supported by APM fragments.
fn is_yaml_fragment(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("yml") || ext.eq_ignore_ascii_case("yaml"))
}

/// Parse each fragment, layer supported manifest fields, and emit one YAML
/// document string.
///
/// Dependency groups under both `dependencies` and `devDependencies` are
/// concatenated by dependency kind, so current APM groups such as `apm`, `mcp`,
/// and `lsp` plus future groups survive the merge. Duplicate entries are
/// dropped, keeping the first occurrence: `mcp` servers are deduplicated by
/// their `name` field when present, while other dependency kinds use the
/// serialized entry as the key.
///
/// Non-dependency manifest fields are layered in fragment order: mappings merge
/// recursively, sequences append unique values, equal values are kept, and a
/// later scalar replaces an earlier scalar. `name` and `version` remain owned by
/// dotfiles so the generated manifest identity is stable.
pub(super) fn merge_fragments(fragments: &[PathBuf]) -> Result<String> {
    use serde_yaml_ng::Value;

    let mut root = serde_yaml_ng::Mapping::new();
    root.insert(Value::from("name"), Value::from("dotfiles"));
    root.insert(Value::from("version"), Value::from("1.0.0"));

    let mut seen_dependencies: HashMap<String, HashSet<String>> = HashMap::new();
    let mut seen_dev_dependencies: HashMap<String, HashSet<String>> = HashMap::new();

    for path in fragments {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("reading manifest fragment {}", path.display()))?;
        if content.trim().is_empty() {
            continue;
        }
        let value: Value = serde_yaml_ng::from_str(&content)
            .with_context(|| format!("parsing manifest fragment {}", path.display()))?;

        let mapping = value.as_mapping().with_context(|| {
            format!("manifest fragment {} is not a YAML mapping", path.display())
        })?;

        for (key, fragment_value) in mapping {
            let Some(key_name) = key.as_str() else {
                merge_manifest_value(&mut root, key, fragment_value);
                continue;
            };
            match key_name {
                "name" | "version" => {}
                "dependencies" => merge_dependency_section(
                    &mut root,
                    "dependencies",
                    fragment_value,
                    &mut seen_dependencies,
                    path,
                )?,
                "devDependencies" => merge_dependency_section(
                    &mut root,
                    "devDependencies",
                    fragment_value,
                    &mut seen_dev_dependencies,
                    path,
                )?,
                _ => merge_manifest_value(&mut root, key, fragment_value),
            }
        }
    }

    let body = serde_yaml_ng::to_string(&Value::Mapping(root))
        .context("serialising merged apm manifest")?;
    Ok(format!("{GENERATED_HEADER}{body}"))
}

/// Merge a top-level non-dependency manifest field into the generated root.
fn merge_manifest_value(
    root: &mut serde_yaml_ng::Mapping,
    key: &serde_yaml_ng::Value,
    incoming: &serde_yaml_ng::Value,
) {
    match root.get_mut(key) {
        Some(existing) => merge_layered_value(existing, incoming),
        None => {
            root.insert(key.clone(), incoming.clone());
        }
    }
}

/// Merge one layered YAML value into another.
fn merge_layered_value(existing: &mut serde_yaml_ng::Value, incoming: &serde_yaml_ng::Value) {
    use serde_yaml_ng::Value;

    match (existing, incoming) {
        (Value::Mapping(existing_map), Value::Mapping(incoming_map)) => {
            for (key, value) in incoming_map {
                merge_manifest_value(existing_map, key, value);
            }
        }
        (Value::Sequence(existing_items), Value::Sequence(incoming_items)) => {
            append_unique_values(existing_items, incoming_items);
        }
        (existing_value, incoming_value) if existing_value == incoming_value => {}
        (existing_value, incoming_value) => {
            *existing_value = incoming_value.clone();
        }
    }
}

/// Append values from `incoming` that are not already present in `existing`.
fn append_unique_values(
    existing: &mut Vec<serde_yaml_ng::Value>,
    incoming: &[serde_yaml_ng::Value],
) {
    let mut seen: HashSet<String> = existing.iter().map(value_dedup_key).collect();
    for entry in incoming {
        if seen.insert(value_dedup_key(entry)) {
            existing.push(entry.clone());
        }
    }
}

/// Merge one dependency section (`dependencies` or `devDependencies`).
fn merge_dependency_section(
    root: &mut serde_yaml_ng::Mapping,
    section_name: &'static str,
    section: &serde_yaml_ng::Value,
    seen: &mut HashMap<String, HashSet<String>>,
    fragment: &Path,
) -> Result<()> {
    let groups = section.as_mapping().with_context(|| {
        format!(
            "{section_name} in manifest fragment {} must be a YAML mapping",
            fragment.display()
        )
    })?;
    let target_section = ensure_mapping_field(root, section_name)?;

    for (kind_value, entries_value) in groups {
        let kind = kind_value.as_str().with_context(|| {
            format!(
                "{section_name} dependency kind in manifest fragment {} is not a string",
                fragment.display()
            )
        })?;
        let entries = entries_value.as_sequence().with_context(|| {
            format!(
                "{section_name}.{kind} in manifest fragment {} must be a YAML sequence",
                fragment.display()
            )
        })?;
        let target_entries = ensure_sequence_field(target_section, kind)?;
        let seen_entries = seen.entry(kind.to_owned()).or_default();
        for entry in entries {
            if seen_entries.insert(dependency_dedup_key(kind, entry)) {
                target_entries.push(entry.clone());
            }
        }
    }

    Ok(())
}

/// Return a mapping field, creating it if needed.
fn ensure_mapping_field<'a>(
    mapping: &'a mut serde_yaml_ng::Mapping,
    field: &str,
) -> Result<&'a mut serde_yaml_ng::Mapping> {
    let key = serde_yaml_ng::Value::from(field);
    if !mapping.contains_key(&key) {
        mapping.insert(
            key.clone(),
            serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::new()),
        );
    }
    match mapping.get_mut(&key) {
        Some(serde_yaml_ng::Value::Mapping(value)) => Ok(value),
        Some(_) | None => anyhow::bail!("generated APM manifest field {field} is not a mapping"),
    }
}

/// Return a sequence field, creating it if needed.
fn ensure_sequence_field<'a>(
    mapping: &'a mut serde_yaml_ng::Mapping,
    field: &str,
) -> Result<&'a mut Vec<serde_yaml_ng::Value>> {
    let key = serde_yaml_ng::Value::from(field);
    if !mapping.contains_key(&key) {
        mapping.insert(key.clone(), serde_yaml_ng::Value::Sequence(Vec::new()));
    }
    match mapping.get_mut(&key) {
        Some(serde_yaml_ng::Value::Sequence(value)) => Ok(value),
        Some(_) | None => {
            anyhow::bail!("generated APM manifest dependency group {field} is not a sequence")
        }
    }
}

/// Deduplication key for a dependency entry.
fn dependency_dedup_key(kind: &str, entry: &serde_yaml_ng::Value) -> String {
    if kind == "mcp" {
        mcp_dedup_key(entry)
    } else {
        value_dedup_key(entry)
    }
}

/// Deduplication key for a generic YAML value: its serialized representation.
fn value_dedup_key(entry: &serde_yaml_ng::Value) -> String {
    serde_yaml_ng::to_string(entry).unwrap_or_else(|_| format!("{entry:?}"))
}

/// Deduplication key for an `mcp` dependency: its `name` field when present,
/// otherwise its serialized representation (registry string shorthands).
fn mcp_dedup_key(entry: &serde_yaml_ng::Value) -> String {
    entry
        .get("name")
        .and_then(serde_yaml_ng::Value::as_str)
        .map_or_else(
            || format!("@{}", value_dedup_key(entry)),
            |name| format!("name:{name}"),
        )
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;

    #[test]
    fn discover_fragment_files_returns_empty_when_dir_missing() {
        let dir = tempfile::tempdir().expect("create temp dir");
        assert!(
            discover_fragment_files(dir.path())
                .expect("discover")
                .is_empty()
        );
    }

    #[test]
    fn discover_fragment_files_errors_when_config_path_is_not_directory() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let apm_dir = dir.path().join(".apm");
        std::fs::create_dir_all(&apm_dir).expect("create ~/.apm");
        std::fs::write(apm_dir.join("config"), "not a directory\n").expect("write config file");

        let err = discover_fragment_files(dir.path()).expect_err("config file should error");
        assert!(
            format!("{err:#}").contains("reading APM config directory"),
            "expected context for read_dir failure, got {err:#}"
        );
    }

    #[test]
    fn discover_fragment_files_returns_yaml_files_only_sorted() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let cfg = dir.path().join(".apm").join("config");
        std::fs::create_dir_all(&cfg).expect("create config dir");
        std::fs::write(cfg.join("work.yml"), "name: work\n").expect("write work.yml");
        std::fs::write(cfg.join("base.yaml"), "name: base\n").expect("write base.yaml");
        std::fs::write(cfg.join("README.md"), "ignore me\n").expect("write README.md");

        let files = discover_fragment_files(dir.path()).expect("discover");
        assert_eq!(files.len(), 2);
        assert!(files[0].ends_with("base.yaml"));
        assert!(files[1].ends_with("work.yml"));
    }

    #[test]
    fn merge_fragments_concatenates_apm_and_mcp_dependencies() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let a = dir.path().join("a.yml");
        let b = dir.path().join("b.yml");
        std::fs::write(
            &a,
            "name: a\nversion: 1.0.0\ndependencies:\n  apm:\n    - foo/bar\n",
        )
        .expect("write a");
        std::fs::write(
            &b,
            "name: b\nversion: 1.0.0\ndependencies:\n  apm:\n    - baz/qux\n  mcp:\n    - server-1\n",
        )
        .expect("write b");

        let merged = merge_fragments(&[a, b]).expect("merge");
        assert!(merged.starts_with(GENERATED_HEADER));
        assert!(merged.contains("foo/bar"));
        assert!(merged.contains("baz/qux"));
        assert!(merged.contains("server-1"));
        assert!(merged.contains("name: dotfiles"));
    }

    #[test]
    fn merge_fragments_deduplicates_apm_and_mcp_entries() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let a = dir.path().join("a.yml");
        let b = dir.path().join("b.yml");
        std::fs::write(
            &a,
            "name: a\nversion: 1.0.0\ndependencies:\n  apm:\n    - foo/bar\n  mcp:\n    - name: kusto\n      command: agency\n",
        )
        .expect("write a");
        std::fs::write(
            &b,
            "name: b\nversion: 1.0.0\ndependencies:\n  apm:\n    - foo/bar\n  mcp:\n    - name: kusto\n      command: other\n",
        )
        .expect("write b");

        let merged = merge_fragments(&[a, b]).expect("merge");
        assert_eq!(merged.matches("foo/bar").count(), 1);
        assert_eq!(merged.matches("name: kusto").count(), 1);
        // First occurrence wins, so the duplicate's command is dropped.
        assert!(merged.contains("agency"));
        assert!(!merged.contains("other"));
    }

    #[test]
    fn merge_fragments_preserves_complex_map_entries() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let f = dir.path().join("complex.yml");
        std::fs::write(
            &f,
            "name: x\nversion: 1.0.0\ndependencies:\n  apm:\n    - git: dev.azure.com/org/repo\n      path: services/foo\n",
        )
        .expect("write");
        let merged = merge_fragments(&[f]).expect("merge");
        assert!(merged.contains("dev.azure.com/org/repo"));
        assert!(merged.contains("services/foo"));
    }

    #[test]
    fn merge_fragments_preserves_newer_manifest_fields() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let a = dir.path().join("a.yml");
        let b = dir.path().join("b.yml");
        std::fs::write(
            &a,
            "\
name: a
version: 1.0.0
targets:
  - copilot
policy:
  fetch_failure_default: warn
registries:
  default: corp
  corp:
    url: https://packages.example.com
scripts:
  review: copilot -p review.prompt.md
dependencies:
  lsp:
    - rust-analyzer
devDependencies:
  apm:
    - ./dev/package
",
        )
        .expect("write a");
        std::fs::write(
            &b,
            "\
name: b
version: 1.0.0
targets:
  - copilot
  - claude
policy:
  hash: sha256:abcdef
scripts:
  start: copilot -p start.prompt.md
dependencies:
  lsp:
    - rust-analyzer
    - yaml-language-server
devDependencies:
  mcp:
    - name: fixture
      command: fixture-mcp
",
        )
        .expect("write b");

        let merged = merge_fragments(&[a, b]).expect("merge");

        assert!(merged.contains("targets:"));
        assert_eq!(merged.matches("- copilot").count(), 1);
        assert!(merged.contains("- claude"));
        assert!(merged.contains("fetch_failure_default: warn"));
        assert!(merged.contains("hash: sha256:abcdef"));
        assert!(merged.contains("registries:"));
        assert!(merged.contains("review: copilot -p review.prompt.md"));
        assert!(merged.contains("start: copilot -p start.prompt.md"));
        assert!(merged.contains("lsp:"));
        assert_eq!(merged.matches("rust-analyzer").count(), 1);
        assert!(merged.contains("yaml-language-server"));
        assert!(merged.contains("devDependencies:"));
        assert!(merged.contains("./dev/package"));
        assert!(merged.contains("name: fixture"));
    }

    #[test]
    fn merge_fragments_replaces_scalar_fields_with_later_fragments() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let a = dir.path().join("a.yml");
        let b = dir.path().join("b.yml");
        std::fs::write(&a, "description: base description\n").expect("write a");
        std::fs::write(&b, "description: overlay description\n").expect("write b");

        let merged = merge_fragments(&[a, b]).expect("merge");

        assert!(merged.contains("description: overlay description"));
        assert!(!merged.contains("description: base description"));
    }

    #[test]
    fn merge_fragments_errors_when_dependency_section_is_not_mapping() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let f = dir.path().join("bad.yml");
        std::fs::write(&f, "dependencies: []\n").expect("write");

        let err = merge_fragments(&[f]).expect_err("dependencies must be a mapping");

        assert!(
            format!("{err:#}").contains("dependencies in manifest fragment"),
            "expected dependency section context, got {err:#}"
        );
    }

    #[test]
    fn merge_fragments_errors_when_dependency_group_is_not_sequence() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let f = dir.path().join("bad.yml");
        std::fs::write(&f, "dependencies:\n  apm: foo/bar\n").expect("write");

        let err = merge_fragments(&[f]).expect_err("dependencies.apm must be a sequence");

        assert!(
            format!("{err:#}").contains("dependencies.apm"),
            "expected dependency group context, got {err:#}"
        );
    }

    #[test]
    fn merge_fragments_skips_empty_files() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let f = dir.path().join("empty.yml");
        std::fs::write(&f, "\n\n").expect("write");
        let merged = merge_fragments(&[f]).expect("merge");
        assert!(merged.contains("name: dotfiles"));
        assert!(!merged.contains("dependencies:"));
    }
}
