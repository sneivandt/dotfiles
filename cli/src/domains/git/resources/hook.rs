//! Git hook resource.
use crate::infra::fs::content_hash;
use std::path::PathBuf;

use crate::engine::{
    IntrinsicState, RemovableResource, Resource, ResourceChange, ResourceResult, ResourceState,
};

pub(crate) const OWNERSHIP_EXTENSION: &str = "dotfiles-owned";

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

    fn ownership_path(&self) -> PathBuf {
        self.target.with_extension(OWNERSHIP_EXTENSION)
    }

    fn ownership_record(&self) -> ResourceResult<String> {
        match std::fs::read_to_string(self.ownership_path()) {
            Ok(recorded) => Ok(recorded),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(error) => Err(error.into()),
        }
    }

    fn target_contents(&self) -> ResourceResult<Option<Vec<u8>>> {
        let Some(metadata) =
            crate::infra::fs::symlink_metadata_optional(&self.target, "stat hook")?
        else {
            return Ok(None);
        };
        if !metadata.is_file() || metadata.is_symlink() {
            return Err(anyhow::anyhow!(
                "preserving foreign hook {}; move it aside before installing dotfiles hooks",
                self.target.display()
            )
            .into());
        }
        Ok(Some(crate::infra::fs::read_bytes(&self.target)?))
    }
}

#[cfg(unix)]
fn git_can_execute(path: &std::path::Path) -> ResourceResult<bool> {
    match nix::unistd::access(path, nix::unistd::AccessFlags::X_OK) {
        Ok(()) => Ok(true),
        Err(nix::errno::Errno::EACCES) => Ok(false),
        Err(error) => Err(anyhow::Error::new(error)
            .context(format!("checking hook execute access: {}", path.display()))
            .into()),
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
        let source = crate::infra::fs::read_bytes(&self.source)?;
        let before = self.target_contents()?;
        if let Some(contents) = &before
            && *contents != source
            && !self
                .ownership_record()?
                .lines()
                .any(|line| line == content_hash(contents))
        {
            return Err(anyhow::anyhow!(
                "preserving foreign or modified hook {}; back it up and move it aside before installing",
                self.target.display()
            ).into());
        }
        crate::infra::fs::ensure_parent_dir(&self.target)?;
        let parent = self
            .target
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let (mut staged, file) =
            crate::infra::fs::TempGuard::create_unique_file(parent, ".dotfiles-hook", "tmp")?;
        drop(file);
        std::fs::write(staged.path(), &source)?;

        #[cfg(unix)]
        {
            crate::infra::fs::set_executable(staged.path())?;
            if !git_can_execute(staged.path())? {
                return Err(anyhow::anyhow!(
                    "Git cannot execute hooks in {}; check directory access and mount options",
                    parent.display()
                )
                .into());
            }
        }

        if self.target_contents()? != before {
            return Err(anyhow::anyhow!(
                "hook {} changed during installation; retry",
                self.target.display()
            )
            .into());
        }
        // Record both possible publication outcomes so an interrupted update remains owned.
        let mut ownership = format!("{}\n", content_hash(&source));
        if let Some(before) = &before {
            ownership.push_str(&content_hash(before));
            ownership.push('\n');
        }
        crate::infra::fs::write_atomic(&self.ownership_path(), ownership)?;
        crate::infra::fs::rename_into_place(staged.path(), &self.target)?;
        staged.persist();
        crate::infra::fs::write_atomic(
            &self.ownership_path(),
            format!("{}\n", content_hash(source)),
        )?;

        Ok(ResourceChange::Applied)
    }
}

impl RemovableResource for HookFileResource {
    fn removal_state(&self) -> ResourceResult<ResourceState> {
        if self.targets_source()? {
            return Ok(ResourceState::Missing);
        }
        let Some(contents) = self.target_contents()? else {
            return Ok(ResourceState::Missing);
        };
        let hash = content_hash(&contents);
        if self.ownership_record()?.lines().any(|line| line == hash)
            || (self.source.try_exists()?
                && crate::infra::fs::read_bytes(&self.source)? == contents)
        {
            Ok(ResourceState::Correct)
        } else {
            Ok(ResourceState::Incorrect {
                current: "foreign or modified hook".into(),
            })
        }
    }

