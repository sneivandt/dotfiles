//! `PATH` entry resource.
//!
//! Ensures a directory is on the user's `PATH` by appending to
//! `~/.profile` (Unix) or modifying the user `PATH` via the registry
//! (Windows).
use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{Applicable, Resource, ResourceChange, ResourceState};
use crate::exec::Executor;

/// Source for checking whether a directory is already on `PATH`.
///
/// Production code reads the real `PATH` variable; tests inject a fixed
/// value to avoid depending on the host environment.
#[derive(Debug, Clone)]
enum PathSource {
    /// Read from the `PATH` environment variable at check time.
    Environment,
    /// Use a fixed result (for testing).
    #[cfg(test)]
    Fixed(bool),
}

impl PathSource {
    fn is_on_path(&self, dir: &Path) -> bool {
        match self {
            Self::Environment => std::env::var_os("PATH")
                .is_some_and(|p| std::env::split_paths(&p).any(|entry| entry == dir)),
            #[cfg(test)]
            Self::Fixed(result) => *result,
        }
    }
}

/// Strategy for persisting a `PATH` addition.
#[derive(Debug)]
enum PathStrategy {
    /// Append an `export` line to a POSIX shell profile file.
    ShellProfile {
        /// The profile file to modify (e.g. `~/.profile`).
        path: PathBuf,
        /// The export line to append.
        line: String,
    },
    /// Modify the Windows user `PATH` via the registry.
    WindowsRegistry {
        /// Directory string to add to `PATH`.
        dir: String,
        /// Executor for running `PowerShell`.
        executor: Arc<dyn Executor>,
    },
}

/// A resource that ensures a directory is on the user's `PATH`.
#[derive(Debug)]
pub struct PathEntryResource {
    /// The directory that should be on `PATH`.
    dir: PathBuf,
    /// How to persist the `PATH` change.
    strategy: PathStrategy,
    /// Source for the runtime `PATH` check.
    path_source: PathSource,
}

impl PathEntryResource {
    /// Create a new `PATH` entry resource.
    ///
    /// On Unix the resource appends to `~/.profile`; on Windows it modifies
    /// the user `PATH` via the registry.
    #[must_use]
    pub fn new(
        home: &Path,
        platform: &crate::platform::Platform,
        executor: Arc<dyn Executor>,
    ) -> Self {
        let dir = home.join(".local").join("bin");

        let strategy = if platform.is_windows() {
            PathStrategy::WindowsRegistry {
                dir: dir.to_str().unwrap_or_default().to_string(),
                executor,
            }
        } else {
            PathStrategy::ShellProfile {
                path: home.join(".profile"),
                line: "export PATH=\"$HOME/.local/bin:$PATH\"".to_string(),
            }
        };

        Self {
            dir,
            strategy,
            path_source: PathSource::Environment,
        }
    }

    /// Override the `PATH` source with a fixed value (for testing).
    #[cfg(test)]
    #[must_use]
    const fn with_path_source(mut self, on_path: bool) -> Self {
        self.path_source = PathSource::Fixed(on_path);
        self
    }
}

impl Applicable for PathEntryResource {
    fn description(&self) -> String {
        format!("PATH \u{2192} {}", self.dir.display())
    }

    fn apply(&self) -> Result<ResourceChange> {
        if self.path_source.is_on_path(&self.dir) {
            return Ok(ResourceChange::AlreadyCorrect);
        }

        match &self.strategy {
            PathStrategy::ShellProfile { path, line } => {
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .with_context(|| format!("open {}", path.display()))?;
                writeln!(file)?;
                writeln!(file, "# Added by dotfiles")?;
                writeln!(file, "{line}")?;
                Ok(ResourceChange::Applied)
            }
            PathStrategy::WindowsRegistry { dir, executor } => {
                let script = format!(
                    "$d='{dir}';$p=[Environment]::GetEnvironmentVariable('Path','User');\
                     if(-not($p -and ($p -split ';' -contains $d)))\
                     {{[Environment]::SetEnvironmentVariable('Path',\
                     $(if($p){{\"$p;$d\"}}else{{$d}}),'User')}}",
                    dir = dir.replace('\'', "''"),
                );
                executor.run("powershell", &["-NoProfile", "-Command", &script])?;
                Ok(ResourceChange::Applied)
            }
        }
    }

