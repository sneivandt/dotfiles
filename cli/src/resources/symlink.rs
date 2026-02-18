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

/// Remove a symlink or file.
fn remove_symlink(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::fs::remove_file(path)?;
    }

    #[cfg(windows)]
    {
        // On Windows, use remove_dir_all for directory symlinks, remove_file for file symlinks
        if path.symlink_metadata()?.is_dir() {
            std::fs::remove_dir_all(path)?;
        } else {
            std::fs::remove_file(path)?;
        }
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
