//! Content fingerprinting for the inputs that decide whether `apm install`
//! has real work to do.
//!
//! The merged manifest alone is not enough. Dependencies declared as
//! filesystem paths — the `dot-*` plugins this repo ships into
//! `~/.apm/plugins/` — are symlinked straight at the repository, so editing a
//! skill changes what APM must deploy without changing a single byte of
//! `~/.apm/apm.yml`. The resolved target set matters for the same reason:
//! installing Copilot App or reconciling Cowork adds managed targets that an
//! unchanged manifest would otherwise never be redeployed into.
//!
//! Folding all three into one fingerprint is what lets
//! [`super::install::InstallApmPackages`] skip a redundant multi-second `apm
//! install` on a converged tree while still redeploying edited plugin sources.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use sha2::{Digest as _, Sha256};

use super::manifest::hex_digest;
use super::targets::ApmTargets;

/// Directory depth walked when hashing a local dependency source.
///
/// Local plugins are shallow by construction; the cap exists so a symlink
/// cycle inside a plugin tree cannot spin the walk forever.
const MAX_DEPTH: usize = 16;

/// Directory names never folded into a source fingerprint.
///
/// These hold derived or vendored content whose churn says nothing about what
/// APM would deploy, and walking them would dominate the hash cost.
const IGNORED_DIRS: &[&str] = &[".git", "node_modules", "target"];
const FINGERPRINT_VERSION: &[u8] = b"apm-install-plan-v4\n";

/// Build the fingerprint recorded in the APM install marker.
///
/// Covers the merged manifest, the on-disk content of every local dependency
/// source it references, and whether the explicit Copilot App install and
/// managed Cowork reconciliation are in play, so a matching marker means "the
/// last successful install already deployed exactly this".
///
/// # Errors
///
/// Propagates IO errors from reading a local dependency source that exists but
/// cannot be read. A source that is simply absent is folded in as a missing
/// marker rather than failing: APM itself reports that far better than a
/// fingerprint can.
pub(super) fn install_fingerprint(
    merged: &str,
    home: &Path,
    targets: ApmTargets,
) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(FINGERPRINT_VERSION);
    hasher.update(merged.as_bytes());
    for target in targets.active() {
        hasher.update(b"\ntarget:");
        hasher.update(target.apm_name().as_bytes());
    }
    hasher.update(b"\n");

    for path in local_dependency_paths(merged, home) {
        hasher.update(path.to_string_lossy().as_bytes());
        hasher.update(b"\n");
        hash_source(&path, &mut hasher, 0)?;
    }

    Ok(hex_digest(hasher))
}

/// Resolve every dependency entry that names a local filesystem source.
///
/// Returns a sorted, de-duplicated list so the fingerprint does not depend on
/// fragment or mapping iteration order. A manifest that cannot be parsed
/// yields no paths: the caller still hashes the raw manifest text, and APM
/// reports the parse failure with far better diagnostics than this could.
fn local_dependency_paths(merged: &str, home: &Path) -> Vec<PathBuf> {
    use serde_yaml_ng::Value;

    let value: Value = match serde_yaml_ng::from_str(merged) {
        Ok(value) => value,
        Err(err) => {
            tracing::debug!("failed to parse merged manifest for local source scan: {err}");
            return Vec::new();
        }
    };

    let mut paths: Vec<PathBuf> = ["dependencies", "devDependencies"]
        .iter()
        .filter_map(|section| value.get(section).and_then(Value::as_mapping))
        .flat_map(serde_yaml_ng::Mapping::values)
        .filter_map(Value::as_sequence)
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|entry| resolve_local_path(entry, home))
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

/// Resolve a dependency entry to an absolute path when it names a local source.
///
/// Registry coordinates such as `owner/repo` are deliberately excluded: they
/// are pinned by the lockfile, so their content cannot drift without the
/// manifest or lockfile changing, and probing them would be pointless IO.
/// Relative entries resolve against `~/.apm`, the directory holding the
/// generated manifest that declares them.
fn resolve_local_path(entry: &str, home: &Path) -> Option<PathBuf> {
    let entry = entry.trim();
    if entry.is_empty() {
        return None;
    }

    if let Some(rest) = entry
        .strip_prefix("~/")
        .or_else(|| entry.strip_prefix("~\\"))
    {
        return Some(home.join(rest));
    }

    if ["./", "../", ".\\", "..\\"]
        .iter()
        .any(|prefix| entry.starts_with(prefix))
    {
        return Some(home.join(".apm").join(entry));
    }

    let path = Path::new(entry);
    path.is_absolute().then(|| path.to_path_buf())
}

