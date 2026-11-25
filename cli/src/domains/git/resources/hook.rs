//! Git hook resource.
use crate::infra::fs::{
    content_hash, open_regular_source, publish_with_ownership, rename_noreplace,
};
use anyhow::Context as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use crate::engine::{
    IntrinsicState, RemovableResource, Resource, ResourceChange, ResourceResult, ResourceState,
};

pub(crate) const OWNERSHIP_EXTENSION: &str = "dotfiles-owned";
pub(crate) const REMOVAL_EXTENSION: &str = "dotfiles-removing";

/// A git hook file resource that can be checked, installed, and removed.
#[derive(Debug, Clone)]
pub(crate) struct HookFileResource {
    /// Source hook file (e.g., hooks/pre-commit).
    pub source: PathBuf,
    /// Target path in .git/hooks/ (e.g., .git/hooks/pre-commit).
    pub target: PathBuf,
}

impl HookFileResource {
    /// Create a new hook file resource.
    #[must_use]
    pub(crate) const fn new(source: PathBuf, target: PathBuf) -> Self {
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
        let path = self.ownership_path();
        let Some(metadata) =
            crate::infra::fs::symlink_metadata_optional(&path, "stat hook ownership")?
        else {
            return Ok(String::new());
        };
        if !metadata.is_file() {
            return Err(anyhow::anyhow!(
                "preserving non-regular hook ownership record {}; move it aside before retrying",
                path.display()
            )
            .into());
        }
        let mut recorded = String::new();
        open_regular_source(&path)?
            .read_to_string(&mut recorded)
            .with_context(|| format!("read hook ownership {}", path.display()))?;
        Ok(recorded)
    }

    fn check_pending_removal(&self) -> anyhow::Result<()> {
        let saved = self.target.with_extension(REMOVAL_EXTENSION);
        anyhow::ensure!(
            crate::infra::fs::symlink_metadata_optional(&saved, "inspect saved hook")?.is_none(),
            "unfinished removal of {}: inspect {} and restore it if the hook is missing, or move it somewhere safe before retrying",
            self.target.display(),
            saved.display()
        );
        Ok(())
    }

    fn target_contents(path: &Path) -> ResourceResult<Option<Vec<u8>>> {
        let Some(metadata) = crate::infra::fs::symlink_metadata_optional(path, "stat hook")? else {
            return Ok(None);
        };
        if !metadata.is_file() {
            return Err(anyhow::anyhow!(
                "preserving foreign hook {}; move it aside before installing dotfiles hooks",
                path.display()
            )
            .into());
        }
        Ok(Some(read_hook(path)?))
    }
}

fn read_hook(path: &Path) -> ResourceResult<Vec<u8>> {
    let mut bytes = Vec::new();
    open_regular_source(path)?
        .read_to_end(&mut bytes)
        .with_context(|| format!("read hook {}", path.display()))?;
    Ok(bytes)
}

#[cfg(unix)]
fn git_can_execute(path: &Path) -> ResourceResult<bool> {
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
        self.check_pending_removal()?;
        if self.targets_source()? {
            return Ok(ResourceChange::AlreadyCorrect);
        }
        let source = read_hook(&self.source)?;
        let before = Self::target_contents(&self.target)?;
        let recorded = self.ownership_record()?;
        if let Some(contents) = &before
            && *contents != source
            && !recorded.lines().any(|line| line == content_hash(contents))
        {
            return Err(anyhow::anyhow!(
                "preserving foreign or modified hook {}; back it up and move it aside before installing",
                self.target.display()
            ).into());
        }
        crate::infra::fs::ensure_parent_dir(&self.target)?;
        let parent = self.target.parent().unwrap_or_else(|| Path::new("."));
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

        if Self::target_contents(&self.target)? != before {
            return Err(anyhow::anyhow!(
                "hook {} changed during installation; retry",
                self.target.display()
            )
            .into());
        }
        publish_with_ownership(&self.ownership_path(), before.as_deref(), &source, || {
            crate::infra::fs::rename_into_place(staged.path(), &self.target)?;
            staged.persist();
            Ok(())
        })
        .map(|()| ResourceChange::Applied)
    }
}

impl RemovableResource for HookFileResource {
    fn removal_state(&self) -> ResourceResult<ResourceState> {
        self.check_pending_removal()?;
        if self.targets_source()? {
            return Ok(ResourceState::Missing);
        }
        self.ownership_state_at(&self.target)
    }

    fn remove(&self) -> ResourceResult<ResourceChange> {
        if self.removal_state()? != ResourceState::Correct {
            return Ok(ResourceChange::AlreadyCorrect);
        }
        self.remove_verified_hook()
    }
}

impl HookFileResource {
    fn ownership_state_at(&self, path: &Path) -> ResourceResult<ResourceState> {
        let Some(contents) = Self::target_contents(path)? else {
            return Ok(ResourceState::Missing);
        };
        let hash = content_hash(&contents);
        if self.ownership_record()?.lines().any(|line| line == hash)
            || (self.source.try_exists()? && read_hook(&self.source)? == contents)
        {
            Ok(ResourceState::Correct)
        } else {
            Ok(ResourceState::Incorrect {
                current: "foreign or modified hook".into(),
            })
        }
    }

    fn remove_verified_hook(&self) -> ResourceResult<ResourceChange> {
        let saved = self.target.with_extension(REMOVAL_EXTENSION);
        rename_noreplace(&self.target, &saved)
            .with_context(|| format!("saving {} before removal", self.target.display()))?;
        self.finish_removal(&saved)
    }

    fn finish_removal(&self, saved: &Path) -> ResourceResult<ResourceChange> {
        // Check the entry we actually moved, not the name another writer can replace.
        let remove = || -> anyhow::Result<()> {
            anyhow::ensure!(
                self.ownership_state_at(saved)? == ResourceState::Correct,
                "hook {} changed during removal",
                self.target.display()
            );
            crate::infra::fs::remove_file_if_present(
                &self.ownership_path(),
                "stat hook ownership",
            )?;
            Ok(())
        };
        if let Err(error) = remove() {
            if let Err(restore) = rename_noreplace(saved, &self.target) {
                return Err(error
                    .context(format!(
                        "could not restore {} ({restore}); original entry saved at {}",
                        self.target.display(),
                        saved.display()
                    ))
                    .into());
            }
            return Err(error.into());
        }
        std::fs::remove_file(saved)
            .with_context(|| format!("removing saved hook {}", saved.display()))?;
        Ok(ResourceChange::Applied)
    }
}

impl IntrinsicState for HookFileResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        self.check_pending_removal()?;
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
            Some(metadata) if !metadata.is_file() && !metadata.is_symlink() => {
                return Ok(ResourceState::Invalid {
                    reason:
                        "hook is not a regular file; move it aside before installing dotfiles hooks"
                            .into(),
                });
            }
            None => {
                drop(self.ownership_record()?);
                return Ok(ResourceState::Missing);
            }
            Some(_) => {}
        }
        let src_content = read_hook(&self.source)?;
        let dst_content = read_hook(&self.target)?;
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
#[path = "tests/hook.rs"]
mod tests;
