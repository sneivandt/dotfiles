//! Git hook resource.
use std::path::PathBuf;

use crate::engine::{
    IntrinsicState, RemovableResource, Resource, ResourceChange, ResourceResult, ResourceState,
};

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

    pub(crate) fn targets_source(&self) -> ResourceResult<bool> {
        if self.source == self.target {
            return Ok(true);
        }
        if !self.source.try_exists()? || !self.target.try_exists()? {
            return Ok(false);
        }
        Ok(crate::infra::fs::canonicalize(&self.source)?
            == crate::infra::fs::canonicalize(&self.target)?)
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
        if self.targets_source()? {
            return Ok(ResourceChange::AlreadyCorrect);
        }
        crate::infra::fs::ensure_parent_dir(&self.target)?;
        let parent = self
            .target
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let (mut staged, file) =
            crate::infra::fs::TempGuard::create_unique_file(parent, ".dotfiles-hook", "tmp")?;
        drop(file);
        crate::infra::fs::copy_file(&self.source, staged.path())?;

        #[cfg(unix)]
        crate::infra::fs::set_executable(staged.path())?;

        crate::infra::fs::rename_into_place(staged.path(), &self.target)?;
        staged.persist();

        Ok(ResourceChange::Applied)
    }
}

impl RemovableResource for HookFileResource {
    fn remove(&self) -> ResourceResult<ResourceChange> {
        if self.targets_source()? {
            return Ok(ResourceChange::AlreadyCorrect);
        }
        if crate::infra::fs::remove_file_if_present(&self.target, "stat hook")? {
            Ok(ResourceChange::Applied)
        } else {
            Ok(ResourceChange::AlreadyCorrect)
        }
    }
}

impl IntrinsicState for HookFileResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
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
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn apply_preserves_existing_hook_when_source_copy_fails() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source-directory");
        let target = dir.path().join("pre-commit");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(&target, "working hook").unwrap();
        let resource = HookFileResource::new(source, target.clone());

        assert!(resource.apply().is_err());

        assert_eq!(std::fs::read_to_string(target).unwrap(), "working hook");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn apply_replaces_existing_hook_and_converges() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let target = dir.path().join("pre-commit");
        std::fs::write(&source, "new hook").unwrap();
        std::fs::write(&target, "old hook").unwrap();
        let resource = HookFileResource::new(source, target.clone());

        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);

        assert_eq!(std::fs::read_to_string(target).unwrap(), "new hook");
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn apply_preserves_target_when_publication_fails() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let target = dir.path().join("pre-commit");
        std::fs::write(&source, "new hook").unwrap();
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep"), "user content").unwrap();
        let resource = HookFileResource::new(source, target.clone());

        assert!(resource.apply().is_err());

        assert_eq!(
            std::fs::read_to_string(target.join("keep")).unwrap(),
            "user content"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn apply_replaces_hook_symlink_without_writing_through_it() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let target = dir.path().join("pre-commit");
        let unrelated = dir.path().join("unrelated");
        std::fs::write(&source, "new hook").unwrap();
        std::fs::write(&unrelated, "user content").unwrap();
        std::os::unix::fs::symlink(&unrelated, &target).unwrap();
        let resource = HookFileResource::new(source, target.clone());

        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);

        assert!(!target.symlink_metadata().unwrap().is_symlink());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "new hook");
        assert_eq!(std::fs::read_to_string(unrelated).unwrap(), "user content");
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
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

    #[test]
    fn source_targets_are_preserved_by_apply_and_remove() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let source = dir.path().join("pre-commit");
        std::fs::write(&source, "uncommitted hook content").unwrap();
        for target in [source.clone(), dir.path().join(".").join("pre-commit")] {
            let resource = HookFileResource::new(source.clone(), target);
            assert!(resource.targets_source().unwrap());
            assert_eq!(resource.apply().unwrap(), ResourceChange::AlreadyCorrect);
            assert_eq!(resource.remove().unwrap(), ResourceChange::AlreadyCorrect);
            assert_eq!(
                std::fs::read_to_string(&source).unwrap(),
                "uncommitted hook content"
            );
        }
    }

    #[test]
    fn canonical_directory_alias_preserves_source_hook() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let hooks = dir.path().join("hooks");
        std::fs::create_dir(&hooks).unwrap();
        let source = hooks.join("pre-commit");
        std::fs::write(&source, "uncommitted hook content").unwrap();
        let alias = dir.path().join("alias");
        #[cfg(unix)]
        std::os::unix::fs::symlink(std::fs::canonicalize(&hooks).unwrap(), &alias).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(std::fs::canonicalize(&hooks).unwrap(), &alias)
            .expect("native symlinks require a symlink-capable test worker");
        let resource = HookFileResource::new(source.clone(), alias.join("pre-commit"));
        assert!(resource.targets_source().unwrap());
        assert_eq!(resource.apply().unwrap(), ResourceChange::AlreadyCorrect);
        assert_eq!(resource.remove().unwrap(), ResourceChange::AlreadyCorrect);
        assert_eq!(
            std::fs::read_to_string(source).unwrap(),
            "uncommitted hook content"
        );
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
