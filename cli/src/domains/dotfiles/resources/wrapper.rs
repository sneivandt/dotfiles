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
pub enum WrapperType {
    /// POSIX shell wrapper (`dotfiles.sh`).
    Sh,
    /// `PowerShell` wrapper (`dotfiles.ps1`).
    Pwsh,
}

impl WrapperType {
    /// Select a launcher format the target OS can execute.
    #[must_use]
    pub const fn detect(platform: crate::infra::platform::Platform) -> Self {
        if platform.is_windows() {
            Self::Pwsh
        } else {
            Self::Sh
        }
    }
}

/// A resource that installs a CLI wrapper script on the user's `PATH`.
#[derive(Debug)]
pub struct WrapperResource {
    /// Where the wrapper script will be installed.
    target: PathBuf,
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
    pub fn new(wrapper_type: WrapperType, dotfiles_root: &Path, home: &Path) -> Self {
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

        Self { target, content }
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
        format!("wrapper \u{2192} {}", self.target.display())
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
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn make_sh_resource(root: &Path, home: &Path) -> WrapperResource {
        WrapperResource::new(WrapperType::Sh, root, home)
    }

    fn make_pwsh_resource(root: &Path, home: &Path) -> WrapperResource {
        WrapperResource::new(WrapperType::Pwsh, root, home)
    }

    // ── Description ──────────────────────────────────────────────────

    #[test]
    fn sh_description_includes_target_path() {
        let r = make_sh_resource(Path::new("/repo"), Path::new("/home/user"));
        let expected = Path::new(".local").join("bin").join("dotfiles");
        assert!(
            r.description()
                .contains(expected.to_string_lossy().as_ref()),
            "got: {}",
            r.description()
        );
    }

    #[test]
    fn pwsh_description_includes_target_path() {
        let r = make_pwsh_resource(Path::new("/repo"), Path::new("/home/user"));
        let expected = Path::new(".local").join("bin").join("dotfiles");
        assert!(
            r.description()
                .contains(expected.to_string_lossy().as_ref()),
            "got: {}",
            r.description()
        );
    }

    // ── Content generation ───────────────────────────────────────────

    #[test]
    fn sh_content_contains_shebang_and_dotfiles_root() {
        let r = make_sh_resource(Path::new("/repo"), Path::new("/home/user"));
        assert!(r.content.starts_with("#!/bin/sh\n"));
        assert!(r.content.contains("DOTFILES_ROOT='/repo'"));
        assert!(
            r.content
                .contains("exec \"$DOTFILES_ROOT/dotfiles.sh\" \"$@\"")
        );
    }

    #[test]
    fn sh_content_single_quotes_dotfiles_root() {
        let r = make_sh_resource(
            Path::new("/repo with 'quote/$HOME"),
            Path::new("/home/user"),
        );
        assert!(
            r.content
                .contains("DOTFILES_ROOT='/repo with '\\''quote/$HOME'")
        );
    }

    #[test]
    fn pwsh_content_delegates_to_repo_wrapper() {
        let r = make_pwsh_resource(Path::new("/repo"), Path::new("/home/user"));
        assert!(
            r.content.starts_with("@echo off"),
            "cmd shim must start with @echo off"
        );
        assert!(
            r.content.contains("DOTFILES_ROOT=/repo"),
            "cmd shim must set DOTFILES_ROOT"
        );
        assert!(
            r.content.contains("dotfiles.ps1"),
            "cmd shim must delegate to dotfiles.ps1"
        );
    }

    #[test]
    fn pwsh_content_probes_for_pwsh_at_runtime() {
        let r = make_pwsh_resource(Path::new("/repo"), Path::new("/home/user"));
        assert!(
            r.content.contains("where /q pwsh"),
            "cmd shim must probe for pwsh at runtime"
        );
        assert!(
            r.content.contains("powershell.exe"),
            "cmd shim must fall back to powershell.exe"
        );
    }

    #[test]
    fn pwsh_content_escapes_percent_in_default_root() {
        let r = make_pwsh_resource(
            Path::new(r"C:\Users\%USERNAME%\dotfiles"),
            Path::new("/home/user"),
        );
        assert!(
            r.content.contains(r"C:\Users\%%USERNAME%%\dotfiles"),
            "cmd shim must escape percent signs in set assignments"
        );
        assert!(
            !r.content.contains("&& ("),
            "cmd shim should not wrap invocation in a parenthesized block"
        );
    }

    // ── State detection ──────────────────────────────────────────────

    #[test]
    fn state_missing_when_file_does_not_exist() {
        let home = TempDir::new().unwrap();
        let r = make_sh_resource(Path::new("/repo"), home.path());
        let state = r.current_state().unwrap();
        assert_eq!(state, ResourceState::Missing);
    }

    #[test]
    fn state_correct_when_content_matches() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path();
        let r = make_sh_resource(Path::new("/repo"), home);

        std::fs::create_dir_all(r.target.parent().unwrap()).unwrap();
        std::fs::write(&r.target, &r.content).unwrap();
        #[cfg(unix)]
        crate::infra::fs::set_executable(&r.target).unwrap();

        assert!(
            matches!(r.current_state().unwrap(), ResourceState::Incorrect { .. }),
            "legacy copies must acquire an ownership record before the next template upgrade"
        );
        r.apply().unwrap();
        assert_eq!(r.current_state().unwrap(), ResourceState::Correct);
    }

