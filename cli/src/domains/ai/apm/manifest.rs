//! Manifest fingerprinting, marker files, and merged-manifest writes.

use anyhow::{Context as _, Result};
use sha2::{Digest as _, Sha256};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Build a stable fingerprint for the generated manifest content.
pub(super) fn manifest_fingerprint(content: &str) -> String {
    let mut hash = String::with_capacity(64);
    for byte in Sha256::digest(content.as_bytes()) {
        hash.push(hex_nibble(byte >> 4));
        hash.push(hex_nibble(byte & 0x0f));
    }
    hash
}

/// Convert a 4-bit value to a lowercase hexadecimal character.
const fn hex_nibble(nibble: u8) -> char {
    match nibble {
        0 => '0',
        1 => '1',
        2 => '2',
        3 => '3',
        4 => '4',
        5 => '5',
        6 => '6',
        7 => '7',
        8 => '8',
        9 => '9',
        10 => 'a',
        11 => 'b',
        12 => 'c',
        13 => 'd',
        14 => 'e',
        15 => 'f',
        _ => '?',
    }
}

/// Return whether the marker records a successful install for this manifest.
pub(super) fn manifest_marker_matches(marker: &Path, manifest_hash: &str) -> Result<bool> {
    match std::fs::read_to_string(marker) {
        Ok(existing) => Ok(existing.trim() == manifest_hash),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err)
            .with_context(|| format!("reading APM manifest install marker {}", marker.display())),
    }
}

/// Record that APM successfully installed the current generated manifest.
pub(super) fn write_manifest_marker(marker: &Path, manifest_hash: &str) -> Result<()> {
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }
    std::fs::write(marker, format!("{manifest_hash}\n"))
        .with_context(|| format!("writing APM manifest install marker {}", marker.display()))
}

/// Build a human-facing phrase describing the dependencies in the merged
/// manifest, e.g. `"3 APM dependencies"` or `"1 APM dependency"`.
///
/// Used for the always-visible install change line so the console output
/// matches the `    {verb}: {desc}` style emitted by the other apply tasks.
/// Falls back to `"APM manifest"` when the count cannot be determined.
pub(super) fn describe_dependencies(merged: &str) -> String {
    match count_manifest_dependencies(merged) {
        Some(1) => "1 APM dependency".to_string(),
        Some(n) => format!("{n} APM dependencies"),
        None => "APM manifest".to_string(),
    }
}

/// Count all dependency entries in the merged manifest.
///
/// Both `dependencies` and `devDependencies` are included, and every dependency
/// group under each section counts. Returns `None` if the manifest cannot be
/// parsed or has no dependency sections.
fn count_manifest_dependencies(merged: &str) -> Option<usize> {
    use serde_yaml_ng::Value;

    let value: Value = match serde_yaml_ng::from_str(merged) {
        Ok(value) => value,
        Err(err) => {
            // A genuine parse failure is worth surfacing at debug level; the
            // caller still falls back to the generic "APM manifest" phrase.
            tracing::debug!("failed to parse merged manifest for dependency count: {err}");
            return None;
        }
    };
    let count = ["dependencies", "devDependencies"]
        .iter()
        .filter_map(|section| value.get(section).and_then(Value::as_mapping))
        .flat_map(serde_yaml_ng::Mapping::values)
        .filter_map(Value::as_sequence)
        .map(Vec::len)
        .sum();
    (count > 0).then_some(count)
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
    let mut guard = crate::infra::fs::TempPath::new(tmp.clone());
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
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::super::fragments::merge_fragments;
    use super::*;

    #[test]
    fn describe_dependencies_counts_apm_and_mcp_entries() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let f = dir.path().join("deps.yml");
        std::fs::write(
            &f,
            "\
name: d
version: 1.0.0
dependencies:
  apm:
    - foo/bar
    - baz/qux
  mcp:
    - server-1
  lsp:
    - rust-analyzer
devDependencies:
  apm:
    - ./dev/package
",
        )
        .expect("write");
        let merged = merge_fragments(&[f]).expect("merge");
        assert_eq!(count_manifest_dependencies(&merged), Some(5));
        assert_eq!(describe_dependencies(&merged), "5 APM dependencies");
    }

    #[test]
    fn describe_dependencies_uses_singular_for_one_entry() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let f = dir.path().join("one.yml");
        std::fs::write(
            &f,
            "name: o\nversion: 1.0.0\ndependencies:\n  apm:\n    - foo/bar\n",
        )
        .expect("write");
        let merged = merge_fragments(&[f]).expect("merge");
        assert_eq!(describe_dependencies(&merged), "1 APM dependency");
    }

    #[test]
    fn describe_dependencies_falls_back_when_no_dependencies() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let f = dir.path().join("none.yml");
        std::fs::write(&f, "name: n\nversion: 1.0.0\n").expect("write");
        let merged = merge_fragments(&[f]).expect("merge");
        assert_eq!(count_manifest_dependencies(&merged), None);
        assert_eq!(describe_dependencies(&merged), "APM manifest");
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
