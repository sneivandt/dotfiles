//! Generated-manifest persistence and dependency descriptions.

use anyhow::{Context as _, Result};
use serde::Deserialize;
use serde_yaml_ng::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Read the authoritative APM lockfile for before/after change detection.
///
/// APM 0.27+ preserves unchanged target mappings and timestamps, so exact bytes
/// now represent meaningful convergence state without custom normalization.
pub(super) fn read_lock_snapshot(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("reading APM lockfile {}", path.display())),
    }
}

/// Describe dependency-level changes between two APM lockfile snapshots.
///
/// The lockfile is APM's authoritative record of resolved packages and
/// deployments. Parsing it avoids treating install chatter such as cached,
/// unchanged packages as state changes. Unknown lock fields remain part of the
/// comparison so a newer APM can still produce a conservative metadata-change
/// description instead of silently hiding a dependency change.
pub(super) fn describe_lock_changes(before: Option<&[u8]>, after: Option<&[u8]>) -> Vec<String> {
    let Some(before) = parse_locked_dependencies(before) else {
        return Vec::new();
    };
    let Some(after) = parse_locked_dependencies(after) else {
        return Vec::new();
    };

    let keys: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    keys.into_iter()
        .filter_map(|key| match (before.get(key), after.get(key)) {
            (None, Some(dependency)) => Some(format!("installed: {}", dependency.display_name())),
            (Some(dependency), None) => Some(format!("removed: {}", dependency.display_name())),
            (Some(before), Some(after)) if before != after => {
                Some(describe_dependency_update(before, after))
            }
            (Some(_), Some(_)) | (None, None) => None,
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct ApmLock {
    #[serde(default)]
    dependencies: Vec<LockedDependency>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct LockedDependency {
    #[serde(default)]
    repo_url: String,
    #[serde(default)]
    name: String,
    virtual_path: Option<String>,
    local_path: Option<String>,
    resolved_commit: Option<String>,
    resolved_ref: Option<String>,
    version: Option<String>,
    content_hash: Option<String>,
    #[serde(default)]
    deployed_files: Vec<String>,
    #[serde(default)]
    deployed_file_hashes: BTreeMap<String, String>,
    target_subset: Option<Vec<String>>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl LockedDependency {
    fn identity(&self) -> String {
        format!(
            "{}\0{}\0{}\0{}",
            self.repo_url,
            self.virtual_path.as_deref().unwrap_or_default(),
            self.local_path.as_deref().unwrap_or_default(),
            self.name
        )
    }

    fn display_name(&self) -> String {
        if let Some(virtual_path) = self.virtual_path.as_deref() {
            return join_package_path(&self.repo_url, virtual_path);
        }
        if !self.name.is_empty() {
            return self.name.clone();
        }
        if let Some(local_path) = self.local_path.as_deref() {
            return local_path.to_string();
        }
        if !self.repo_url.is_empty() {
            return self.repo_url.clone();
        }
        "unnamed APM dependency".to_string()
    }
}

fn parse_locked_dependencies(bytes: Option<&[u8]>) -> Option<BTreeMap<String, LockedDependency>> {
    let lock = match bytes {
        Some(bytes) => serde_yaml_ng::from_slice::<ApmLock>(bytes).ok()?,
        None => ApmLock {
            dependencies: Vec::new(),
        },
    };
    Some(
        lock.dependencies
            .into_iter()
            .map(|dependency| (dependency.identity(), dependency))
            .collect(),
    )
}

fn describe_dependency_update(before: &LockedDependency, after: &LockedDependency) -> String {
    let mut changes = Vec::new();
    if before.version != after.version {
        changes.push(format!(
            "version {} -> {}",
            display_value(before.version.as_deref()),
            display_value(after.version.as_deref())
        ));
    }
    if before.resolved_ref != after.resolved_ref {
        changes.push(format!(
            "ref {} -> {}",
            display_value(before.resolved_ref.as_deref()),
            display_value(after.resolved_ref.as_deref())
        ));
    }
    if before.resolved_commit != after.resolved_commit {
        changes.push(format!(
            "commit {} -> {}",
            display_revision(before.resolved_commit.as_deref()),
            display_revision(after.resolved_commit.as_deref())
        ));
    }

    let content_changed = before.content_hash != after.content_hash
        || before.deployed_file_hashes != after.deployed_file_hashes;
    if content_changed && before.resolved_commit == after.resolved_commit {
        changes.push("content changed".to_string());
    }
    if before.deployed_files != after.deployed_files {
        changes.push("deployed files changed".to_string());
    }
    if before.target_subset != after.target_subset {
        changes.push("targets changed".to_string());
    }
    if changes.is_empty() {
        changes.push("lock metadata changed".to_string());
    }

    format!(
        "updated: {} · {}",
        after.display_name(),
        changes.join(" · ")
    )
}

fn display_value(value: Option<&str>) -> &str {
    value.filter(|value| !value.is_empty()).unwrap_or("-")
}

fn display_revision(value: Option<&str>) -> &str {
    let value = display_value(value);
    value.get(..7).unwrap_or(value)
}

fn join_package_path(repo_url: &str, virtual_path: &str) -> String {
    match (
        repo_url.trim_end_matches('/'),
        virtual_path.trim_start_matches('/'),
    ) {
        ("", "") => "unnamed APM dependency".to_string(),
        ("", path) | (path, "") => path.to_string(),
        (repo, path) => format!("{repo}/{path}"),
    }
}

/// Return whether the generated manifest should be written to disk.
///
/// # Errors
///
/// Returns an error if `target` is a symlink (it must be removed first) or if
/// the existing file's content or metadata cannot be read.
pub(super) fn merged_manifest_needs_write(target: &Path, content: &str) -> Result<bool> {
    match std::fs::symlink_metadata(target) {
        Ok(meta) if meta.file_type().is_symlink() => {
            anyhow::bail!(
                "merged manifest target is a symlink; remove it before continuing: {}",
                target.display()
            );
        }
        Ok(meta) if meta.is_file() => {
            let existing = std::fs::read(target).with_context(|| {
                format!("reading existing merged manifest {}", target.display())
            })?;
            Ok(existing != content.as_bytes())
        }
        Ok(_) => Ok(true),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(true),
        Err(err) => Err(err)
            .with_context(|| format!("reading metadata for merged manifest {}", target.display())),
    }
}

/// Write the merged manifest to `target`, replacing any existing file.
///
/// Returns early without writing if the existing file already has identical
/// content, so we avoid bumping the mtime on unchanged runs.  The write is
/// staged to a sibling temp file and renamed into place so a concurrent reader
/// never observes a partially written manifest.
///
/// # Errors
///
/// Returns an error if `target` is a symlink, if the existing file cannot be
/// read for comparison, or if the temp file cannot be written or renamed.
pub(super) fn write_merged_manifest(target: &Path, content: &str) -> Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }

    // Reuse the same symlink-bail / read-compare logic the planner uses so the
    // two paths cannot drift. Skips the write entirely when content matches.
    if !merged_manifest_needs_write(target, content)? {
        return Ok(());
    }

    let tmp = manifest_temp_path(target);
    std::fs::write(&tmp, content)
        .with_context(|| format!("writing temporary merged manifest {}", tmp.display()))?;
    let mut guard = crate::infra::fs::TempGuard::file(tmp.clone());
    std::fs::rename(&tmp, target).with_context(|| {
        format!(
            "renaming {} into place at {}",
            tmp.display(),
            target.display()
        )
    })?;
    guard.persist();
    Ok(())
}

/// Build the sibling temp path used to stage an atomic manifest write.
///
/// Keeping the temp file in the same directory as `target` guarantees the
/// subsequent rename stays on one filesystem and is therefore atomic.
fn manifest_temp_path(target: &Path) -> PathBuf {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let name = target.file_name().map_or_else(
        || "dotfiles_apm_tmp".to_string(),
        |n| format!("{}.dotfiles_tmp", n.to_string_lossy()),
    );
    parent.join(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_changes_name_remote_ref_and_commit_updates() {
        let before = b"\
dependencies:
  - repo_url: cursor/plugins
    name: unslop
    virtual_path: pstack/skills/unslop
    resolved_commit: efa2a531abcd
    version: unknown
";
        let after = b"\
dependencies:
  - repo_url: cursor/plugins
    name: unslop
    virtual_path: pstack/skills/unslop
    resolved_ref: main
    resolved_commit: 93b00b89abcd
    version: unknown
";

        assert_eq!(
            describe_lock_changes(Some(before), Some(after)),
            [
                "updated: cursor/plugins/pstack/skills/unslop · ref - -> main · commit \
                 efa2a53 -> 93b00b8"
            ]
        );
    }

    #[test]
    fn lock_changes_distinguish_added_removed_and_local_content() {
        let before = b"\
dependencies:
  - repo_url: old/plugin
    name: old-plugin
  - repo_url: _local/dot-agent
    name: dot-agent
    source: local
    local_path: ~/.apm/plugins/dot-agent
    deployed_file_hashes:
      .agents/skills/status-update/SKILL.md: sha256:old
";
        let after = b"\
dependencies:
  - repo_url: new/plugin
    name: new-plugin
  - repo_url: _local/dot-agent
    name: dot-agent
    source: local
    local_path: ~/.apm/plugins/dot-agent
    deployed_file_hashes:
      .agents/skills/status-update/SKILL.md: sha256:new
";

        let changes = describe_lock_changes(Some(before), Some(after));
        assert!(changes.contains(&"removed: old-plugin".to_string()));
        assert!(changes.contains(&"installed: new-plugin".to_string()));
        assert!(changes.contains(&"updated: dot-agent · content changed".to_string()));
    }

    #[test]
    fn lock_changes_fall_back_when_a_snapshot_cannot_be_parsed() {
        assert!(describe_lock_changes(Some(b"not: [yaml"), Some(b"dependencies: []")).is_empty());
    }

    #[test]
    fn write_merged_manifest_errors_when_target_is_symlink() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let target = dir.path().join("apm.yml");
        let source = dir.path().join("source.yml");
        std::fs::write(&source, "old\n").expect("write source");

        // Skip on platforms where unprivileged symlink creation isn't
        // available (Windows without developer mode).
        #[cfg(unix)]
        std::os::unix::fs::symlink(&source, &target).expect("symlink");
        #[cfg(windows)]
        if std::os::windows::fs::symlink_file(&source, &target).is_err() {
            return;
        }

        let result = write_merged_manifest(&target, "new content\n");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("symlink"),
            "error should identify symlink target: {msg}"
        );
        let meta = std::fs::symlink_metadata(&target).expect("stat");
        assert!(
            meta.file_type().is_symlink(),
            "symlink should be left untouched"
        );
    }

    #[test]
    fn write_merged_manifest_skips_rewrite_when_unchanged() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let target = dir.path().join("apm.yml");
        std::fs::write(&target, "same\n").expect("seed");
        let mtime_before = std::fs::metadata(&target)
            .expect("stat")
            .modified()
            .expect("mtime");
        // Sleep briefly so a rewrite would change mtime measurably.
        std::thread::sleep(std::time::Duration::from_millis(10));
        write_merged_manifest(&target, "same\n").expect("write");
        let mtime_after = std::fs::metadata(&target)
            .expect("stat")
            .modified()
            .expect("mtime");
        assert_eq!(mtime_before, mtime_after);
    }
}
