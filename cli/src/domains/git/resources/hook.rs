//! Git hook resource.
use anyhow::Result;
use std::path::PathBuf;

use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};

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

impl Resource for HookFileResource {
    fn description(&self) -> String {
        self.target.file_name().map_or_else(
            || self.target.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        crate::infra::fs::prepare_target(&self.target)?;
        crate::infra::fs::copy_file(&self.source, &self.target)?;

        #[cfg(unix)]
        crate::infra::fs::set_executable(&self.target)?;

        Ok(ResourceChange::Applied)
    }

    fn remove(&self) -> ResourceResult<ResourceChange> {
        if crate::infra::fs::remove_file_if_present(&self.target, "stat hook")? {
            Ok(ResourceChange::Applied)
        } else {
            Ok(ResourceChange::AlreadyCorrect)
        }
    }
}

impl IntrinsicState for HookFileResource {
    fn current_state(&self) -> Result<ResourceState> {
        if let Some(reason) = crate::infra::fs::missing_source_reason(&self.source) {
            return Ok(ResourceState::Invalid { reason });
        }

        // Detect broken symlinks at the target location
        match crate::infra::fs::symlink_metadata_optional(&self.target, "stat target")? {
            Some(_) if !self.target.exists() => {
                return Ok(ResourceState::Incorrect {
                    current: "broken symlink".to_string(),
                });
            }
            None => return Ok(ResourceState::Missing),
            Some(_) => {}
        }

        // On Unix, verify the installed hook has the executable bit set
        #[cfg(unix)]
        {
            use anyhow::Context as _;
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&self.target)
                .with_context(|| format!("read target metadata: {}", self.target.display()))?
                .permissions()
                .mode();
            if mode & 0o111 == 0 {
                return Ok(ResourceState::Incorrect {
                    current: "not executable".to_string(),
                });
            }
        }

        // Compare file contents
        let src_content = crate::infra::fs::read_bytes(&self.source)?;
        let dst_content = crate::infra::fs::read_bytes(&self.target)?;

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
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let resource = HookFileResource::new(src, dst);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[cfg(unix)]
    #[test]
    fn current_state_not_executable_returns_incorrect() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("hook");
        let dst = dir.path().join("target");
        let content = "#!/bin/sh\necho hi";
        std::fs::write(&src, content).unwrap();
        std::fs::write(&dst, content).unwrap();
        std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o644)).unwrap();
        let resource = HookFileResource::new(src, dst);
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Incorrect { current } if current == "not executable"
        ));
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

    #[cfg(unix)]
    #[test]
    fn remove_deletes_broken_symlink() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("target");
        symlink(dir.path().join("missing-hook"), &dst).unwrap();

        let resource = HookFileResource::new(dir.path().join("src"), dst.clone());

        assert!(!dst.exists());
        assert!(dst.symlink_metadata().is_ok());

        let result = resource.remove().unwrap();

        assert_eq!(result, ResourceChange::Applied);
        assert!(dst.symlink_metadata().is_err());
    }
}
