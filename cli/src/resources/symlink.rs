//! Symlink resource.
use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{Applicable, Resource, ResourceChange, ResourceState};
use crate::exec::Executor;

/// A symlink resource that can be checked and applied.
#[derive(Debug, Clone)]
pub struct SymlinkResource {
    /// The source file/directory (what the symlink points to).
    pub source: PathBuf,
    /// The target path (where the symlink will be created).
    pub target: PathBuf,
    /// Executor used for subprocess fallbacks (e.g. mklink on Windows).
    executor: Arc<dyn Executor>,
}

impl SymlinkResource {
    /// Create a new symlink resource.
    #[must_use]
    pub fn new(source: PathBuf, target: PathBuf, executor: Arc<dyn Executor>) -> Self {
        Self {
            source,
            target,
            executor,
        }
    }
}

impl Applicable for SymlinkResource {
    fn description(&self) -> String {
        format!("{} -> {}", self.target.display(), self.source.display())
    }

    fn apply(&self) -> Result<ResourceChange> {
        crate::fs::ensure_parent_dir(&self.target)?;

        // Attempt to remove any existing target; ignore NotFound since the
        // path may already be absent.  This avoids a TOCTOU race between a
        // separate existence check and the removal.
        match remove_symlink(&self.target) {
            Ok(()) => {}
            Err(e)
                if e.downcast_ref::<std::io::Error>()
                    .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) =>
            {
                // Path was already absent — nothing to remove.
            }
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("remove existing: {}", self.target.display()));
            }
        }

        // Create the symlink
        create_symlink(&self.source, &self.target, &*self.executor)
            .with_context(|| format!("create link: {}", self.target.display()))?;

        Ok(ResourceChange::Applied)
    }

    fn remove(&self) -> Result<ResourceChange> {
        // Refuse to overwrite a target that is not a symlink/junction.  If the
        // user replaced the managed symlink with their own real file or directory
        // we must not silently destroy their content during uninstall.
        match self.target.symlink_metadata() {
            Ok(meta) if !meta.is_symlink() => {
                return Ok(ResourceChange::Skipped {
                    reason: format!(
                        "target is not a symlink and will not be overwritten: {}",
                        self.target.display()
                    ),
                });
            }
            _ => {}
        }

        // Copy source content into place, then remove the symlink, so the user
        // retains the file/directory after uninstall instead of losing it.
        copy_into_place(&self.source, &self.target).with_context(|| {
            format!(
                "materialize {} -> {}",
                self.target.display(),
                self.source.display()
            )
        })?;
        Ok(ResourceChange::Applied)
    }
}

impl Resource for SymlinkResource {
    fn current_state(&self) -> Result<ResourceState> {
        // Check if source exists
        if !self.source.exists() {
            return Ok(ResourceState::Invalid {
                reason: format!("source does not exist: {}", self.source.display()),
            });
        }

        // Check if target is a real directory (not a symlink)
        if self.target.is_dir()
            && self
                .target
                .symlink_metadata()
                .map(|m| !m.is_symlink())
                .unwrap_or(false)
        {
            return Ok(ResourceState::Invalid {
                reason: "target is a real directory".to_string(),
            });
        }

        // Check if symlink already points to the correct source
        std::fs::read_link(&self.target).map_or_else(
            |_| {
                // Target doesn't exist or isn't a symlink.
                // Use symlink_metadata() (not exists()) so that dangling symlinks
                // — which exist on disk but point to a missing file — are detected
                // as Incorrect rather than Missing.
                if self.target.symlink_metadata().is_ok() {
                    Ok(ResourceState::Incorrect {
                        current: "target is a regular file or dangling symlink".to_string(),
                    })
                } else {
                    Ok(ResourceState::Missing)
                }
            },
            |existing| {
                if paths_equal(&existing, &self.source) {
                    Ok(ResourceState::Correct)
                } else {
                    Ok(ResourceState::Incorrect {
                        current: format!("points to {}", existing.display()),
                    })
                }
            },
        )
    }
}