/// Fold a file's content, or a directory's tree, into `hasher`.
fn hash_source(path: &Path, hasher: &mut Sha256, depth: usize) -> Result<()> {
    // Follow symlinks deliberately: `~/.apm/plugins/dot-agent` *is* a symlink
    // into the repository, and its target's content is exactly what matters.
    let Ok(metadata) = std::fs::metadata(path) else {
        hasher.update(b"<absent>\n");
        return Ok(());
    };

    if metadata.is_file() {
        let bytes = std::fs::read(path)
            .with_context(|| format!("reading local APM dependency source {}", path.display()))?;
        hasher.update(b"<file>");
        hasher.update(bytes.len().to_le_bytes());
        hasher.update(&bytes);
        hasher.update(b"\n");
        return Ok(());
    }

    if !metadata.is_dir() {
        hasher.update(b"<other>\n");
        return Ok(());
    }

    if depth >= MAX_DEPTH {
        hasher.update(b"<truncated>\n");
        return Ok(());
    }

    hasher.update(b"<dir>\n");
    for child in sorted_children(path)? {
        let name = child
            .file_name()
            .map(OsStr::to_string_lossy)
            .unwrap_or_default();
        hasher.update(name.as_bytes());
        hasher.update(b"\n");
        hash_source(&child, hasher, depth.saturating_add(1))?;
    }
    Ok(())
}

/// List a directory's entries in a stable order, dropping ignored directories.
fn sorted_children(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("reading local APM dependency directory {}", dir.display()))?;

    let mut children = Vec::new();
    for entry in entries {
        let entry =
            entry.with_context(|| format!("reading directory entry in {}", dir.display()))?;
        let name = entry.file_name();
        if IGNORED_DIRS
            .iter()
            .any(|ignored| name == OsStr::new(ignored))
        {
            continue;
        }
        children.push(entry.path());
    }
    children.sort();
    Ok(children)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::ai::apm::targets::CopilotTarget;

    const MANIFEST: &str = "\
name: dotfiles
version: 1.0.0
dependencies:
  apm:
    - ~/.apm/plugins/dot-agent
    - owner/registry-package