    fn remove(&self) -> ResourceResult<ResourceChange> {
        if self.targets_source()? {
            return Ok(ResourceChange::AlreadyCorrect);
        }
        if self.removal_state()? != ResourceState::Correct {
            return Ok(ResourceChange::AlreadyCorrect);
        }
        if crate::infra::fs::remove_file_if_present(&self.target, "stat hook")? {
            crate::infra::fs::remove_file_if_present(
                &self.ownership_path(),
                "stat hook ownership",
            )?;
            Ok(ResourceChange::Applied)
        } else {
            Ok(ResourceChange::AlreadyCorrect)
        }
    }
}

impl IntrinsicState for HookFileResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        if let Some(reason) = crate::infra::fs::missing_source_reason(&self.source)? {
            return Ok(ResourceState::Invalid { reason });
        }

        // Detect broken symlinks at the target location
        match crate::infra::fs::symlink_metadata_optional(&self.target, "stat target")? {
            Some(metadata) if metadata.is_symlink() && !self.targets_source()? => {
                return Ok(ResourceState::Invalid {
                    reason: "foreign hook symlink; move it aside before installing dotfiles hooks"
                        .into(),
                });
            }
            None => return Ok(ResourceState::Missing),
            Some(_) => {}
        }
        let src_content = crate::infra::fs::read_bytes(&self.source)?;
        let dst_content = crate::infra::fs::read_bytes(&self.target)?;
        let record = self.ownership_record()?;
        let hash = content_hash(&dst_content);
        let owned = record.lines().any(|line| line == hash);
        if src_content != dst_content && !owned {
            return Ok(ResourceState::Invalid {
                reason: "foreign or modified hook; back it up and move it aside before installing dotfiles hooks".into(),
            });
        }

        #[cfg(unix)]
        if !git_can_execute(&self.target)? {
            return Ok(ResourceState::Incorrect {
                current: "not executable".to_string(),
            });
        }

        // A two-hash record is only a pending publication, even if the new
        // bytes are already visible. Retry must retire the previous hash.
        if src_content == dst_content
            && (record.lines().eq([hash.as_str()]) || self.targets_source()?)
        {
            Ok(ResourceState::Correct)
        } else {
            Ok(ResourceState::Incorrect {
                current: if owned && src_content == dst_content {
                    "ownership update incomplete"
                } else if owned {
                    "content differs"
                } else {
                    "ownership not recorded"
                }
                .to_string(),
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
    fn current_state_preserves_source_inspection_errors() {
        let dir = tempfile::tempdir().unwrap();
        let resource = HookFileResource::new(
            dir.path().join("invalid\0source"),
            dir.path().join("target"),
        );
        let error = resource.current_state().unwrap_err();
        assert!(format!("{error:#}").contains("inspect source"));
        assert!(!resource.target.exists());
    }

    #[cfg(unix)]
    #[test]
    fn target_symlink_loop_is_an_inspection_error_not_repairable_state() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let target = dir.path().join("target");
        std::fs::write(&source, "#!/bin/sh\n").unwrap();
        std::os::unix::fs::symlink("target", &target).unwrap();
        let resource = HookFileResource::new(source, target.clone());
        let error = resource.current_state().unwrap_err();
        assert!(
            matches!(error, crate::engine::resource::ResourceError::Io { .. }),
            "{error:#}"
        );
        assert_eq!(std::fs::read_link(target).unwrap(), PathBuf::from("target"));
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
        resource.apply().unwrap();
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let resource = HookFileResource::new(src, dst);
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Invalid { .. }
        ));
        assert!(resource.apply().is_err());
        assert_eq!(
            std::fs::read_to_string(&resource.target).unwrap(),
            "old content"
        );
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
        std::fs::write(&source, "old hook").unwrap();
        let resource = HookFileResource::new(source.clone(), target.clone());
        resource.apply().unwrap();
        std::fs::write(&source, "new hook").unwrap();

        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);

        assert_eq!(std::fs::read_to_string(target).unwrap(), "new hook");
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
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
    fn apply_preserves_foreign_hook_symlink_and_its_referent() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let target = dir.path().join("pre-commit");
        let unrelated = dir.path().join("unrelated");
        std::fs::write(&source, "new hook").unwrap();
        std::fs::write(&unrelated, "user content").unwrap();
        std::os::unix::fs::symlink(&unrelated, &target).unwrap();
        let resource = HookFileResource::new(source, target.clone());

        assert!(resource.apply().is_err());
        assert!(target.symlink_metadata().unwrap().is_symlink());
        assert_eq!(std::fs::read_to_string(unrelated).unwrap(), "user content");
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Invalid { .. }
        ));
    }

    #[test]
    fn remove_deletes_file() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("target");
        let source = dir.path().join("src");
        std::fs::write(&source, "content").unwrap();
        let resource = HookFileResource::new(source, dst.clone());
        resource.apply().unwrap();
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
    fn remove_preserves_foreign_broken_symlink() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("target");
        symlink(dir.path().join("missing-hook"), &dst).unwrap();

        let resource = HookFileResource::new(dir.path().join("src"), dst.clone());

        assert!(!dst.exists());
        assert!(dst.symlink_metadata().is_ok());

        assert!(resource.remove().is_err());
        assert!(dst.symlink_metadata().is_ok());
    }

    #[test]
    fn edited_owned_hook_is_preserved_by_update_and_removal() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let target = dir.path().join("pre-commit");
        std::fs::write(&source, "managed").unwrap();
        let resource = HookFileResource::new(source, target.clone());
        resource.apply().unwrap();
        std::fs::write(&target, "user modified").unwrap();
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Invalid { .. }
        ));
        assert!(resource.apply().is_err());
        assert_eq!(resource.remove().unwrap(), ResourceChange::AlreadyCorrect);
        assert_eq!(std::fs::read_to_string(target).unwrap(), "user modified");
    }

    #[test]
    fn removal_recognizes_owned_hook_after_source_changes() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        std::fs::write(&source, "version one").unwrap();
        let resource = HookFileResource::new(source.clone(), dir.path().join("pre-commit"));
        resource.apply().unwrap();
        std::fs::write(source, "version two").unwrap();
        assert_eq!(resource.removal_state().unwrap(), ResourceState::Correct);
        assert_eq!(resource.remove().unwrap(), ResourceChange::Applied);
        assert!(!resource.ownership_path().exists());
    }

    #[test]
    fn retry_finalizes_hook_ownership_on_either_side_of_publication() {
        for published in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let resource =
                HookFileResource::new(dir.path().join("source"), dir.path().join("hook"));
            std::fs::write(&resource.source, b"old").unwrap();
            resource.apply().unwrap();
            std::fs::write(&resource.source, b"new").unwrap();
            let pending = format!("{}\n{}\n", content_hash(b"new"), content_hash(b"old"));
            std::fs::write(resource.ownership_path(), &pending).unwrap();
            if published {
                std::fs::write(&resource.target, b"new").unwrap();
            }
            assert!(matches!(
                resource.current_state().unwrap(),
                ResourceState::Incorrect { .. }
            ));
            assert_eq!(
                std::fs::read_to_string(resource.ownership_path()).unwrap(),
                pending
            );
            resource.apply().unwrap();
            assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
            assert_eq!(std::fs::read(&resource.target).unwrap(), b"new");
            std::fs::write(&resource.target, b"old").unwrap(); // user restores an older copy
            assert_eq!(resource.remove().unwrap(), ResourceChange::AlreadyCorrect);
            assert_eq!(std::fs::read(&resource.target).unwrap(), b"old");
        }
    }

    #[cfg(unix)]
    #[test]
    fn copied_hook_with_owner_execute_removed_runs_helpers_after_repair() {
        use std::os::unix::fs::PermissionsExt as _;
        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let helpers = root.join("hooks");
        std::fs::create_dir(&helpers).unwrap();
        for check in ["check-sensitive.sh", "check-rust.sh"] {
            std::fs::write(helpers.join(check), format!("echo {check} >> ran\n")).unwrap();
        }
        std::fs::write(
            root.join("git"),
            "#!/bin/sh\n[ \"$*\" = 'rev-parse --show-toplevel' ] || exit 99\npwd\n",
        )
        .unwrap();
        crate::infra::fs::set_executable(&root.join("git")).unwrap();
        std::os::unix::fs::symlink("/bin/sh", root.join("sh")).unwrap();
        let source = helpers.join("pre-commit");
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../hooks/pre-commit"),
            &source,
        )
        .unwrap();
        let resource = HookFileResource::new(source, root.join("installed/pre-commit"));
        resource.apply().unwrap();
        let run = || {
            std::process::Command::new(&resource.target)
                .current_dir(&root)
                .env_clear()
                .env("PATH", &root)
                .status()
        };
        // Root may execute a file with any execute bit; ordinary owners cannot.
        let mode = if nix::unistd::getuid().is_root() {
            0o644
        } else {
            0o655
        };
        std::fs::set_permissions(&resource.target, std::fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(
            run().unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Incorrect { .. }
        ));
        assert!(!root.join("ran").exists());
        resource.apply().unwrap();
        assert!(run().unwrap().success());
        assert_eq!(
            std::fs::read_to_string(root.join("ran")).unwrap(),
            "check-sensitive.sh\ncheck-rust.sh\n"
        );
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }
}
