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
    /// Detect the wrapper type from the `DOTFILES_WRAPPER` environment
    /// variable, falling back to platform heuristics.
    #[must_use]
    pub fn detect(
        env: &dyn crate::infra::env::Env,
        platform: crate::infra::platform::Platform,
    ) -> Self {
        match env.var("DOTFILES_WRAPPER").as_deref() {
            Some("sh") => Self::Sh,
            Some("pwsh") => Self::Pwsh,
            _ => {
                if platform.is_windows() {
                    Self::Pwsh
                } else {
                    Self::Sh
                }
            }
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

    fn target_contents(&self) -> Result<Option<Vec<u8>>> {
        let Some(metadata) = self.target_metadata()? else {
            return Ok(None);
        };
        anyhow::ensure!(
            metadata.is_file() && !metadata.is_symlink(),
            "preserving foreign launcher {}; move it aside before installing",
            self.target.display()
        );
        Ok(Some(crate::infra::fs::read_bytes(&self.target)?))
    }

    fn owned(&self, contents: &[u8]) -> Result<bool> {
        // Adopt an exact copy installed before ownership records existed.
        if contents == self.content.as_bytes() {
            return Ok(true);
        }
        let recorded = match std::fs::read_to_string(self.ownership_path()) {
            Ok(recorded) => recorded,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        Ok(recorded
            .lines()
            .any(|line| line == crate::infra::fs::content_hash(contents)))
    }

    fn verify_baseline(&self, before: Option<&[u8]>) -> Result<()> {
        anyhow::ensure!(
            self.target_contents()?.as_deref() == before,
            "launcher {} changed during installation; retry",
            self.target.display()
        );
        Ok(())
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
        // Retain both outcomes if publication or the final ledger write is interrupted.
        let hash = crate::infra::fs::content_hash(&self.content);
        let mut ownership = format!("{hash}\n");
        if let Some(contents) = &before {
            ownership.push_str(&crate::infra::fs::content_hash(contents));
            ownership.push('\n');
        }
        crate::infra::fs::write_atomic(&self.ownership_path(), ownership)?;
        self.verify_baseline(before.as_deref())?;
        if before.is_none() {
            std::fs::hard_link(staged.path(), &self.target)?;
        } else {
            crate::infra::fs::rename_into_place(staged.path(), &self.target)?;
            staged.persist();
        }
        crate::infra::fs::write_atomic(&self.ownership_path(), format!("{hash}\n"))?;
        Ok(ResourceChange::Applied)
    }
}

impl RemovableResource for WrapperResource {
    fn removal_state(&self) -> ResourceResult<ResourceState> {
        let Some(contents) = self.target_contents()? else {
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

    fn remove(&self) -> ResourceResult<ResourceChange> {
        match self.removal_state()? {
            ResourceState::Correct => {
                crate::infra::fs::remove_file(&self.target)?;
                crate::infra::fs::remove_file_if_present(
                    &self.ownership_path(),
                    "stat launcher ownership",
                )?;
                Ok(ResourceChange::Applied)
            }
            ResourceState::Missing
            | ResourceState::Incorrect { .. }
            | ResourceState::Invalid { .. }
            | ResourceState::Unknown { .. } => Ok(ResourceChange::AlreadyCorrect),
        }
    }
}

impl IntrinsicState for WrapperResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
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
        let current = crate::infra::fs::read_string(&self.target)?;
        if !self.owned(current.as_bytes())? {
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

        if current == self.content && permissions_correct {
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

        let state = r.current_state().unwrap();
        assert_eq!(state, ResourceState::Correct);
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
            assert_eq!(new.remove().unwrap(), ResourceChange::Applied);
            assert!(!new.ownership_path().exists());
            std::fs::write(&new.target, &new.content).unwrap();
            #[cfg(unix)]
            crate::infra::fs::set_executable(&new.target).unwrap();
            assert_eq!(new.current_state().unwrap(), ResourceState::Correct);
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
