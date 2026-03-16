//! CLI wrapper installation resource.
//!
//! Installs a small script on the user's `PATH` (`~/.local/bin/dotfiles` on
//! both Unix and Windows) that delegates to the repository's wrapper script
//! so `dotfiles` can be invoked from anywhere.
use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};

use super::{Applicable, Resource, ResourceChange, ResourceState};

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
    pub fn detect(platform: &crate::platform::Platform) -> Self {
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
    #[must_use]
    pub fn new(wrapper_type: WrapperType, dotfiles_root: &Path, home: &Path) -> Self {
        let bin_dir = home.join(".local").join("bin");

        let (target, content) = match wrapper_type {
            WrapperType::Sh => {
                let target = bin_dir.join("dotfiles");
                let content = format!(
                    "#!/bin/sh\n\
                     # Installed by dotfiles \u{2014} do not edit.\n\
                     DOTFILES_ROOT=\"${{DOTFILES_ROOT:-{root}}}\"\n\
                     export DOTFILES_ROOT\n\
                     exec \"$DOTFILES_ROOT/dotfiles.sh\" \"$@\"\n",
                    root = dotfiles_root.display()
                );
                (target, content)
            }
            WrapperType::Pwsh => {
                let target = bin_dir.join("dotfiles.ps1");
                let content = format!(
                    "# Installed by dotfiles \u{2014} do not edit.\n\
                     if (-not $env:DOTFILES_ROOT) {{ $env:DOTFILES_ROOT = '{root}' }}\n\
                     Set-ExecutionPolicy Bypass -Scope Process -Force\n\
                     & \"$env:DOTFILES_ROOT{sep}dotfiles.ps1\" @args\n\
                     exit $LASTEXITCODE\n",
                    root = dotfiles_root.display(),
                    sep = std::path::MAIN_SEPARATOR,
                );
                (target, content)
            }
        };

        Self { target, content }
    }
}

impl Applicable for WrapperResource {
    fn description(&self) -> String {
        format!("wrapper \u{2192} {}", self.target.display())
    }

    fn apply(&self) -> Result<ResourceChange> {
        crate::fs::ensure_parent_dir(&self.target)?;

        std::fs::write(&self.target, &self.content)
            .with_context(|| format!("write wrapper to {}", self.target.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.target, std::fs::Permissions::from_mode(0o755))
                .with_context(|| format!("chmod 755 {}", self.target.display()))?;
        }

        Ok(ResourceChange::Applied)
    }

    fn remove(&self) -> Result<ResourceChange> {
        if self.target.exists() {
            std::fs::remove_file(&self.target)
                .with_context(|| format!("remove wrapper {}", self.target.display()))?;
            Ok(ResourceChange::Applied)
        } else {
            Ok(ResourceChange::AlreadyCorrect)
        }
    }
}

impl Resource for WrapperResource {
    fn current_state(&self) -> Result<ResourceState> {
        if !self.target.exists() {
            return Ok(ResourceState::Missing);
        }

        let current = std::fs::read_to_string(&self.target)
            .with_context(|| format!("read wrapper at {}", self.target.display()))?;

        if current == self.content {
            Ok(ResourceState::Correct)
        } else {
            Ok(ResourceState::Incorrect { current })
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
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
        assert!(
            r.content
                .contains("DOTFILES_ROOT=\"${DOTFILES_ROOT:-/repo}\"")
        );
        assert!(
            r.content
                .contains("exec \"$DOTFILES_ROOT/dotfiles.sh\" \"$@\"")
        );
    }

    #[test]
    fn pwsh_content_uses_dotfiles_root_env() {
        let r = make_pwsh_resource(Path::new("/repo"), Path::new("/home/user"));
        assert!(r.content.contains("$env:DOTFILES_ROOT = '/repo'"));
        assert!(
            r.content
                .contains("Set-ExecutionPolicy Bypass -Scope Process -Force")
        );
        assert!(r.content.contains("dotfiles.ps1\" @args"));
        assert!(r.content.contains("exit $LASTEXITCODE"));
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

    // ── Target paths ─────────────────────────────────────────────────

    #[test]
    fn sh_target_is_local_bin_dotfiles() {
        let r = make_sh_resource(Path::new("/repo"), Path::new("/home/user"));
        assert_eq!(r.target, PathBuf::from("/home/user/.local/bin/dotfiles"));
    }

    #[test]
    fn pwsh_target_is_local_bin_dotfiles_ps1() {
        let r = make_pwsh_resource(Path::new("/repo"), Path::new("/home/user"));
        assert_eq!(
            r.target,
            PathBuf::from("/home/user/.local/bin/dotfiles.ps1")
        );
    }
}
