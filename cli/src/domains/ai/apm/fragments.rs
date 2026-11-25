//! Discovery, merging, and de-duplication of APM YAML config fragments.

use crate::infra::fs::{PlannedDirectory, entry_name, link_points_to_source};
use crate::infra::yaml::{Mapping, Value};
use anyhow::{Context as _, Result};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsString;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::GENERATED_HEADER;

/// A configured APM fragment source and its declared home-relative target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ApmFragmentSource {
    source: PathBuf,
    target: PathBuf,
}

impl ApmFragmentSource {
    /// Create a managed fragment source resolved by the application layer.
    #[must_use]
    pub(crate) const fn new(source: PathBuf, target: PathBuf) -> Self {
        Self { source, target }
    }
}

/// Inactive APM fragment sources resolved from non-selected categories, plus
/// any glob inspection errors that must block reconciliation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct InactiveApmFragments {
    sources: Vec<ApmFragmentSource>,
    inspection_errors: Vec<String>,
}

impl InactiveApmFragments {
    #[must_use]
    pub(crate) const fn new(
        sources: Vec<ApmFragmentSource>,
        inspection_errors: Vec<String>,
    ) -> Self {
        Self {
            sources,
            inspection_errors,
        }
    }

    pub(crate) fn sources(&self) -> Result<&[ApmFragmentSource]> {
        anyhow::ensure!(
            self.inspection_errors.is_empty(),
            "could not inspect inactive APM fragment sources: {}",
            self.inspection_errors.join("; ")
        );
        Ok(&self.sources)
    }
}

/// Discover `*.yml` and `*.yaml` files in `~/.apm/config/`.
///
/// Returns an empty vector if the directory does not exist.  Results are
/// sorted by path so the merged manifest is deterministic regardless of the
/// filesystem's enumeration order.
#[cfg(test)]
pub(super) fn discover_fragment_files(home: &Path) -> Result<Vec<PathBuf>> {
    discover_yaml_files(&home.join(".apm").join("config"))
}

/// Discover the fragment files that the symlink task makes effective.
///
/// Managed source paths replace home entries with the same target name. This
/// lets dry runs evaluate the post-symlink desired state without mutating the
/// home directory, while preserving unmanaged fragments already present there.
pub(super) fn discover_effective_fragment_files(
    home: &Path,
    managed: &[ApmFragmentSource],
    inactive: &[ApmFragmentSource],
) -> Result<Vec<PathBuf>> {
    let config = home.join(".apm/config");
    let directory = PlannedDirectory::resolve(&config)?;
    let in_config = |fragment: &ApmFragmentSource| -> Result<bool> {
        let parent = fragment
            .target
            .parent()
            .context("fragment target has no parent")?;
        directory.same_directory(&PlannedDirectory::resolve(&home.join(parent))?)
    };
    let mut fragments = BTreeMap::new();
    let mut active = HashSet::new();
    for fragment in managed {
        if in_config(fragment)? {
            let target = home.join(&fragment.target);
            anyhow::ensure!(
                active.insert(entry_name(&target)?),
                "ambiguous active APM fragment target {}",
                target.display()
            );
            let name = target
                .file_name()
                .context("fragment target has no filename")?;
            fragments.insert(name.to_owned(), fragment.source.clone());
        }
    }
    let mut retired: HashMap<OsString, Vec<&Path>> = HashMap::new();
    for fragment in inactive {
        if in_config(fragment)? {
            retired
                .entry(entry_name(&home.join(&fragment.target))?)
                .or_default()
                .push(&fragment.source);
        }
    }
    let home_fragments = discover_yaml_files_filtered(&config, |path| {
        // read_dir gives us the stored name, including for dangling links.
        let name = path.file_name().context("fragment entry has no filename")?;
        if active.contains(name) {
            return Ok(true);
        }
        let Some(matching) = retired.get(name) else {
            return Ok(false);
        };
        let metadata = std::fs::symlink_metadata(path)
            .with_context(|| format!("checking retained APM fragment {}", path.display()))?;
        if !metadata.file_type().is_symlink() {
            return Ok(false);
        }
        let source = std::fs::read_link(path)
            .with_context(|| format!("reading retained APM fragment {}", path.display()))?;
        for configured in matching {
            if link_points_to_source(path, &source, configured)? {
                return Ok(true);
            }
        }
        Ok(false)
    })?;
    for path in home_fragments {
        if let Some(name) = path.file_name() {
            fragments.insert(name.to_os_string(), path);
        }
    }

    Ok(fragments.into_values().collect())
}

/// Discover YAML files in an APM fragment directory.
pub(super) fn discover_yaml_files(config_dir: &Path) -> Result<Vec<PathBuf>> {
    discover_yaml_files_filtered(config_dir, |_| Ok(false))
}