    fn remove(&self) -> Result<ResourceChange> {
        // Leaving the directory on PATH is harmless; removing it from
        // profile files or registry is fragile and surprising.
        Ok(ResourceChange::AlreadyCorrect)
    }
}

impl Resource for PathEntryResource {
    fn current_state(&self) -> Result<ResourceState> {
        // Fast path: directory is already on the runtime PATH.
        if self.path_source.is_on_path(&self.dir) {
            return Ok(ResourceState::Correct);
        }

        // On Unix, also check whether the export line was already written
        // to the profile (it may not have been sourced yet in this session).
        if let PathStrategy::ShellProfile { ref path, ref line } = self.strategy
            && path.exists()
            && std::fs::read_to_string(path)
                .with_context(|| format!("read {}", path.display()))?
                .contains(line.as_str())
        {
            return Ok(ResourceState::Correct);
        }

        Ok(ResourceState::Missing)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn make_path_entry(home: &Path, on_path: bool) -> PathEntryResource {
        let executor: Arc<dyn crate::exec::Executor> = Arc::new(crate::exec::SystemExecutor);
        let platform = crate::platform::Platform {
            os: crate::platform::Os::Linux,
            is_arch: false,
            is_wsl: false,
        };
        PathEntryResource::new(home, &platform, executor).with_path_source(on_path)
    }

    #[test]
    fn description_includes_dir() {
        let r = make_path_entry(Path::new("/home/user"), false);
        let expected = Path::new(".local").join("bin");
        assert!(
            r.description()
                .contains(expected.to_string_lossy().as_ref()),
            "got: {}",
            r.description()
        );
    }

    #[test]
    fn correct_when_already_on_path() {
        let r = make_path_entry(Path::new("/home/user"), true);
        let state = r.current_state().unwrap();
        assert_eq!(state, ResourceState::Correct);
    }

    #[test]
    fn missing_when_not_on_path_and_no_profile() {
        let tmp = TempDir::new().unwrap();
        let r = make_path_entry(tmp.path(), false);
        let state = r.current_state().unwrap();
        assert_eq!(state, ResourceState::Missing);
    }

    #[test]
    fn correct_when_export_line_in_profile() {
        let tmp = TempDir::new().unwrap();
        let profile = tmp.path().join(".profile");
        std::fs::write(
            &profile,
            "# existing\nexport PATH=\"$HOME/.local/bin:$PATH\"\n",
        )
        .unwrap();

        let r = make_path_entry(tmp.path(), false);
        let state = r.current_state().unwrap();
        assert_eq!(state, ResourceState::Correct);
    }

    #[test]
    fn apply_appends_to_profile() {
        let tmp = TempDir::new().unwrap();
        let profile = tmp.path().join(".profile");
        std::fs::write(&profile, "# existing config\n").unwrap();

        let r = make_path_entry(tmp.path(), false);
        let result = r.apply().unwrap();
        assert_eq!(result, ResourceChange::Applied);

        let content = std::fs::read_to_string(&profile).unwrap();
        assert!(
            content.contains("# Added by dotfiles"),
            "missing marker in: {content}"
        );
        assert!(
            content.contains("export PATH=\"$HOME/.local/bin:$PATH\""),
            "missing export in: {content}"
        );
        assert!(content.starts_with("# existing config\n"));
    }

    #[test]
    fn apply_creates_profile_if_missing() {
        let tmp = TempDir::new().unwrap();
        let profile = tmp.path().join(".profile");
        assert!(!profile.exists());

        let r = make_path_entry(tmp.path(), false);
        r.apply().unwrap();

        assert!(profile.exists());
        let content = std::fs::read_to_string(&profile).unwrap();
        assert!(content.contains("export PATH=\"$HOME/.local/bin:$PATH\""));
    }

    #[test]
    fn apply_skips_when_already_on_path() {
        let tmp = TempDir::new().unwrap();
        let r = make_path_entry(tmp.path(), true);
        let result = r.apply().unwrap();
        assert_eq!(result, ResourceChange::AlreadyCorrect);
    }

    #[test]
    fn remove_is_noop() {
        let r = make_path_entry(Path::new("/home/user"), false);
        let result = r.remove().unwrap();
        assert_eq!(result, ResourceChange::AlreadyCorrect);
    }
}