/// Copy `source` into `target`, replacing the symlink that currently lives at
/// `target`.  Files are staged to a sibling temp path first so that the window
/// where `target` is absent is as small as possible.  Directories are handled
/// recursively via [`crate::fs::copy_dir_recursive`]; symlinks within the
/// source tree are recreated as symlinks rather than followed, preventing
/// unintended traversal outside the source tree.
fn copy_into_place(source: &Path, target: &Path) -> Result<()> {
    if source.is_dir() {
        copy_dir_into_place(source, target)
    } else {
        copy_file_into_place(source, target)
    }
}

/// Build a sibling temporary path by appending `suffix` to the target name.
fn sibling_temp_path(target: &Path, suffix: &str) -> PathBuf {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let name = target.file_name().map_or_else(
        || "dotfiles_tmp".to_string(),
        |n| format!("{}{suffix}", n.to_string_lossy()),
    );
    parent.join(name)
}

/// Copy a regular file: stage to a temp sibling, remove the symlink, rename
/// the temp file into place.
fn copy_file_into_place(source: &Path, target: &Path) -> Result<()> {
    let tmp = sibling_temp_path(target, ".dotfiles_tmp");
    std::fs::copy(source, &tmp)
        .with_context(|| format!("copy {} to {}", source.display(), tmp.display()))?;

    let mut guard = crate::fs::TempPath::new(tmp.clone());

    match target.symlink_metadata() {
        Ok(meta) if meta.is_symlink() => {
            remove_symlink(target)
                .with_context(|| format!("remove symlink: {}", target.display()))?;
        }
        Ok(_) => {
            // Target exists but is not a symlink — refuse to overwrite to
            // prevent data loss.
            return Err(anyhow::anyhow!(
                "refusing to overwrite non-symlink target: {}",
                target.display()
            ));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Target is absent; nothing to remove before rename.
        }
        Err(e) => {
            return Err(anyhow::Error::new(e))
                .with_context(|| format!("stat {}", target.display()));
        }
    }

    std::fs::rename(&tmp, target)
        .with_context(|| format!("rename {} to {}", tmp.display(), target.display()))?;

    guard.persist();
    Ok(())
}

/// Copy a directory: stage into a sibling temp directory, remove the
/// symlink/junction, then rename the temp directory into place.  Falls back to
/// a plain copy+delete when the rename crosses a filesystem boundary (EXDEV).
fn copy_dir_into_place(source: &Path, target: &Path) -> Result<()> {
    let tmp = sibling_temp_path(target, "_dotfiles_tmp");
    let mut guard = crate::fs::TempDir::new(tmp.clone());

    crate::fs::copy_dir_recursive(source, &tmp, false)
        .with_context(|| format!("recursive copy {} to {}", source.display(), tmp.display()))?;

    match target.symlink_metadata() {
        Ok(meta) if meta.is_symlink() => {
            remove_symlink(target)
                .with_context(|| format!("remove symlink/junction: {}", target.display()))?;
        }
        Ok(_) => {
            // Target exists but is not a symlink — refuse to overwrite to
            // prevent data loss.
            return Err(anyhow::anyhow!(
                "refusing to overwrite non-symlink target: {}",
                target.display()
            ));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Target is absent; nothing to remove before rename.
        }
        Err(e) => {
            return Err(anyhow::Error::new(e))
                .with_context(|| format!("stat {}", target.display()));
        }
    }

    // Prefer atomic rename; fall back to copy+delete only on cross-filesystem move.
    match std::fs::rename(&tmp, target) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            crate::fs::copy_dir_recursive(&tmp, target, false).with_context(|| {
                format!("cross-fs copy {} to {}", tmp.display(), target.display())
            })?;
            // Disarm the guard before cleanup so it doesn't try to remove on drop.
            guard.persist();
            // Best-effort cleanup; failure here is non-fatal since target is correct.
            let _ = std::fs::remove_dir_all(&tmp);
        }
        Err(e) => {
            return Err(anyhow::Error::new(e).context(format!(
                "rename {} to {}",
                tmp.display(),
                target.display()
            )));
        }
    }

    guard.persist();
    Ok(())
}

