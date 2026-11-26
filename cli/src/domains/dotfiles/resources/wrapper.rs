//! CLI wrapper installation resource.
//!
//! Installs a small script on the user's `PATH` (`~/.local/bin/dotfiles` on
//! Unix and `~/.local/bin/dotfiles.cmd` on Windows) that delegates to the
//! repository's wrapper script so `dotfiles` can be invoked from anywhere.
use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::engine::{
    IntrinsicState, RemovableResource, Resource, ResourceChange, ResourceResult, ResourceState,
};

/// Which wrapper script to install on the user's `PATH`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WrapperType {
    /// POSIX shell wrapper (`dotfiles.sh`).
    Sh,
    /// `PowerShell` wrapper (`dotfiles.ps1`).
    Pwsh,
}

impl WrapperType {
    /// Select a launcher format the target OS can execute.
    #[must_use]
    pub(crate) const fn detect(platform: crate::infra::platform::Platform) -> Self {
        if platform.is_windows() {
            Self::Pwsh
        } else {
            Self::Sh
        }
    }
}

/// A resource that installs a CLI wrapper script on the user's `PATH`.
#[derive(Debug)]
pub(crate) struct WrapperResource {
    /// Where the wrapper script will be installed.
    target: PathBuf,
    /// Home directory used to abbreviate user-facing target paths.
    display_home: PathBuf,
    /// Expected file content.
    content: String,
}

impl WrapperResource {
    /// Create a new wrapper resource.
    ///
    /// `wrapper_type` selects the shell flavour, `dotfiles_root` is the
    /// repository root (used to build the path to the real wrapper), and
    /// `home` is the user's home directory (the install target is
    /// `$HOME/.local/bin/`).
    ///
    /// On Unix this produces an extensionless shell script.  On Windows
    /// it produces a `.cmd` shim that directly invokes the repository's
    /// `dotfiles.ps1`.
    #[must_use]
    pub(crate) fn new(wrapper_type: WrapperType, dotfiles_root: &Path, home: &Path) -> Self {
        let bin_dir = home.join(".local").join("bin");

        let (target, content) = match wrapper_type {
            WrapperType::Sh => {
                let target = bin_dir.join("dotfiles");
                let root = sh_single_quote(&dotfiles_root.display().to_string());
                let content = format!(
                    "#!/bin/sh\n\
                      # Installed by dotfiles \u{2014} do not edit.\n\
                      if [ -z \"${{DOTFILES_ROOT:-}}\" ]; then\n\
                      \tDOTFILES_ROOT={root}\n\
                      fi\n\
                      export DOTFILES_ROOT\n\
                      exec \"$DOTFILES_ROOT/dotfiles.sh\" \"$@\"\n"
                );
                (target, content)
            }
            WrapperType::Pwsh => {
                let target = bin_dir.join("dotfiles.cmd");
                let root = cmd_set_value_escape(&dotfiles_root.display().to_string());
                let sep = std::path::MAIN_SEPARATOR;
                // Probe for pwsh (PowerShell 7+) at runtime, falling back to
                // Windows PowerShell if unavailable.
                let content = format!(
                    "@echo off\r\n\
                      rem Installed by dotfiles \u{2014} do not edit.\r\n\
                      if not defined DOTFILES_ROOT set \"DOTFILES_ROOT={root}\"\r\n\
                      where /q pwsh\r\n\
                      if errorlevel 1 goto dotfiles_windows_powershell\r\n\
                      pwsh -NoProfile -ExecutionPolicy Bypass \
                      -File \"%DOTFILES_ROOT%{sep}dotfiles.ps1\" %*\r\n\
                      exit /b %ERRORLEVEL%\r\n\
                      :dotfiles_windows_powershell\r\n\
                      powershell.exe -NoProfile -ExecutionPolicy Bypass \
                      -File \"%DOTFILES_ROOT%{sep}dotfiles.ps1\" %*\r\n"
                );
                (target, content)
            }
        };

        Self {
            target,
            display_home: home.to_path_buf(),
            content,
        }
    }

    fn target_metadata(&self) -> Result<Option<std::fs::Metadata>> {
        crate::infra::fs::symlink_metadata_optional(&self.target, "stat wrapper")
    }

    fn ownership_path(&self) -> PathBuf {
        let mut name = self.target.as_os_str().to_os_string();
        name.push(".dotfiles-owned");
        name.into()
    }

    fn removal_staging_path(&self) -> PathBuf {
        let mut name = self.target.as_os_str().to_os_string();
        name.push(".dotfiles-removing");
        name.into()
    }

    fn contents_at(path: &Path) -> Result<Option<Vec<u8>>> {
        let Some(metadata) = crate::infra::fs::symlink_metadata_optional(path, "stat wrapper")?
        else {
            return Ok(None);
        };
        anyhow::ensure!(
            metadata.is_file() && !metadata.is_symlink(),
            "preserving foreign launcher {}; move it aside before installing",
            path.display()
        );
        Ok(Some(crate::infra::fs::read_bytes(path)?))
    }

