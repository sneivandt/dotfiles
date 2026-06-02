//! Discovery, merging, and de-duplication of APM YAML config fragments.

use anyhow::{Context as _, Result};
use std::collections::HashSet;
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

/// Parse each fragment, concatenate `dependencies.apm` and `dependencies.mcp`
/// across all of them, and emit a single YAML document string.
///
/// Duplicate entries are dropped, keeping the first occurrence: `apm` refs are
/// deduplicated by their serialized value and `mcp` servers by their `name`
/// field (falling back to the serialized value for string shorthands). This
/// keeps the generated manifest stable when base and overlay fragments declare
/// the same plugin or MCP server.
pub(super) fn merge_fragments(fragments: &[PathBuf]) -> Result<String> {
    use serde_yaml_ng::Value;

    let mut apm_deps: Vec<Value> = Vec::new();
    let mut mcp_deps: Vec<Value> = Vec::new();
    let mut seen_apm: HashSet<String> = HashSet::new();
    let mut seen_mcp: HashSet<String> = HashSet::new();

    for path in fragments {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("reading manifest fragment {}", path.display()))?;
        if content.trim().is_empty() {
            continue;
        }
        let value: Value = serde_yaml_ng::from_str(&content)
            .with_context(|| format!("parsing manifest fragment {}", path.display()))?;

        if let Some(deps) = value.get("dependencies") {
            if let Some(seq) = deps.get("apm").and_then(Value::as_sequence) {
                for entry in seq {
                    if seen_apm.insert(apm_dedup_key(entry)) {
                        apm_deps.push(entry.clone());
                    }
                }
            }
            if let Some(seq) = deps.get("mcp").and_then(Value::as_sequence) {
                for entry in seq {
                    if seen_mcp.insert(mcp_dedup_key(entry)) {
                        mcp_deps.push(entry.clone());
                    }
                }
            }
        }
    }

    let mut root = serde_yaml_ng::Mapping::new();
    root.insert(Value::from("name"), Value::from("dotfiles"));
    root.insert(Value::from("version"), Value::from("1.0.0"));

    let mut deps = serde_yaml_ng::Mapping::new();
    if !apm_deps.is_empty() {
        deps.insert(Value::from("apm"), Value::Sequence(apm_deps));
    }
    if !mcp_deps.is_empty() {
        deps.insert(Value::from("mcp"), Value::Sequence(mcp_deps));
    }
    if !deps.is_empty() {
        root.insert(Value::from("dependencies"), Value::Mapping(deps));
    }

    let body = serde_yaml_ng::to_string(&Value::Mapping(root))
        .context("serialising merged apm manifest")?;
    Ok(format!("{GENERATED_HEADER}{body}"))
}

/// Deduplication key for an `apm` dependency: its serialized representation.
fn apm_dedup_key(entry: &serde_yaml_ng::Value) -> String {
    serde_yaml_ng::to_string(entry).unwrap_or_else(|_| format!("{entry:?}"))
}

/// Deduplication key for an `mcp` dependency: its `name` field when present,
/// otherwise its serialized representation (registry string shorthands).
fn mcp_dedup_key(entry: &serde_yaml_ng::Value) -> String {
    entry
        .get("name")
        .and_then(serde_yaml_ng::Value::as_str)
        .map_or_else(
            || format!("@{}", apm_dedup_key(entry)),
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
    fn merge_fragments_skips_empty_files() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let f = dir.path().join("empty.yml");
        std::fs::write(&f, "\n\n").expect("write");
        let merged = merge_fragments(&[f]).expect("merge");
        assert!(merged.contains("name: dotfiles"));
        assert!(!merged.contains("dependencies:"));
    }
}
