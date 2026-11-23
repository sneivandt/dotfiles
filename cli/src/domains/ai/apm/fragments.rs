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
        Ok(parent == Path::new(".apm/config")
            || directory.same_directory(&PlannedDirectory::resolve(&home.join(parent))?)?)
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
        (existing_value, incoming_value) if existing_value == incoming_value => {}
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
mod tests {
    use super::*;

    fn merge_test_fragments(contents: &[&str]) -> Result<String> {
        let dir = tempfile::tempdir().expect("create fragment directory");
        let paths: Vec<_> = contents
            .iter()
            .enumerate()
            .map(|(index, content)| {
                let path = dir.path().join(format!("{index}.yml"));
                std::fs::write(&path, content).expect("write fragment");
                path
            })
            .collect();
        merge_fragments(&paths)
    }

    #[test]
    fn documented_agent_yaml_examples_use_the_real_fragment_merger() {
        let document = include_str!("../../../../../docs/content/guides/agent-configuration.md");
        let examples: Vec<_> = document
            .split("```yaml")
            .skip(1)
            .map(|block| {
                let (_, body) = block.split_once('\n').expect("YAML fence header");
                body.split_once("```").expect("closed YAML fence").0
            })
            .collect();
        assert_eq!(
            examples.len(),
            2,
            "example plugin dependency and retained empty fragment"
        );
        for (index, example) in examples.iter().enumerate() {
            let merged = merge_test_fragments(&[example]).expect("merge documented fragment");
            let value: Value = crate::infra::yaml::from_str(&merged).expect("generated YAML");
            assert_eq!(value["name"], "dotfiles");
            assert_eq!(value["targets"].as_array().expect("targets").len(), 2);
            assert_eq!(
                value["dependencies"]["apm"]
                    .as_array()
                    .expect("APM dependencies")
                    .len(),
                usize::from(index == 0)
            );
        }
    }

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
    fn managed_fragment_shadows_only_the_same_target_identity() {
        let home = tempfile::tempdir().unwrap();
        let config = home.path().join(".apm/config");
        std::fs::create_dir_all(&config).unwrap();
        let user = config.join("Team.yml");
        std::fs::write(&user, "description: home").unwrap();
        let selected = home.path().join("source.yml");
        std::fs::write(&selected, "description: selected").unwrap();
        let managed = [ApmFragmentSource::new(
            selected.clone(),
            ".apm/config/team.yml".into(),
        )];
        let files = discover_effective_fragment_files(home.path(), &managed, &[]).unwrap();
        if config.join("team.yml").try_exists().unwrap() {
            assert_eq!(files, [selected]);
        } else {
            assert_eq!(files, [user, selected]);
        }
    }

    fn check_coexisting_names(home: &Path, first: &str, second: &str) {
        let config = home.join(".apm/config");
        std::fs::create_dir_all(&config).unwrap();
        let selected = home.join("selected.yml");
        std::fs::write(&selected, "description: selected").unwrap();
        let managed = [ApmFragmentSource::new(
            selected.clone(),
            Path::new(".apm/config").join(first),
        )];
        std::fs::write(config.join(first), "description: old").unwrap();
        let user = config.join(second);
        std::fs::File::create_new(&user).expect("fixture names must physically coexist");
        assert_eq!(
            discover_effective_fragment_files(home, &managed, &[]).unwrap(),
            [selected, user]
        );
    }

    #[test]
    fn coexisting_kelvin_and_ascii_fragments_both_contribute() {
        let home = tempfile::tempdir().unwrap();
        check_coexisting_names(home.path(), "K.yml", "\u{212a}.yml");
    }

    #[cfg(unix)]
    #[test]
    fn coexisting_unix_case_variants_both_contribute() {
        let home = tempfile::tempdir().unwrap();
        check_coexisting_names(home.path(), "Team.yml", "team.yml");
    }