fn discover_yaml_files_filtered(
    config_dir: &Path,
    skip: impl Fn(&Path) -> Result<bool>,
) -> Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(config_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            // A dangling parent link is not an absent config directory.
            PlannedDirectory::resolve(config_dir).with_context(|| {
                format!("reading APM config directory {}", config_dir.display())
            })?;
            return Ok(Vec::new());
        }
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
        if !is_yaml_fragment(&path) || skip(&path)? {
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
    let mut root = Mapping::new();
    root.insert("name".into(), Value::from("dotfiles"));
    root.insert("version".into(), Value::from("1.0.0"));

    let mut seen_dependencies = HashMap::new();

    for path in fragments {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("reading manifest fragment {}", path.display()))?;
        if content.trim().is_empty() {
            continue;
        }
        let value: Value = crate::infra::yaml::from_str(&content)
            .with_context(|| format!("parsing manifest fragment {}", path.display()))?;

        let mapping = value.as_object().with_context(|| {
            format!("manifest fragment {} is not a YAML mapping", path.display())
        })?;

        for (key, fragment_value) in mapping {
            match key.as_str() {
                "name" | "version" => {}
                section @ ("dependencies" | "devDependencies") => merge_dependency_section(
                    &mut root,
                    section,
                    fragment_value,
                    seen_dependencies.entry(section.to_owned()).or_default(),
                    path,
                )?,
                _ => merge_manifest_value(&mut root, key, fragment_value),
            }
        }
    }

    let body = crate::infra::yaml::to_string(&Value::Object(root))
        .context("serialising merged apm manifest")?;
    Ok(format!("{GENERATED_HEADER}{body}"))
}

/// Merge a top-level non-dependency manifest field into the generated root.
fn merge_manifest_value(root: &mut Mapping, key: &str, incoming: &Value) {
    match root.get_mut(key) {
        Some(existing) => merge_layered_value(existing, incoming),
        None => {
            root.insert(key.to_string(), incoming.clone());
        }
    }
}

/// Merge one layered YAML value into another.
fn merge_layered_value(existing: &mut Value, incoming: &Value) {
    match (existing, incoming) {
        (Value::Object(existing_map), Value::Object(incoming_map)) => {
            for (key, value) in incoming_map {
                merge_manifest_value(existing_map, key, value);
            }
        }
        (Value::Array(existing_items), Value::Array(incoming_items)) => {
            append_unique_values(existing_items, incoming_items);
        }
        (existing_value, incoming_value) => {
            *existing_value = incoming_value.clone();
        }
    }
}

/// Append values from `incoming` that are not already present in `existing`.
fn append_unique_values(existing: &mut Vec<Value>, incoming: &[Value]) {
    let mut seen: HashSet<String> = existing.iter().map(value_dedup_key).collect();
    for entry in incoming {
        if seen.insert(value_dedup_key(entry)) {
            existing.push(entry.clone());
        }
    }
}

/// Merge one dependency section (`dependencies` or `devDependencies`).
fn merge_dependency_section(
    root: &mut Mapping,
    section_name: &str,
    section: &Value,
    seen: &mut HashMap<String, HashSet<String>>,
    fragment: &Path,
) -> Result<()> {
    let groups = section.as_object().with_context(|| {
        format!(
            "{section_name} in manifest fragment {} must be a YAML mapping",
            fragment.display()
        )
    })?;
    let target_section = ensure_mapping_field(root, section_name)?;

    for (kind_value, entries_value) in groups {
        let kind = kind_value.as_str();
        let entries = entries_value.as_array().with_context(|| {
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
fn ensure_mapping_field<'a>(mapping: &'a mut Mapping, field: &str) -> Result<&'a mut Mapping> {
    let Value::Object(value) = mapping
        .entry(field.to_string())
        .or_insert_with(|| Value::Object(Mapping::new()))
    else {
        anyhow::bail!("generated APM manifest field {field} is not a mapping");
    };
    Ok(value)
}

/// Return a sequence field, creating it if needed.
fn ensure_sequence_field<'a>(mapping: &'a mut Mapping, field: &str) -> Result<&'a mut Vec<Value>> {
    let Value::Array(value) = mapping
        .entry(field.to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
    else {
        anyhow::bail!("generated APM manifest dependency group {field} is not a sequence");
    };
    Ok(value)
}

/// Deduplicate MCP entries by name (or serialized shorthand), and other kinds
/// by their serialized value.
fn dependency_dedup_key(kind: &str, entry: &Value) -> String {
    if kind == "mcp" {
        entry.get("name").and_then(Value::as_str).map_or_else(
            || format!("@{}", value_dedup_key(entry)),
            |name| format!("name:{name}"),
        )
    } else {
        value_dedup_key(entry)
    }
}

/// Deduplication key for a generic YAML value: its serialized representation.
fn value_dedup_key(entry: &Value) -> String {
    crate::infra::yaml::to_string(entry).unwrap_or_else(|_| format!("{entry:?}"))
}

#[cfg(test)]
#[path = "tests/fragments.rs"]
mod tests;
