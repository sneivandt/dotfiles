//! Symlink resource.
use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};

use super::{Applicable, Resource, ResourceChange, ResourceState};

/// A symlink resource that can be checked and applied.
#[derive(Debug, Clone)]
pub struct SymlinkResource {
    /// The source file/directory (what the symlink points to).
    pub source: PathBuf,
    /// The target path (where the symlink will be created).
    pub target: PathBuf,
}

impl SymlinkResource {
    /// Create a new symlink resource.
    #[must_use]
    pub const fn new(source: PathBuf, target: PathBuf) -> Self {
        Self { source, target }
    }
}

impl Applicable for SymlinkResource {
    fn description(&self) -> String {
        format!("{} -> {}", self.target.display(), self.source.display())
    }

    fn apply(&self) -> Result<ResourceChange> {
        super::helpers::fs::ensure_parent_dir(&self.target)?;

        // Remove existing target if it's a symlink or file
        if self.target.exists() || self.target.symlink_metadata().is_ok() {
            remove_symlink(&self.target)
                .with_context(|| format!("remove existing: {}", self.target.display()))?;
        }

        // Create the symlink
        create_symlink(&self.source, &self.target)
            .with_context(|| format!("create link: {}", self.target.display()))?;

        Ok(ResourceChange::Applied)
    }

    fn remove(&self) -> Result<ResourceChange> {
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
                // Target doesn't exist or isn't a symlink
                if self.target.exists() {
                    Ok(ResourceState::Incorrect {
                        current: "target is a regular file".to_string(),
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
/// recursively; any symlinks *within* the source tree are followed (their
/// content is copied, not the link itself).
fn copy_into_place(source: &Path, target: &Path) -> Result<()> {
    if source.is_dir() {
        copy_dir_into_place(source, target)
    } else {
        copy_file_into_place(source, target)
    }
}

/// Copy a regular file: stage to a temp sibling, remove the symlink, rename
/// the temp file into place.
fn copy_file_into_place(source: &Path, target: &Path) -> Result<()> {
    // Use a sibling temp name to keep the rename on the same filesystem.
    let tmp = target.with_extension("dotfiles_tmp");
    std::fs::copy(source, &tmp)
        .with_context(|| format!("copy {} to {}", source.display(), tmp.display()))?;

    let cleanup_file = || {
        let _ = std::fs::remove_file(&tmp);
    };

    if let Err(e) = remove_symlink(target) {
        cleanup_file();
        return Err(e).with_context(|| format!("remove symlink: {}", target.display()));
    }
    if let Err(e) = std::fs::rename(&tmp, target) {
        cleanup_file();
        return Err(e).with_context(|| format!("rename {} to {}", tmp.display(), target.display()));
    }
    Ok(())
}

/// Copy a directory: stage into a sibling temp directory, remove the
/// symlink/junction, then rename the temp directory into place.  Falls back to
/// a plain copy+delete when the rename crosses a filesystem boundary (EXDEV).
fn copy_dir_into_place(source: &Path, target: &Path) -> Result<()> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let stem = target.file_name().map_or_else(
        || "dotfiles_tmp".to_string(),
        |n| format!("{}_dotfiles_tmp", n.to_string_lossy()),
    );
    let tmp = parent.join(&stem);

    let cleanup_dir = || {
        let _ = std::fs::remove_dir_all(&tmp);
    };

    super::helpers::fs::copy_dir_recursive(source, &tmp, false)
        .with_context(|| format!("recursive copy {} to {}", source.display(), tmp.display()))?;

    if let Err(e) = remove_symlink(target) {
        cleanup_dir();
        return Err(e).with_context(|| format!("remove symlink/junction: {}", target.display()));
    }

    // Prefer atomic rename; fall back to copy+delete on cross-filesystem move.
    if std::fs::rename(&tmp, target).is_err() {
        if let Err(e) = super::helpers::fs::copy_dir_recursive(&tmp, target, false) {
            cleanup_dir();
            return Err(e).with_context(|| {
                format!("cross-fs copy {} to {}", tmp.display(), target.display())
            });
        }
        std::fs::remove_dir_all(&tmp)
            .with_context(|| format!("remove tmp dir: {}", tmp.display()))?;
    }
    Ok(())
}

/// Compare two paths for equality, handling UNC prefix normalization on Windows.
fn paths_equal(a: &Path, b: &Path) -> bool {
    let normalize = |p: &Path| -> PathBuf {
        #[cfg(windows)]
        {
            let s = p.to_string_lossy();
            if let Some(stripped) = s.strip_prefix(r"\\?\") {
                return PathBuf::from(stripped);
            }
        }
        p.to_path_buf()
    };

    normalize(a) == normalize(b)
}

/// Create a symlink at `link` pointing to `target`.
fn create_symlink(target: &Path, link: &Path) -> Result<()> {
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
        use crate::exec;

        // Try native symlink API first
        let is_dir = target.is_dir();
        let result = if is_dir {
            std::os::windows::fs::symlink_dir(target, link)
        } else {
            std::os::windows::fs::symlink_file(target, link)
        };

        if result.is_err() {
            // Fall back to mklink via cmd.exe (requires admin or dev mode)
            // Use /J (junction) for directories, no flag (symlink) for files.
            // Note: /H creates a hard link which has different semantics.
            let link_str = link.to_string_lossy();
            let target_str = target.to_string_lossy();
            let mut args: Vec<&str> = vec!["/c", "mklink"];
            if is_dir {
                args.push("/J");
            }
            args.push(&link_str);
            args.push(&target_str);
            exec::run("cmd", &args)?;
        }
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
    let meta = std::fs::symlink_metadata(path)
        .with_context(|| format!("reading metadata: {}", path.display()))?;
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
        .arg("/q")
        .arg(path)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .context("failed to run rmdir")?;
    if !output.status.success() {
        anyhow::bail!(
            "remove directory/symlink '{}': {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn paths_equal_works() {
        let path1 = PathBuf::from("/tmp/test");
        let path2 = PathBuf::from("/tmp/test");
        assert!(paths_equal(&path1, &path2));

        let path3 = PathBuf::from("/tmp/other");
        assert!(!paths_equal(&path1, &path3));
    }

    #[test]
    fn symlink_resource_description() {
        let resource = SymlinkResource::new(PathBuf::from("/source"), PathBuf::from("/target"));
        assert!(resource.description().contains("/source"));
        assert!(resource.description().contains("/target"));
    }

    #[test]
    fn symlink_resource_invalid_when_source_missing() {
        let temp_dir = tempfile::tempdir().unwrap();
        let resource = SymlinkResource::new(
            temp_dir.path().join("nonexistent"),
            temp_dir.path().join("target"),
        );

        let state = resource.current_state().unwrap();
        assert!(matches!(state, ResourceState::Invalid { .. }));
    }

    #[test]
    fn symlink_resource_missing_when_target_not_exists() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source = temp_dir.path().join("source");
        std::fs::write(&source, "test").unwrap();

        let resource = SymlinkResource::new(source, temp_dir.path().join("target"));

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

        let resource = SymlinkResource::new(source, target);

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

        let resource = SymlinkResource::new(source, target);

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

        let resource = SymlinkResource::new(source, target);

        let state = resource.current_state().unwrap();
        assert!(matches!(state, ResourceState::Incorrect { .. }));
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

        let resource = SymlinkResource::new(source.clone(), target.clone());
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

        let resource = SymlinkResource::new(source_dir.clone(), target_dir.clone());
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
}