    #[cfg(windows)]
    #[test]
    fn native_case_sensitive_fragment_names_and_parent_directories() {
        let home = tempfile::tempdir().unwrap();
        if !crate::infra::fs::enable_fixture_case_sensitivity(home.path()).unwrap() {
            return;
        }
        let canonical = home.path().join(".apm/config");
        let separate = home.path().join(".APM/Config");
        let other = [ApmFragmentSource::new(
            home.path().join("other-source.yml"),
            ".APM/Config/Team.yml".into(),
        )];
        // A different planned path is not selected even before its directories exist.
        assert!(
            discover_effective_fragment_files(home.path(), &other, &[])
                .unwrap()
                .is_empty()
        );
        std::fs::create_dir_all(&separate).unwrap();
        std::fs::create_dir_all(&canonical).unwrap();
        if !crate::infra::fs::enable_fixture_case_sensitivity(&canonical).unwrap() {
            return;
        }
        check_coexisting_names(home.path(), "Team.yml", "team.yml");
        let files = discover_effective_fragment_files(home.path(), &other, &[]).unwrap();
        assert_eq!(
            files,
            [canonical.join("Team.yml"), canonical.join("team.yml")]
        );
    }

    #[cfg(windows)]
    #[test]
    fn missing_fragment_directory_recognizes_its_ordinary_windows_alias() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join("probe")).unwrap();
        if !home.path().join("PROBE").try_exists().unwrap() {
            return;
        }
        let source = home.path().join("selected.yml");
        let managed = [ApmFragmentSource::new(
            source.clone(),
            ".APM/Config/Team.yml".into(),
        )];
        assert_eq!(
            discover_effective_fragment_files(home.path(), &managed, &[]).unwrap(),
            [source]
        );
        assert!(!home.path().join(".apm").try_exists().unwrap());
    }

    #[test]
    fn inactive_fragment_pointing_to_a_distinct_source_is_not_owned() {
        let home = tempfile::tempdir().unwrap();
        let config = home.path().join(".apm/config");
        std::fs::create_dir_all(&config).unwrap();
        let retired = home.path().join("K.yml");
        let user = home.path().join("\u{212a}.yml");
        std::fs::write(&retired, "retired").unwrap();
        // Sharing file contents/identity is not sufficient to own a different entry.
        std::fs::hard_link(&retired, &user).unwrap();
        let link = config.join("old.yml");
        crate::infra::fs::create_native_symlink(&user, &link, false).unwrap();
        let original = std::fs::read_link(&link).unwrap();
        let inactive = [ApmFragmentSource::new(
            retired,
            ".apm/config/old.yml".into(),
        )];
        assert_eq!(
            discover_effective_fragment_files(home.path(), &[], &inactive).unwrap(),
            std::slice::from_ref(&link)
        );
        assert_eq!(std::fs::read_link(&link).unwrap(), original);
    }

    #[test]
    fn fragment_discovery_fails_on_an_uninspectable_parent() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join(".apm"), "preserve").unwrap();
        let managed = [ApmFragmentSource::new(
            home.path().join("selected"),
            ".apm/config/Team.yml".into(),
        )];
        assert!(discover_effective_fragment_files(home.path(), &managed, &[]).is_err());
        assert_eq!(
            std::fs::read(home.path().join(".apm")).unwrap(),
            b"preserve"
        );
    }

    #[test]
    fn effective_fragments_replace_broken_managed_links_before_metadata_checks() {
        let dir = tempfile::tempdir_in(".").expect("create fixture directory");
        let home = crate::infra::fs::canonicalize(dir.path()).expect("resolve fixture home");
        let config = home.join(".apm").join("config");
        std::fs::create_dir_all(&config).expect("create fragment directory");
        let stale_link = config.join("base.yml");
        let missing = home.join("removed-source.yml");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&missing, &stale_link).expect("create stale fragment link");
        #[cfg(windows)]
        if std::os::windows::fs::symlink_file(&missing, &stale_link).is_err() {
            return;
        }

        let source = home.join("new-source.yml");
        std::fs::write(&source, "description: replacement\n").expect("write active source");
        let unmanaged = config.join("user.yml");
        std::fs::write(&unmanaged, "description: user\n").expect("write unmanaged fragment");

        assert!(discover_effective_fragment_files(&home, &[], &[]).is_err());
        let effective = discover_effective_fragment_files(
            &home,
            &[ApmFragmentSource::new(
                source.clone(),
                ".apm/config/base.yml".into(),
            )],
            &[],
        )
        .expect("managed source must mask the stale home entry");

        assert_eq!(effective, [source, unmanaged]);
        assert_eq!(std::fs::read_link(&stale_link).unwrap(), missing);

        let declared_name = if config.join("BASE.YML").symlink_metadata().is_ok() {
            "BASE.YML"
        } else {
            "base.yml"
        };
        let inactive = [ApmFragmentSource::new(
            missing.clone(),
            Path::new(".apm/config").join(declared_name),
        )];
        assert_eq!(
            discover_effective_fragment_files(&home, &[], &inactive).unwrap(),
            [config.join("user.yml")]
        );
        std::fs::write(&missing, "description: inactive desktop\n").unwrap();
        assert_eq!(
            discover_effective_fragment_files(&home, &[], &inactive).unwrap(),
            [config.join("user.yml")]
        );
        assert_eq!(std::fs::read_link(&stale_link).unwrap(), missing);
    }

    #[test]
    fn merge_fragments_concatenates_apm_and_mcp_dependencies() {
        let merged = merge_test_fragments(&[
            "name: a\nversion: 1.0.0\ndependencies:\n  apm:\n    - foo/bar\n",
            "name: b\nversion: 1.0.0\ndependencies:\n  apm:\n    - baz/qux\n  mcp:\n    - server-1\n",
        ])
        .expect("merge");
        assert!(merged.starts_with(GENERATED_HEADER));
        assert!(merged.contains("foo/bar"));
        assert!(merged.contains("baz/qux"));
        assert!(merged.contains("server-1"));
        assert!(merged.contains("name: dotfiles"));
    }

    #[test]
    fn merge_fragments_deduplicates_apm_and_mcp_entries() {
        let merged = merge_test_fragments(&[
            "name: a\nversion: 1.0.0\ndependencies:\n  apm:\n    - foo/bar\n  mcp:\n    - name: kusto\n      command: agency\n",
            "name: b\nversion: 1.0.0\ndependencies:\n  apm:\n    - foo/bar\n  mcp:\n    - name: kusto\n      command: other\n",
        ])
        .expect("merge");
        assert_eq!(merged.matches("foo/bar").count(), 1);
        assert_eq!(merged.matches("name: kusto").count(), 1);
        // First occurrence wins, so the duplicate's command is dropped.
        assert!(merged.contains("agency"));
        assert!(!merged.contains("other"));
    }

    #[test]
    fn merge_fragments_deduplicates_dependency_sections_independently() {
        let fragment = "\
dependencies:
  apm:
    - example/plugin
devDependencies:
  apm:
    - example/plugin
";
        let merged = merge_test_fragments(&[fragment, fragment]).expect("merge");
        let document: Value = crate::infra::yaml::from_str(&merged).expect("parse merged manifest");
        for section in ["dependencies", "devDependencies"] {
            assert_eq!(
                document[section]["apm"],
                Value::Array(vec![Value::from("example/plugin")]),
                "{section} must retain its own deduplicated dependency"
            );
        }
    }

    #[test]
    fn merge_fragments_preserves_complex_map_entries() {
        let merged = merge_test_fragments(&[
            "name: x\nversion: 1.0.0\ndependencies:\n  apm:\n    - git: dev.azure.com/org/repo\n      path: services/foo\n",
        ])
        .expect("merge");
        assert!(merged.contains("dev.azure.com/org/repo"));
        assert!(merged.contains("services/foo"));
    }

    #[test]
    fn merge_fragments_preserves_newer_manifest_fields() {
        let merged = merge_test_fragments(&[
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
        ])
        .expect("merge");

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
        let merged = merge_test_fragments(&[
            "description: base description\n",
            "description: overlay description\n",
        ])
        .expect("merge");

        assert!(merged.contains("description: overlay description"));
        assert!(!merged.contains("description: base description"));
    }

    #[test]
    fn merge_fragments_errors_when_dependency_section_is_not_mapping() {
        let err = merge_test_fragments(&["dependencies: []\n"])
            .expect_err("dependencies must be a mapping");

        assert!(
            format!("{err:#}").contains("dependencies in manifest fragment"),
            "expected dependency section context, got {err:#}"
        );
    }

    #[test]
    fn merge_fragments_errors_when_dependency_group_is_not_sequence() {
        let err = merge_test_fragments(&["dependencies:\n  apm: foo/bar\n"])
            .expect_err("dependencies.apm must be a sequence");

        assert!(
            format!("{err:#}").contains("dependencies.apm"),
            "expected dependency group context, got {err:#}"
        );
    }

    #[test]
    fn merge_fragments_skips_empty_files() {
        let merged = merge_test_fragments(&["\n\n"]).expect("merge");
        assert!(merged.contains("name: dotfiles"));
        assert!(!merged.contains("dependencies:"));
    }
}