    #[cfg(unix)]
    #[test]
    fn state_incorrect_when_wrapper_is_not_executable() {
        use std::os::unix::fs::PermissionsExt as _;

        let tmp = TempDir::new().unwrap();
        let r = make_sh_resource(Path::new("/repo"), tmp.path());
        std::fs::create_dir_all(r.target.parent().unwrap()).unwrap();
        std::fs::write(&r.target, &r.content).unwrap();
        std::fs::set_permissions(&r.target, std::fs::Permissions::from_mode(0o644)).unwrap();

        assert!(matches!(
            r.current_state().unwrap(),
            ResourceState::Incorrect { .. }
        ));
    }

    #[test]
    fn state_invalid_when_content_is_foreign() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path();
        let r = make_sh_resource(Path::new("/repo"), home);

        std::fs::create_dir_all(r.target.parent().unwrap()).unwrap();
        std::fs::write(&r.target, "old content").unwrap();

        let state = r.current_state().unwrap();
        assert!(matches!(state, ResourceState::Invalid { .. }));
    }

    #[test]
    fn state_invalid_when_target_is_non_utf8_binary() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path();
        let r = make_sh_resource(Path::new("/repo"), home);

        std::fs::create_dir_all(r.target.parent().unwrap()).unwrap();
        std::fs::write(&r.target, [0x7f, b'E', b'L', b'F', 0xff, 0xfe]).unwrap();

        let state = r.current_state().unwrap();
        assert!(matches!(state, ResourceState::Invalid { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn state_broken_symlink_is_invalid_not_missing() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path();
        let r = make_sh_resource(Path::new("/repo"), home);

        std::fs::create_dir_all(r.target.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink("/nonexistent/dotfiles-wrapper", &r.target).unwrap();

        let state = r.current_state().unwrap();
        assert!(matches!(state, ResourceState::Invalid { .. }));
    }

    // ── Apply ────────────────────────────────────────────────────────

    #[test]
    fn apply_creates_file_with_correct_content() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path();
        let r = make_sh_resource(Path::new("/repo"), home);

        let result = r.apply().unwrap();
        assert_eq!(result, ResourceChange::Applied);

        let actual = std::fs::read_to_string(&r.target).unwrap();
        assert_eq!(actual, r.content);
    }

    #[cfg(unix)]
    #[test]
    fn apply_sets_executable_permission() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new().unwrap();
        let home = tmp.path();
        let r = make_sh_resource(Path::new("/repo"), home);

        r.apply().unwrap();

        let mode = std::fs::metadata(&r.target).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }

    #[test]
    fn foreign_and_modified_launchers_are_preserved() {
        for wrapper_type in [WrapperType::Sh, WrapperType::Pwsh] {
            for installed in [false, true] {
                let home = TempDir::new().unwrap();
                let r = WrapperResource::new(wrapper_type, Path::new("/repo"), home.path());
                if installed {
                    r.apply().unwrap();
                }
                std::fs::create_dir_all(r.target.parent().unwrap()).unwrap();
                std::fs::write(&r.target, "user-owned content").unwrap();
                assert!(matches!(
                    r.current_state().unwrap(),
                    ResourceState::Invalid { .. }
                ));
                assert!(r.apply().is_err());
                assert_eq!(r.remove().unwrap(), ResourceChange::AlreadyCorrect);
                assert_eq!(
                    std::fs::read_to_string(&r.target).unwrap(),
                    "user-owned content"
                );
            }
        }
    }

    #[test]
    fn owned_launchers_upgrade_and_legacy_exact_copies_are_adopted() {
        for wrapper_type in [WrapperType::Sh, WrapperType::Pwsh] {
            let home = TempDir::new().unwrap();
            let old = WrapperResource::new(wrapper_type, Path::new("/old"), home.path());
            old.apply().unwrap();
            let new = WrapperResource::new(wrapper_type, Path::new("/new"), home.path());
            assert!(matches!(
                new.current_state().unwrap(),
                ResourceState::Incorrect { .. }
            ));
            new.apply().unwrap();
            assert_eq!(new.current_state().unwrap(), ResourceState::Correct);
            assert_eq!(std::fs::read_to_string(&new.target).unwrap(), new.content);
            // Both entries are valid recovery outcomes until publication is finalized.
            std::fs::write(
                new.ownership_path(),
                format!(
                    "{}\n{}\n",
                    crate::infra::fs::content_hash(&old.content),
                    crate::infra::fs::content_hash(&new.content)
                ),
            )
            .unwrap();
            assert!(new.owned(old.content.as_bytes()).unwrap());
            assert!(new.owned(new.content.as_bytes()).unwrap());
            assert!(matches!(
                new.current_state().unwrap(),
                ResourceState::Incorrect { .. }
            ));
            new.apply().unwrap();
            assert_eq!(new.current_state().unwrap(), ResourceState::Correct);
            std::fs::write(&new.target, &old.content).unwrap();
            assert_eq!(new.remove().unwrap(), ResourceChange::AlreadyCorrect);
            assert_eq!(std::fs::read_to_string(&new.target).unwrap(), old.content);
            std::fs::write(&new.target, &new.content).unwrap();
            assert_eq!(new.remove().unwrap(), ResourceChange::Applied);
            assert!(!new.ownership_path().exists());
            std::fs::write(&new.target, &new.content).unwrap();
            #[cfg(unix)]
            crate::infra::fs::set_executable(&new.target).unwrap();
            assert!(matches!(
                new.current_state().unwrap(),
                ResourceState::Incorrect { .. }
            ));
            new.apply().unwrap();
            assert!(new.ownership_path().exists());
        }
    }

    #[test]
    fn baseline_recheck_preserves_intervening_changes() {
        let home = TempDir::new().unwrap();
        let r = make_sh_resource(Path::new("/repo"), home.path());
        r.apply().unwrap();
        let before = std::fs::read(&r.target).unwrap();
        std::fs::write(&r.target, "concurrent user edit").unwrap();
        assert!(r.verify_baseline(Some(&before)).is_err());
        assert!(r.verify_baseline(None).is_err());
        assert_eq!(
            std::fs::read_to_string(&r.target).unwrap(),
            "concurrent user edit"
        );
        assert!(r.verify_baseline(Some(b"concurrent user edit")).is_ok());
    }

    #[test]
    fn ownership_inspection_errors_are_not_treated_as_missing_records() {
        let home = TempDir::new().unwrap();
        let old = make_sh_resource(Path::new("/old"), home.path());
        old.apply().unwrap();
        std::fs::write(old.ownership_path(), [0xff]).unwrap();
        let new = make_sh_resource(Path::new("/new"), home.path());
        assert!(new.current_state().is_err());
        assert!(new.apply().is_err());
        assert!(new.remove().is_err());
        assert_eq!(std::fs::read_to_string(&old.target).unwrap(), old.content);
        assert_eq!(std::fs::read(old.ownership_path()).unwrap(), [0xff]);
    }

    // ── Remove ───────────────────────────────────────────────────────

    #[test]
    fn remove_deletes_existing_file() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path();
        let r = make_sh_resource(Path::new("/repo"), home);

        std::fs::create_dir_all(r.target.parent().unwrap()).unwrap();
        std::fs::write(&r.target, &r.content).unwrap();

        let result = r.remove().unwrap();
        assert_eq!(result, ResourceChange::Applied);
        assert!(!r.target.exists());
        assert!(!r.removal_staging_path().exists());
    }

    #[test]
    fn finish_removal_restores_modified_staged_launcher() {
        let tmp = TempDir::new().unwrap();
        let r = make_sh_resource(Path::new("/repo"), tmp.path());
        std::fs::create_dir_all(r.target.parent().unwrap()).unwrap();
        let saved = r.removal_staging_path();
        std::fs::write(&saved, "user replaced launcher").unwrap();

        assert!(
            r.finish_removal(&saved).is_err(),
            "modified staged launcher must fail verification"
        );
        assert!(
            !saved.exists(),
            "staged file must be moved back to target on verification failure"
        );
        assert_eq!(
            std::fs::read_to_string(&r.target).unwrap(),
            "user replaced launcher"
        );
    }

    #[test]
    fn interrupted_removal_recovers_owned_staged_launcher() {
        let home = TempDir::new().unwrap();
        let old = make_sh_resource(Path::new("/old"), home.path());
        old.apply().unwrap();

        // Simulate interruption after moving the launcher to `.dotfiles-removing`
        // while `.dotfiles-owned` is still retained on disk.
        let saved = old.removal_staging_path();
        std::fs::rename(&old.target, &saved).unwrap();
        assert!(!old.target.exists());
        assert!(saved.exists());
        assert!(old.ownership_path().exists());

        // Even with a newer expected launcher template, `.dotfiles-owned` still
        // proves ownership of the staged file so a retry finishes cleanup.
        let current = make_sh_resource(Path::new("/new"), home.path());
        assert_eq!(current.removal_state().unwrap(), ResourceState::Correct);
        assert_eq!(current.remove().unwrap(), ResourceChange::Applied);
        assert!(!current.target.exists());
        assert!(!saved.exists());
        assert!(!current.ownership_path().exists());
        assert_eq!(current.removal_state().unwrap(), ResourceState::Missing);
        assert_eq!(current.remove().unwrap(), ResourceChange::AlreadyCorrect);
    }

    #[test]
    fn reinstall_and_remove_succeed_after_interrupted_removal() {
        let home = TempDir::new().unwrap();
        let old = make_sh_resource(Path::new("/old"), home.path());
        old.apply().unwrap();
        let saved = old.removal_staging_path();
        std::fs::rename(&old.target, &saved).unwrap();

        // Reinstalling cleans up the owned staging file and installs the new launcher.
        let new = make_sh_resource(Path::new("/new"), home.path());
        assert_eq!(new.current_state().unwrap(), ResourceState::Missing);
        assert_eq!(new.apply().unwrap(), ResourceChange::Applied);
        assert!(!saved.exists());
        assert_eq!(new.current_state().unwrap(), ResourceState::Correct);
        assert_eq!(new.remove().unwrap(), ResourceChange::Applied);
        assert!(!new.target.exists());
        assert!(!saved.exists());
        assert!(!new.ownership_path().exists());

        // If both an owned launcher and an owned leftover `.dotfiles-removing`
        // exist together, `current_state` flags cleanup and `remove` removes both.
        new.apply().unwrap();
        std::fs::write(&saved, &new.content).unwrap();
        assert!(matches!(
            new.current_state().unwrap(),
            ResourceState::Incorrect { .. }
        ));
        assert_eq!(new.remove().unwrap(), ResourceChange::Applied);
        assert!(!new.target.exists());
        assert!(!saved.exists());
        assert!(!new.ownership_path().exists());
    }

    #[test]
    fn foreign_staged_removal_file_is_preserved() {
        let home = TempDir::new().unwrap();
        let r = make_sh_resource(Path::new("/repo"), home.path());
        std::fs::create_dir_all(r.target.parent().unwrap()).unwrap();
        let saved = r.removal_staging_path();
        std::fs::write(&saved, "foreign staged content").unwrap();

        assert!(r.current_state().is_err());
        assert!(r.apply().is_err());
        assert!(r.removal_state().is_err());
        assert!(r.remove().is_err());
        assert_eq!(
            std::fs::read_to_string(&saved).unwrap(),
            "foreign staged content"
        );
    }

    #[test]
    fn recovery_preserves_staged_file_swapped_before_unlink() {
        // 1. Recovery during `apply()` swaps `.dotfiles-removing` to an unowned file
        //    after the initial check and before staged cleanup.
        {
            let home = TempDir::new().unwrap();
            let old = make_sh_resource(Path::new("/old"), home.path());
            old.apply().unwrap();
            let saved = old.removal_staging_path();
            std::fs::rename(&old.target, &saved).unwrap();

            let new = make_sh_resource(Path::new("/new"), home.path());
            let err = new
                .apply_with_before_staged_cleanup(|| {
                    std::fs::write(&saved, "swapped user file").unwrap();
                })
                .unwrap_err();
            assert!(
                format!("{err:#}").contains("changed during removal"),
                "unexpected error: {err:#}"
            );
            assert_eq!(
                std::fs::read_to_string(&saved).unwrap(),
                "swapped user file",
                "swapped staged file must be restored rather than unlinked"
            );
            assert!(!new.target.exists(), "new launcher must not be published");
            assert!(
                new.ownership_path().exists(),
                "ownership record must remain intact after failed recovery"
            );
        }

        // 2. Recovery during `remove()` when both target and `.dotfiles-removing`
        //    are owned, and `.dotfiles-removing` is swapped before unlink.
        {
            let home = TempDir::new().unwrap();
            let r = make_sh_resource(Path::new("/repo"), home.path());
            r.apply().unwrap();
            let saved = r.removal_staging_path();
            std::fs::write(&saved, &r.content).unwrap();

            let err = r
                .remove_with_before_staged_cleanup(|| {
                    std::fs::write(&saved, "swapped before dual remove").unwrap();
                })
                .unwrap_err();
            assert!(
                format!("{err:#}").contains("changed during removal"),
                "unexpected error: {err:#}"
            );
            assert_eq!(
                std::fs::read_to_string(&saved).unwrap(),
                "swapped before dual remove"
            );
            assert_eq!(std::fs::read_to_string(&r.target).unwrap(), r.content);
            assert!(r.ownership_path().exists());
        }

        // 3. Recovery during `remove()` when target is foreign and `.dotfiles-removing`
        //    is owned, then `.dotfiles-removing` is swapped before unlink.
        {
            let home = TempDir::new().unwrap();
            let r = make_sh_resource(Path::new("/repo"), home.path());
            r.apply().unwrap();
            let saved = r.removal_staging_path();
            std::fs::rename(&r.target, &saved).unwrap();
            std::fs::write(&r.target, "foreign live launcher").unwrap();

            let err = r
                .remove_with_before_staged_cleanup(|| {
                    std::fs::write(&saved, "swapped before foreign-target remove").unwrap();
                })
                .unwrap_err();
            assert!(
                format!("{err:#}").contains("changed during removal"),
                "unexpected error: {err:#}"
            );
            assert_eq!(
                std::fs::read_to_string(&saved).unwrap(),
                "swapped before foreign-target remove"
            );
            assert_eq!(
                std::fs::read_to_string(&r.target).unwrap(),
                "foreign live launcher"
            );
            assert!(r.ownership_path().exists());
        }
    }

    #[test]
    fn finish_removal_persists_isolated_entry_when_restore_target_exists() {
        let home = TempDir::new().unwrap();
        let r = make_sh_resource(Path::new("/repo"), home.path());
        std::fs::create_dir_all(r.target.parent().unwrap()).unwrap();
        let saved = r.removal_staging_path();
        std::fs::write(&saved, "unowned entry").unwrap();
        std::fs::write(&r.target, "existing target").unwrap();

        let err = r.finish_removal(&saved).unwrap_err();
        let rendered = format!("{err:#}");
        let mut preserved_dirs: Vec<PathBuf> = std::fs::read_dir(r.target.parent().unwrap())
            .unwrap()
            .filter_map(|entry| {
                let path = entry.unwrap().path();
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(".dotfiles-remove-"))
                    .then_some(path)
            })
            .collect();
        assert_eq!(
            preserved_dirs.len(),
            1,
            "unrestorable entry must be preserved in a single private staging dir: {rendered}"
        );
        let preserved = preserved_dirs.pop().unwrap().join("entry");
        assert!(
            rendered.contains(&preserved.display().to_string()),
            "error must name preserved isolated entry {}: {rendered}",
            preserved.display()
        );
        assert_eq!(
            std::fs::read_to_string(&preserved).unwrap(),
            "unowned entry",
            "persisted private copy must remain readable"
        );
        assert_eq!(
            std::fs::read_to_string(&r.target).unwrap(),
            "existing target"
        );
    }

    #[test]
    fn remove_returns_already_correct_when_absent() {
        let home = TempDir::new().unwrap();
        let r = make_sh_resource(Path::new("/repo"), home.path());
        let result = r.remove().unwrap();
        assert_eq!(result, ResourceChange::AlreadyCorrect);
    }

    #[cfg(unix)]
    #[test]
    fn foreign_symlinks_are_preserved_on_apply_and_remove() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path();
        let r = make_sh_resource(Path::new("/repo"), home);

        std::fs::create_dir_all(r.target.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink("/nonexistent/dotfiles-wrapper", &r.target).unwrap();

        assert!(r.apply().is_err());
        assert!(r.remove().is_err());
        assert_eq!(
            std::fs::read_link(&r.target).unwrap(),
            Path::new("/nonexistent/dotfiles-wrapper")
        );
    }

    // ── Target paths ─────────────────────────────────────────────────

    #[test]
    fn sh_target_is_local_bin_dotfiles() {
        let r = make_sh_resource(Path::new("/repo"), Path::new("/home/user"));
        assert_eq!(r.target, PathBuf::from("/home/user/.local/bin/dotfiles"));
    }

    #[test]
    fn pwsh_target_is_local_bin_dotfiles_cmd() {
        let r = make_pwsh_resource(Path::new("/repo"), Path::new("/home/user"));
        assert_eq!(
            r.target,
            PathBuf::from("/home/user/.local/bin/dotfiles.cmd")
        );
    }
}