/// Compare two paths for equality, canonicalizing when possible.
///
/// Attempts `fs::canonicalize` on both paths so that symlinks in the path,
/// case differences (Windows), and `\\?\` UNC prefixes are resolved before
/// comparison.  Falls back to raw comparison when canonicalization fails
/// (e.g., dangling paths).
fn paths_equal(a: &Path, b: &Path) -> bool {
    // Fast path: literal match (covers the common case where the symlink was
    // created by this tool and nothing has changed).
    if a == b {
        return true;
    }

    // Try canonicalizing both; fall back to original path on failure.
    let canon_a = std::fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf());
    let canon_b = std::fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf());

    #[cfg(windows)]
    {
        // Windows filesystems are case-insensitive; compare with
        // case-folded Unicode to avoid false mismatches.
        let sa = canon_a.to_string_lossy().to_lowercase();
        let sb = canon_b.to_string_lossy().to_lowercase();
        sa == sb
    }

    #[cfg(not(windows))]
    {
        canon_a == canon_b
    }
}

/// Create a symlink at `link` pointing to `target`.
fn create_symlink(target: &Path, link: &Path, _executor: &dyn Executor) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).with_context(|| {
            format!(
                "creating symlink {} -> {}",
                link.display(),
                target.display()
            )
        })?;
    }

    #[cfg(windows)]
    {
        let is_dir = target.is_dir();
        let result = if is_dir {
            std::os::windows::fs::symlink_dir(target, link)
        } else {
            std::os::windows::fs::symlink_file(target, link)
        };

        result.with_context(|| {
            format!(
                "creating symlink {} -> {} (enable Developer Mode or run as administrator)",
                link.display(),
                target.display()
            )
        })?;
    }

    Ok(())
}

/// Remove a symlink, handling platform differences.
///
/// On Windows, directory symlinks must be removed with `remove_dir` (not `remove_file`).
/// Rust's `symlink_metadata().is_dir()` returns `false` for symlinks, so we check
/// the raw `FILE_ATTRIBUTE_DIRECTORY` flag to detect directory symlinks.
/// If `remove_dir` still fails with OS error 5 (access denied), we fall back
/// to `cmd /c rmdir` which runs in a separate process.
fn remove_symlink(path: &Path) -> Result<()> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(e.into()),
        Err(e) => {
            return Err(
                anyhow::Error::new(e).context(format!("reading metadata: {}", path.display()))
            );
        }
    };
    if is_dir_like(&meta) {
        match std::fs::remove_dir(path) {
            Ok(()) => {}
            #[cfg(windows)]
            Err(e) if e.raw_os_error() == Some(5) => {
                remove_dir_fallback(path)?;
            }
            Err(e) => return Err(e.into()),
        }
    } else {
        std::fs::remove_file(path).with_context(|| format!("removing file: {}", path.display()))?;
    }
    Ok(())
}

/// Check if metadata represents a directory-like entry.
/// On Windows, `symlink_metadata().is_dir()` returns `false` for directory symlinks,
/// so we check the raw `FILE_ATTRIBUTE_DIRECTORY` bit instead.
fn is_dir_like(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x10 != 0 // FILE_ATTRIBUTE_DIRECTORY
    }
    #[cfg(not(windows))]
    {
        meta.is_dir()
    }
}

