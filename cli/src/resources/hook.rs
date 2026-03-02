//! Git hook resource.
use anyhow::{Context as _, Result};
use std::path::PathBuf;

use super::{Applicable, Resource, ResourceChange, ResourceState};

/// A git hook file resource that can be checked, installed, and removed.
#[derive(Debug, Clone)]
pub struct HookFileResource {
    /// Source hook file (e.g., hooks/pre-commit).
    pub source: PathBuf,
    /// Target path in .git/hooks/ (e.g., .git/hooks/pre-commit).
    pub target: PathBuf,
}

impl HookFileResource {
    /// Create a new hook file resource.
    #[must_use]
    pub const fn new(source: PathBuf, target: PathBuf) -> Self {
        Self { source, target }
    }
}

impl Applicable for HookFileResource {
    fn description(&self) -> String {
        self.target.file_name().map_or_else(
            || self.target.display().to_string(),
            |n| n.to_string_lossy().to_string(),
        )
    }

    fn apply(&self) -> Result<ResourceChange> {
        super::helpers::fs::ensure_parent_dir(&self.target)?;
        super::helpers::fs::remove_existing(&self.target)?;

        // Copy file
        std::fs::copy(&self.source, &self.target)
            .with_context(|| format!("copy hook to {}", self.target.display()))?;

        // Make executable on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&self.target)
                .with_context(|| format!("reading hook metadata: {}", self.target.display()))?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&self.target, perms)
                .with_context(|| format!("setting hook permissions: {}", self.target.display()))?;
        }

        Ok(ResourceChange::Applied)
    }

    fn remove(&self) -> Result<ResourceChange> {
        if self.target.exists() {
            std::fs::remove_file(&self.target)
                .with_context(|| format!("remove hook: {}", self.target.display()))?;
            Ok(ResourceChange::Applied)
        } else {
            Ok(ResourceChange::AlreadyCorrect)
        }
    }
}

impl Resource for HookFileResource {
    fn current_state(&self) -> Result<ResourceState> {
        if !self.source.exists() {
            return Ok(ResourceState::Invalid {
                reason: format!("source does not exist: {}", self.source.display()),
            });
        }

        // Detect broken symlinks at the target location
        if !self.target.exists() && self.target.symlink_metadata().is_ok() {
            return Ok(ResourceState::Incorrect {
                current: "broken symlink".to_string(),
            });
        }

        if !self.target.exists() {
            return Ok(ResourceState::Missing);
        }

        // Compare file contents
        let src_content = std::fs::read(&self.source)
            .with_context(|| format!("read source: {}", self.source.display()))?;
        let dst_content = std::fs::read(&self.target)
            .with_context(|| format!("read target: {}", self.target.display()))?;

        if src_content == dst_content {
            Ok(ResourceState::Correct)
        } else {
            Ok(ResourceState::Incorrect {
                current: "content differs".to_string(),
            })
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn description_returns_filename() {
        let resource = HookFileResource::new(
            PathBuf::from("/repo/hooks/pre-commit"),
            PathBuf::from("/repo/.git/hooks/pre-commit"),
        );
        assert_eq!(resource.description(), "pre-commit");
    }

    #[test]
    fn current_state_missing_source() {
        let dir = tempfile::tempdir().unwrap();
        let resource =
            HookFileResource::new(dir.path().join("nonexistent"), dir.path().join("target"));
        let state = resource.current_state().unwrap();
        assert!(matches!(state, ResourceState::Invalid { .. }));
    }

    #[test]
    fn current_state_missing_target() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("hook");
        std::fs::write(&src, "#!/bin/sh\necho hi").unwrap();
        let resource = HookFileResource::new(src, dir.path().join("target"));
        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    }

    #[test]
    fn current_state_correct() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("hook");
        let dst = dir.path().join("target");
        let content = "#!/bin/sh\necho hi";
        std::fs::write(&src, content).unwrap();
        std::fs::write(&dst, content).unwrap();
        let resource = HookFileResource::new(src, dst);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn current_state_incorrect() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("hook");
        let dst = dir.path().join("target");
        std::fs::write(&src, "new content").unwrap();
        std::fs::write(&dst, "old content").unwrap();
        let resource = HookFileResource::new(src, dst);
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Incorrect { .. }
        ));
    }

    #[test]
    fn apply_copies_file() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("hook");
        let dst = dir.path().join("subdir").join("target");
        std::fs::write(&src, "#!/bin/sh\necho hi").unwrap();
        let resource = HookFileResource::new(src.clone(), dst.clone());
        let result = resource.apply().unwrap();
        assert_eq!(result, ResourceChange::Applied);
        assert_eq!(
            std::fs::read_to_string(&dst).unwrap(),
            std::fs::read_to_string(&src).unwrap()
        );
    }

    #[test]
    fn remove_deletes_file() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("target");
        std::fs::write(&dst, "content").unwrap();
        let resource = HookFileResource::new(dir.path().join("src"), dst.clone());
        let result = resource.remove().unwrap();
        assert_eq!(result, ResourceChange::Applied);
        assert!(!dst.exists());
    }

    #[test]
    fn remove_nonexistent_returns_already_correct() {
        let dir = tempfile::tempdir().unwrap();
        let resource =
            HookFileResource::new(dir.path().join("src"), dir.path().join("nonexistent"));
        let result = resource.remove().unwrap();
        assert_eq!(result, ResourceChange::AlreadyCorrect);
    }
}