    fn target_contents(&self) -> Result<Option<Vec<u8>>> {
        Self::contents_at(&self.target)
    }

    fn ownership_state_at(&self, path: &Path) -> ResourceResult<ResourceState> {
        let Some(contents) = Self::contents_at(path)? else {
            return Ok(ResourceState::Missing);
        };
        Ok(if self.owned(&contents)? {
            ResourceState::Correct
        } else {
            ResourceState::Incorrect {
                current: "foreign or modified launcher".into(),
            }
        })
    }

    fn pending_removal_state(&self) -> ResourceResult<ResourceState> {
        let saved = self.removal_staging_path();
        match self.ownership_state_at(&saved)? {
            ResourceState::Missing => Ok(ResourceState::Missing),
            ResourceState::Correct => Ok(ResourceState::Correct),
            ResourceState::Incorrect { .. }
            | ResourceState::Invalid { .. }
            | ResourceState::Unknown { .. } => Err(anyhow::anyhow!(
                "preserving foreign staged launcher {}; move it aside before retrying",
                saved.display()
            )
            .into()),
        }
    }

    fn remove_verified_entry(&self, source: &Path, restore_to: &Path) -> Result<()> {
        use anyhow::Context as _;

        let parent = source
            .parent()
            .ok_or_else(|| anyhow::anyhow!("launcher {} has no parent", source.display()))?;
        let mut staging_dir = crate::infra::fs::PrivateTempDir::create(parent, ".dotfiles-remove")
            .with_context(|| {
                format!(
                    "creating private removal directory beside {}",
                    source.display()
                )
            })?;
        let isolated = staging_dir.path().join("entry");
        crate::infra::fs::rename_noreplace(source, &isolated)
            .with_context(|| format!("staging {} before removal", source.display()))?;

        let verify_and_remove = || -> Result<()> {
            anyhow::ensure!(
                self.ownership_state_at(&isolated)? == ResourceState::Correct,
                "launcher {} changed during removal",
                source.display()
            );
            std::fs::remove_file(&isolated)
                .with_context(|| format!("removing staged launcher {}", isolated.display()))?;
            Ok(())
        };
        if let Err(error) = verify_and_remove() {
            if let Err(restore) = crate::infra::fs::rename_noreplace(&isolated, restore_to) {
                // Keep the private directory on disk so PrivateTempDir::drop does not delete
                // an unverified entry that could not be moved back.
                staging_dir.persist();
                return Err(error.context(format!(
                    "could not restore {} ({restore}); original entry saved at {}",
                    restore_to.display(),
                    isolated.display()
                )));
            }
            return Err(error);
        }
        Ok(())
    }

    fn finish_removal(&self, saved: &Path) -> ResourceResult<ResourceChange> {
        // Isolate and re-verify the moved entry inside a private directory before unlinking,
        // rather than unlinking the predictable staging path directly. Keep the ownership
        // record intact until the staged launcher itself is deleted so an interrupted removal
        // still recognizes and cleans up older managed copies.
        self.remove_verified_entry(saved, &self.target)?;
        crate::infra::fs::remove_file_if_present(
            &self.ownership_path(),
            "stat launcher ownership",
        )?;
        Ok(ResourceChange::Applied)
    }

    fn owned(&self, contents: &[u8]) -> Result<bool> {
        // Adopt an exact copy installed before ownership records existed.
        if contents == self.content.as_bytes() {
            return Ok(true);
        }
        let hash = crate::infra::fs::content_hash(contents);
        Ok(self.ownership_record()?.lines().any(|line| line == hash))
    }

    fn ownership_record(&self) -> Result<String> {
        match std::fs::read_to_string(self.ownership_path()) {
            Ok(recorded) => Ok(recorded),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(error) => Err(error.into()),
        }
    }

    fn verify_baseline(&self, before: Option<&[u8]>) -> Result<()> {
        anyhow::ensure!(
            self.target_contents()?.as_deref() == before,
            "launcher {} changed during installation; retry",
            self.target.display()
        );
        Ok(())
    }
    fn apply_with_before_staged_cleanup(
        &self,
        before_staged_cleanup: impl FnOnce(),
    ) -> ResourceResult<ResourceChange> {
        let before = self.target_contents()?;
        if let Some(contents) = &before
            && !self.owned(contents)?
        {
            return Err(anyhow::anyhow!(
                "preserving foreign or modified launcher {}; move it aside before installing",
                self.target.display()
            )
            .into());
        }
        let pending_removal = self.pending_removal_state()?;
        crate::infra::fs::ensure_parent_dir(&self.target)?;
        let parent = self
            .target
            .parent()
            .ok_or_else(|| anyhow::anyhow!("launcher has no parent"))?;
        let (mut staged, file) =
            crate::infra::fs::TempGuard::create_unique_file(parent, ".dotfiles-launcher", "tmp")?;
        drop(file);
        std::fs::write(staged.path(), &self.content)?;
        #[cfg(unix)]
        crate::infra::fs::set_executable(staged.path())?;
        self.verify_baseline(before.as_deref())?;
        // Remove any owned leftover from an interrupted removal before updating
        // the ownership record so older managed versions remain verified.
        if pending_removal == ResourceState::Correct {
            before_staged_cleanup();
            let saved = self.removal_staging_path();
            self.remove_verified_entry(&saved, &saved)?;
        }
        crate::infra::fs::publish_with_ownership(
            &self.ownership_path(),
            before.as_deref(),
            self.content.as_bytes(),
            || {
                self.verify_baseline(before.as_deref())?;
                if before.is_none() {
                    std::fs::hard_link(staged.path(), &self.target)?;
                } else {
                    crate::infra::fs::rename_into_place(staged.path(), &self.target)?;
                    staged.persist();
                }
                Ok(())
            },
        )
        .map(|()| ResourceChange::Applied)
    }