";

    fn fingerprint(manifest: &str, home: &Path, targets: &[CopilotTarget]) -> Result<String> {
        install_fingerprint(manifest, home, ApmTargets::from_targets(targets))
    }

    fn seed_plugin(home: &Path, body: &str) {
        let plugin = home.join(".apm").join("plugins").join("dot-agent");
        std::fs::create_dir_all(plugin.join(".apm").join("skills").join("demo")).expect("mkdir");
        std::fs::write(plugin.join("apm.yml"), "name: dot-agent\n").expect("write manifest");
        std::fs::write(
            plugin
                .join(".apm")
                .join("skills")
                .join("demo")
                .join("SKILL.md"),
            body,
        )
        .expect("write skill");
    }

    #[test]
    fn local_paths_resolve_tilde_entries_and_ignore_registry_coordinates() {
        let home = Path::new("/home/u");
        let paths = local_dependency_paths(MANIFEST, home);
        assert_eq!(
            paths,
            vec![home.join(".apm/plugins/dot-agent")],
            "only the local plugin should be treated as a content source"
        );
    }

    #[test]
    fn relative_entries_resolve_against_the_manifest_directory() {
        let home = Path::new("/home/u");
        assert_eq!(
            resolve_local_path("./vendor/pkg", home),
            Some(home.join(".apm").join("./vendor/pkg")),
            "relative entries belong to the generated manifest's directory"
        );
    }

    #[test]
    fn registry_coordinates_are_not_local_paths() {
        let home = Path::new("/home/u");
        assert_eq!(resolve_local_path("owner/repo", home), None);
        assert_eq!(resolve_local_path("", home), None);
    }

    #[test]
    fn fingerprint_changes_when_a_local_plugin_file_changes() {
        // Regression guard for the whole point of this module: editing a
        // locally symlinked plugin must still trigger a redeploy even though
        // the merged manifest is byte-identical.
        let dir = tempfile::tempdir().expect("temp dir");
        let home = dir.path();
        seed_plugin(home, "# before\n");
        let before = fingerprint(MANIFEST, home, &[]).expect("fingerprint");

        seed_plugin(home, "# after\n");
        let after = fingerprint(MANIFEST, home, &[]).expect("fingerprint");

        assert_ne!(
            before, after,
            "an edited plugin source must change the install fingerprint"
        );
    }

    #[test]
    fn fingerprint_is_stable_for_an_unchanged_tree() {
        let dir = tempfile::tempdir().expect("temp dir");
        let home = dir.path();
        seed_plugin(home, "# body\n");
        let first = fingerprint(MANIFEST, home, &[]).expect("fingerprint");
        let second = fingerprint(MANIFEST, home, &[]).expect("fingerprint");
        assert_eq!(
            first, second,
            "repeated hashing of an unchanged tree must agree"
        );
    }

    #[test]
    fn fingerprint_changes_when_a_plugin_file_is_added() {
        let dir = tempfile::tempdir().expect("temp dir");
        let home = dir.path();
        seed_plugin(home, "# body\n");
        let before = fingerprint(MANIFEST, home, &[]).expect("fingerprint");

        std::fs::write(
            home.join(".apm")
                .join("plugins")
                .join("dot-agent")
                .join(".apm")
                .join("skills")
                .join("demo")
                .join("EXTRA.md"),
            "extra\n",
        )
        .expect("write");
        let after = fingerprint(MANIFEST, home, &[]).expect("fingerprint");

        assert_ne!(before, after, "a new plugin file must change the print");
    }

    #[test]
    fn fingerprint_changes_when_the_copilot_app_target_appears() {
        let dir = tempfile::tempdir().expect("temp dir");
        let home = dir.path();
        seed_plugin(home, "# body\n");

        let without = fingerprint(MANIFEST, home, &[]).expect("fingerprint");
        let with = fingerprint(MANIFEST, home, &[CopilotTarget::CopilotApp]).expect("fingerprint");

        assert_ne!(
            without, with,
            "gaining the Copilot App target must force a redeploy"
        );
    }

    #[test]
    fn fingerprint_changes_when_the_copilot_cowork_target_appears() {
        let dir = tempfile::tempdir().expect("temp dir");
        let home = dir.path();
        seed_plugin(home, "# body\n");

        let without = fingerprint(MANIFEST, home, &[]).expect("fingerprint");
        let with = fingerprint(MANIFEST, home, &[CopilotTarget::Cowork]).expect("fingerprint");

        assert_ne!(
            without, with,
            "gaining the Copilot Cowork target must force a redeploy"
        );
    }

    #[test]
    fn fingerprint_changes_when_the_manifest_changes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let home = dir.path();
        seed_plugin(home, "# body\n");

        let before = fingerprint(MANIFEST, home, &[]).expect("fingerprint");
        let after = fingerprint(
            &format!("{MANIFEST}    - owner/another-package\n"),
            home,
            &[],
        )
        .expect("fingerprint");

        assert_ne!(before, after, "manifest edits must still be detected");
    }

    #[test]
    fn missing_local_sources_do_not_fail_the_fingerprint() {
        let dir = tempfile::tempdir().expect("temp dir");
        let print = fingerprint(MANIFEST, dir.path(), &[])
            .expect("an absent plugin must not fail fingerprinting");
        assert_eq!(print.len(), 64, "fingerprint should be a sha256 hex digest");
    }

    #[test]
    fn ignored_directories_are_not_walked() {
        let dir = tempfile::tempdir().expect("temp dir");
        let home = dir.path();
        seed_plugin(home, "# body\n");
        let before = fingerprint(MANIFEST, home, &[]).expect("fingerprint");

        let git_dir = home
            .join(".apm")
            .join("plugins")
            .join("dot-agent")
            .join(".git");
        std::fs::create_dir_all(&git_dir).expect("mkdir");
        std::fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").expect("write");
        let after = fingerprint(MANIFEST, home, &[]).expect("fingerprint");

        assert_eq!(
            before, after,
            "repository metadata must not influence the deployment fingerprint"
        );
    }
}
