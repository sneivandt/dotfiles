use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};

use super::{Resource, ResourceChange, ResourceState};

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

impl Resource for SymlinkResource {
    fn description(&self) -> String {
        format!("{} -> {}", self.target.display(), self.source.display())
    }

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

    fn apply(&self) -> Result<ResourceChange> {
        // Ensure parent directory exists
        if let Some(parent) = self.target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create parent: {}", parent.display()))?;
        }

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
        if self.target.symlink_metadata().is_ok() {
            remove_symlink(&self.target)
                .with_context(|| format!("remove symlink: {}", self.target.display()))?;
            Ok(ResourceChange::Applied)
        } else {
            Ok(ResourceChange::AlreadyCorrect)
        }
    }
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
        std::os::unix::fs::symlink(target, link)?;
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
            let flag = if is_dir { "/J" } else { "/H" };
            exec::run(
                "cmd",
                &[
                    "/c",
                    "mklink",
                    flag,
                    &link.to_string_lossy(),
                    &target.to_string_lossy(),
                ],
            )?;
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
    let meta = std::fs::symlink_metadata(path)?;
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
        std::fs::remove_file(path)?;
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
}
