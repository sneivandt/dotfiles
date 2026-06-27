//! CLI wrapper installation resource.
//!
//! Installs a small script on the user's `PATH` (`~/.local/bin/dotfiles` on
//! Unix and `~/.local/bin/dotfiles.cmd` on Windows) that delegates to the
//! repository's wrapper script so `dotfiles` can be invoked from anywhere.
use anyhow::Result;
use std::path::{Path, PathBuf};

use super::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};

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
    pub fn detect(platform: crate::platform::Platform) -> Self {
        match std::env::var("DOTFILES_WRAPPER").as_deref() {
            Ok("sh") => Self::Sh,
            Ok("pwsh") => Self::Pwsh,
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
        crate::fs::symlink_metadata_optional(&self.target, "stat wrapper")
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
        if let Some(metadata) = self.target_metadata()? {
            if metadata.file_type().is_symlink() {
                crate::fs::remove_file(&self.target)?;
            } else if metadata.is_dir() {
                return Err(crate::error::ResourceError::conflicting_state(
                    self.description(),
                    "wrapper file",
                    "directory",
                ));
            }
        }

        crate::fs::write_with_parent(&self.target, &self.content)?;

        #[cfg(unix)]
        crate::fs::set_executable(&self.target)?;

        Ok(ResourceChange::Applied)
    }

    fn remove(&self) -> ResourceResult<ResourceChange> {
        match self.target_metadata()? {
            Some(metadata) if metadata.is_dir() => {
                Err(crate::error::ResourceError::conflicting_state(
                    self.description(),
                    "wrapper target to be absent",
                    "directory",
                ))
            }
            Some(_) => {
                crate::fs::remove_file(&self.target)?;
                Ok(ResourceChange::Applied)
            }
            None => Ok(ResourceChange::AlreadyCorrect),
        }
    }
}

impl IntrinsicState for WrapperResource {
    fn current_state(&self) -> Result<ResourceState> {
        let Some(metadata) = self.target_metadata()? else {
            return Ok(ResourceState::Missing);
        };

        if metadata.file_type().is_symlink() {
            let current = match std::fs::metadata(&self.target) {
                Ok(_) => "symlink",
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => "broken symlink",
                Err(error) => {
                    return Err(anyhow::Error::new(error)
                        .context(format!("stat wrapper target {}", self.target.display())));
                }
            };
            return Ok(ResourceState::Incorrect {
                current: current.to_string(),
            });
        }

        let current = crate::fs::read_string(&self.target)?;

        if current == self.content {
            Ok(ResourceState::Correct)
        } else {
            Ok(ResourceState::Incorrect { current })
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
        let r = make_sh_resource(Path::new("/repo"), Path::new("/nonexistent"));
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

        let state = r.current_state().unwrap();
        assert_eq!(state, ResourceState::Correct);
    }

    #[test]
    fn state_incorrect_when_content_differs() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path();
        let r = make_sh_resource(Path::new("/repo"), home);

        std::fs::create_dir_all(r.target.parent().unwrap()).unwrap();
        std::fs::write(&r.target, "old content").unwrap();

        let state = r.current_state().unwrap();
        assert!(matches!(state, ResourceState::Incorrect { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn state_broken_symlink_is_incorrect_not_missing() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path();
        let r = make_sh_resource(Path::new("/repo"), home);

        std::fs::create_dir_all(r.target.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink("/nonexistent/dotfiles-wrapper", &r.target).unwrap();

        let state = r.current_state().unwrap();
        assert!(matches!(
            state,
            ResourceState::Incorrect { ref current } if current == "broken symlink"
        ));
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
    fn apply_overwrites_existing_file() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path();
        let r = make_sh_resource(Path::new("/repo"), home);

        std::fs::create_dir_all(r.target.parent().unwrap()).unwrap();
        std::fs::write(&r.target, "old content").unwrap();

        r.apply().unwrap();
        let actual = std::fs::read_to_string(&r.target).unwrap();
        assert_eq!(actual, r.content);
    }

    #[cfg(unix)]
    #[test]
    fn apply_replaces_broken_symlink_with_wrapper_file() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path();
        let r = make_sh_resource(Path::new("/repo"), home);

        std::fs::create_dir_all(r.target.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink("/nonexistent/dotfiles-wrapper", &r.target).unwrap();

        let result = r.apply().unwrap();

        assert_eq!(result, ResourceChange::Applied);
        assert!(
            !std::fs::symlink_metadata(&r.target)
                .unwrap()
                .file_type()
                .is_symlink(),
            "wrapper target must be a regular file after apply"
        );
        assert_eq!(std::fs::read_to_string(&r.target).unwrap(), r.content);
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
        let r = make_sh_resource(Path::new("/repo"), Path::new("/nonexistent"));
        let result = r.remove().unwrap();
        assert_eq!(result, ResourceChange::AlreadyCorrect);
    }

    #[cfg(unix)]
    #[test]
    fn remove_deletes_broken_symlink() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path();
        let r = make_sh_resource(Path::new("/repo"), home);

        std::fs::create_dir_all(r.target.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink("/nonexistent/dotfiles-wrapper", &r.target).unwrap();

        let result = r.remove().unwrap();

        assert_eq!(result, ResourceChange::Applied);
        assert!(r.target.symlink_metadata().is_err());
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