/// Fallback directory removal on Windows using `cmd /c rmdir`.
/// This spawns a separate process that doesn't hold any handles from the
/// current process, which can resolve "Access is denied" errors.
#[cfg(windows)]
fn remove_dir_fallback(path: &Path) -> Result<()> {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("cmd")
        .arg("/c")
        .arg("rmdir")
        .arg("/s")
        .arg("/q")
        .arg(path)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .context("failed to run rmdir")?;
    if !output.status.success() {
        return Err(crate::error::ResourceError::command_failed(
            "rmdir",
            format!(
                "remove directory/symlink '{}': {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::exec::SystemExecutor;

    fn system_executor() -> Arc<dyn Executor> {
        Arc::new(SystemExecutor)
    }

    #[test]
    fn paths_equal_works() {
        let path1 = PathBuf::from("/tmp/test");
        let path2 = PathBuf::from("/tmp/test");
        assert!(paths_equal(&path1, &path2));

        let path3 = PathBuf::from("/tmp/other");
        assert!(!paths_equal(&path1, &path3));
    }

    #[cfg(unix)]
    #[test]
    fn paths_equal_resolves_through_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real_file");
        std::fs::write(&real, "content").unwrap();
        let link = dir.path().join("link_to_file");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        // Should be equal despite different literal paths
        assert!(paths_equal(&real, &link));
    }

    #[test]
    fn paths_equal_handles_nonexistent_paths() {
        // When both paths don't exist and are different, should not be equal
        assert!(!paths_equal(
            Path::new("/nonexistent/path/a"),
            Path::new("/nonexistent/path/b")
        ));
        // When both are the same nonexistent path, should be equal (fast path)
        assert!(paths_equal(
            Path::new("/nonexistent/same"),
            Path::new("/nonexistent/same")
        ));
    }

    #[test]
    fn symlink_resource_description() {
        let resource = SymlinkResource::new(
            PathBuf::from("/source"),
            PathBuf::from("/target"),
            system_executor(),
        );
        assert!(resource.description().contains("/source"));
        assert!(resource.description().contains("/target"));
    }

    #[test]
    fn sibling_temp_path_appends_suffix_without_clobbering_dotfile_name() {
        let bashrc_tmp = sibling_temp_path(Path::new("/home/test/.bashrc"), ".dotfiles_tmp");
        let vimrc_tmp = sibling_temp_path(Path::new("/home/test/.vimrc"), ".dotfiles_tmp");
        let ssh_tmp = sibling_temp_path(Path::new("/home/test/.ssh/config"), ".dotfiles_tmp");

        assert_eq!(bashrc_tmp, PathBuf::from("/home/test/.bashrc.dotfiles_tmp"));
        assert_eq!(vimrc_tmp, PathBuf::from("/home/test/.vimrc.dotfiles_tmp"));
        assert_eq!(
            ssh_tmp,
            PathBuf::from("/home/test/.ssh/config.dotfiles_tmp")
        );
        assert_ne!(bashrc_tmp, vimrc_tmp);
    }

    #[test]
    fn symlink_resource_invalid_when_source_missing() {
        let temp_dir = tempfile::tempdir().unwrap();
        let resource = SymlinkResource::new(
            temp_dir.path().join("nonexistent"),
            temp_dir.path().join("target"),
            system_executor(),
        );

        let state = resource.current_state().unwrap();
        assert!(matches!(state, ResourceState::Invalid { .. }));
    }

    #[test]
    fn symlink_resource_missing_when_target_not_exists() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source = temp_dir.path().join("source");
        std::fs::write(&source, "test").unwrap();

        let resource =
            SymlinkResource::new(source, temp_dir.path().join("target"), system_executor());

        let state = resource.current_state().unwrap();
        assert_eq!(state, ResourceState::Missing);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_resource_correct_when_link_points_to_source() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source = temp_dir.path().join("source");
        let target = temp_dir.path().join("target");
        std::fs::write(&source, "test").unwrap();
        std::os::unix::fs::symlink(&source, &target).unwrap();

        let resource = SymlinkResource::new(source, target, system_executor());

        let state = resource.current_state().unwrap();
        assert_eq!(state, ResourceState::Correct);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_resource_incorrect_when_link_points_to_wrong_source() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source = temp_dir.path().join("source");
        let other = temp_dir.path().join("other");
        let target = temp_dir.path().join("target");
        std::fs::write(&source, "test").unwrap();
        std::fs::write(&other, "other").unwrap();
        // link target → other (not source)
        std::os::unix::fs::symlink(&other, &target).unwrap();

        let resource = SymlinkResource::new(source, target, system_executor());

        let state = resource.current_state().unwrap();
        assert!(matches!(state, ResourceState::Incorrect { .. }));
    }

    #[test]
    fn symlink_resource_incorrect_when_target_is_regular_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source = temp_dir.path().join("source");
        let target = temp_dir.path().join("target");
        std::fs::write(&source, "content").unwrap();
        std::fs::write(&target, "other content").unwrap(); // regular file, not a symlink

        let resource = SymlinkResource::new(source, target, system_executor());

        let state = resource.current_state().unwrap();
        assert!(matches!(state, ResourceState::Incorrect { .. }));
    }

    /// A dangling symlink at the target (pointing to a non-existent path) must
    /// be reported as `Incorrect`, not `Missing`.  `Path::exists()` follows
    /// symlinks and returns `false` for dangling ones, so we use
    /// `symlink_metadata()` instead.
    #[cfg(unix)]
    #[test]
    fn symlink_resource_incorrect_when_target_is_dangling_symlink() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source = temp_dir.path().join("source");
        let target = temp_dir.path().join("target");
        let nowhere = temp_dir.path().join("does_not_exist");
        std::fs::write(&source, "content").unwrap();
        // Create a dangling symlink: target -> nowhere (nowhere doesn't exist)
        std::os::unix::fs::symlink(&nowhere, &target).unwrap();
        assert!(!nowhere.exists(), "precondition: nowhere must not exist");
        assert!(
            target.symlink_metadata().is_ok(),
            "precondition: dangling symlink must be present"
        );

        let resource = SymlinkResource::new(source, target, system_executor());

        let state = resource.current_state().unwrap();
        assert!(
            matches!(state, ResourceState::Incorrect { .. }),
            "dangling symlink should be Incorrect, got {state:?}"
        );
    }

    /// After `remove()` the target must be a regular file containing the
    /// original source content, not a symlink.
    #[test]
    #[allow(clippy::redundant_clone)]
    fn remove_file_symlink_materializes_content() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source = temp_dir.path().join("source.txt");
        let target = temp_dir.path().join("target.txt");
        std::fs::write(&source, b"hello dotfiles").unwrap();

        let resource = SymlinkResource::new(source.clone(), target.clone(), system_executor());
        resource.apply().unwrap();
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Correct
        ));

        resource.remove().unwrap();

        // Must be a regular file, not a symlink.
        let meta = std::fs::symlink_metadata(&target).unwrap();
        assert!(
            !meta.is_symlink(),
            "target should not be a symlink after materialize"
        );
        assert!(meta.is_file(), "target should be a regular file");
        assert_eq!(std::fs::read(&target).unwrap(), b"hello dotfiles");
    }

    /// After `remove()` on a directory symlink the target must be a real
    /// directory containing copies of all source files.
    #[test]
    #[allow(clippy::redundant_clone)]
    fn remove_dir_symlink_materializes_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source_dir = temp_dir.path().join("src_dir");
        let target_dir = temp_dir.path().join("target_dir");
        std::fs::create_dir(&source_dir).unwrap();
        std::fs::write(source_dir.join("a.txt"), b"aaa").unwrap();
        std::fs::create_dir(source_dir.join("sub")).unwrap();
        std::fs::write(source_dir.join("sub").join("b.txt"), b"bbb").unwrap();

        let resource =
            SymlinkResource::new(source_dir.clone(), target_dir.clone(), system_executor());
        resource.apply().unwrap();
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Correct
        ));

        resource.remove().unwrap();

        // Must be a real directory, not a symlink.
        let meta = std::fs::symlink_metadata(&target_dir).unwrap();
        assert!(
            !meta.is_symlink(),
            "target should not be a symlink after materialize"
        );
        assert!(meta.is_dir(), "target should be a real directory");
        assert_eq!(std::fs::read(target_dir.join("a.txt")).unwrap(), b"aaa");
        assert_eq!(
            std::fs::read(target_dir.join("sub").join("b.txt")).unwrap(),
            b"bbb"
        );
    }

    /// `remove()` on a file symlink must succeed even when the symlink has
    /// already been manually deleted — source content is materialized at the
    /// target path regardless.
    #[cfg(unix)]
    #[test]
    #[allow(clippy::redundant_clone)]
    fn remove_file_symlink_materializes_content_when_target_already_gone() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source = temp_dir.path().join("source.txt");
        let target = temp_dir.path().join("target.txt");
        std::fs::write(&source, b"hello dotfiles").unwrap();

        let resource = SymlinkResource::new(source.clone(), target.clone(), system_executor());
        resource.apply().unwrap();

        // Manually remove the symlink before calling remove().
        std::fs::remove_file(&target).unwrap();
        assert!(
            target.symlink_metadata().is_err(),
            "precondition: target must be absent"
        );

        // remove() must not error and must materialize source content.
        resource.remove().unwrap();

        let meta = std::fs::symlink_metadata(&target).unwrap();
        assert!(!meta.is_symlink(), "target should not be a symlink");
        assert!(meta.is_file(), "target should be a regular file");
        assert_eq!(std::fs::read(&target).unwrap(), b"hello dotfiles");
    }

    /// `remove()` on a directory symlink must succeed even when the symlink
    /// has already been manually deleted.
    #[cfg(unix)]
    #[test]
    #[allow(clippy::redundant_clone)]
    fn remove_dir_symlink_materializes_content_when_target_already_gone() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source_dir = temp_dir.path().join("src_dir");
        let target_dir = temp_dir.path().join("target_dir");
        std::fs::create_dir(&source_dir).unwrap();
        std::fs::write(source_dir.join("a.txt"), b"aaa").unwrap();

        let resource =
            SymlinkResource::new(source_dir.clone(), target_dir.clone(), system_executor());
        resource.apply().unwrap();

        // Manually remove the symlink before calling remove().
        std::fs::remove_file(&target_dir).unwrap();
        assert!(
            target_dir.symlink_metadata().is_err(),
            "precondition: target must be absent"
        );

        // remove() must not error and must materialize source content.
        resource.remove().unwrap();

        let meta = std::fs::symlink_metadata(&target_dir).unwrap();
        assert!(!meta.is_symlink(), "target should not be a symlink");
        assert!(meta.is_dir(), "target should be a real directory");
        assert_eq!(std::fs::read(target_dir.join("a.txt")).unwrap(), b"aaa");
    }

    /// `remove()` must not overwrite a real file at the target path — doing so
    /// would destroy user data.  The result must be `Skipped` and the original
    /// file content must remain intact.
    #[test]
    fn remove_does_not_overwrite_real_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source = temp_dir.path().join("source.txt");
        let target = temp_dir.path().join("target.txt");
        std::fs::write(&source, b"source content").unwrap();
        // Write a real file (not a symlink) at target, simulating a user-managed file.
        std::fs::write(&target, b"user content").unwrap();

        let resource = SymlinkResource::new(source, target.clone(), system_executor());
        let result = resource.remove().unwrap();

        assert!(
            matches!(result, ResourceChange::Skipped { .. }),
            "remove() must skip a non-symlink target to prevent data loss, got {result:?}"
        );
        // User content must be completely intact.
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"user content",
            "real file content must not be modified"
        );
    }

    /// `remove()` must not overwrite a real directory at the target path —
    /// doing so would destroy user data.  The result must be `Skipped` and the
    /// directory contents must remain intact.
    #[cfg(unix)]
    #[test]
    fn remove_does_not_overwrite_real_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source_dir = temp_dir.path().join("src_dir");
        let target_dir = temp_dir.path().join("target_dir");
        std::fs::create_dir(&source_dir).unwrap();
        std::fs::write(source_dir.join("source.txt"), b"source content").unwrap();
        // Create a real directory (not a symlink) at target.
        std::fs::create_dir(&target_dir).unwrap();
        std::fs::write(target_dir.join("user.txt"), b"user content").unwrap();

        let resource = SymlinkResource::new(source_dir, target_dir.clone(), system_executor());
        let result = resource.remove().unwrap();

        assert!(
            matches!(result, ResourceChange::Skipped { .. }),
            "remove() must skip a non-symlink target directory to prevent data loss, got {result:?}"
        );
        // User content must be completely intact.
        assert_eq!(
            std::fs::read(target_dir.join("user.txt")).unwrap(),
            b"user content",
            "real directory content must not be modified"
        );
    }
}