    fn remove_with_before_staged_cleanup(
        &self,
        before_staged_cleanup: impl FnOnce(),
    ) -> ResourceResult<ResourceChange> {
        use anyhow::Context as _;

        let target_state = self.ownership_state_at(&self.target)?;
        let pending_state = self.pending_removal_state()?;
        let saved = self.removal_staging_path();
        match (target_state, pending_state) {
            (ResourceState::Correct, ResourceState::Correct) => {
                before_staged_cleanup();
                self.remove_verified_entry(&saved, &saved)?;
                crate::infra::fs::rename_noreplace(&self.target, &saved)
                    .with_context(|| format!("saving {} before removal", self.target.display()))?;
                self.finish_removal(&saved)
            }
            (ResourceState::Correct, ResourceState::Missing) => {
                crate::infra::fs::rename_noreplace(&self.target, &saved)
                    .with_context(|| format!("saving {} before removal", self.target.display()))?;
                self.finish_removal(&saved)
            }
            (
                ResourceState::Missing
                | ResourceState::Incorrect { .. }
                | ResourceState::Invalid { .. }
                | ResourceState::Unknown { .. },
                ResourceState::Correct,
            ) => {
                before_staged_cleanup();
                self.remove_verified_entry(&saved, &saved)?;
                crate::infra::fs::remove_file_if_present(
                    &self.ownership_path(),
                    "stat launcher ownership",
                )?;
                Ok(ResourceChange::Applied)
            }
            _ => Ok(ResourceChange::AlreadyCorrect),
        }
    }
}

fn sh_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn cmd_set_value_escape(value: &str) -> String {
    value.replace('%', "%%")
}

impl Resource for WrapperResource {
    fn description(&self) -> String {
        format!(
            "wrapper \u{2192} {}",
            crate::infra::logging::abbreviate_home(&self.target, &self.display_home)
        )
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        self.apply_with_before_staged_cleanup(|| {})
    }
}

impl RemovableResource for WrapperResource {
    fn removal_state(&self) -> ResourceResult<ResourceState> {
        let target_state = self.ownership_state_at(&self.target)?;
        let pending_state = self.pending_removal_state()?;
        if target_state == ResourceState::Correct || pending_state == ResourceState::Correct {
            Ok(ResourceState::Correct)
        } else {
            Ok(target_state)
        }
    }

    fn remove(&self) -> ResourceResult<ResourceChange> {
        self.remove_with_before_staged_cleanup(|| {})
    }
}

impl IntrinsicState for WrapperResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        let pending_removal = self.pending_removal_state()?;
        let Some(metadata) = self.target_metadata()? else {
            return Ok(ResourceState::Missing);
        };

        if metadata.file_type().is_symlink() {
            return Ok(ResourceState::Invalid {
                reason: "foreign launcher symlink; move it aside before installing".into(),
            });
        }
        if !metadata.is_file() {
            return Ok(ResourceState::Invalid {
                reason: "launcher target is not a regular file".into(),
            });
        }
        let current = crate::infra::fs::read_bytes(&self.target)?;
        if !self.owned(&current)? {
            return Ok(ResourceState::Invalid {
                reason: "foreign or modified launcher; move it aside before installing".into(),
            });
        }

        #[cfg(unix)]
        let permissions_correct = {
            use std::os::unix::fs::PermissionsExt as _;
            metadata.permissions().mode() & 0o777 == 0o755
        };
        #[cfg(not(unix))]
        let permissions_correct = true;

        // A pending two-hash record or an interrupted removal staging file must
        // be finalized on retry, even after the launcher bytes themselves match.
        if current.as_slice() == self.content.as_bytes()
            && permissions_correct
            && pending_removal == ResourceState::Missing
            && self
                .ownership_record()?
                .lines()
                .eq([crate::infra::fs::content_hash(&current).as_str()])
        {
            Ok(ResourceState::Correct)
        } else {
            Ok(ResourceState::Incorrect {
                current: "managed launcher needs updating".into(),
            })
        }
    }
}

#[cfg(test)]
#[path = "tests/wrapper.rs"]
mod tests;
