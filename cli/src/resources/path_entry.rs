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
    ///
    /// Writes directly via the `winreg` crate to preserve the original
    /// value type (`REG_EXPAND_SZ` vs `REG_SZ`).  Using
    /// `[Environment]::SetEnvironmentVariable` would silently coerce the
    /// user's `PATH` to `REG_SZ`, baking the current expansion of tokens
    /// like `%USERPROFILE%\bin` into permanent literals — a data-destroying
    /// transformation with no in-band recovery.
    WindowsRegistry {
        /// Directory string to add to `PATH`.
        dir: String,
        /// Executor for broadcasting the environment change to other
        /// processes after the registry write.
        #[cfg_attr(not(windows), allow(dead_code))]
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
                dir: dir.to_string_lossy().into_owned(),
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
                append_user_path_windows(dir)?;
                // Best-effort broadcast of the environment change so already-
                // running shells / Explorer pick up the new PATH without a
                // logoff.  SetEnvironmentVariable would do this automatically
                // but also silently rewrites the value type, which is exactly
                // what we are avoiding here — so we broadcast ourselves.
                let _ = broadcast_environment_change(&**executor);
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

        // On Windows, also check the persisted user PATH in the registry —
        // it may already contain the directory even if the current process's
        // environment block does not.
        #[cfg(windows)]
        if let PathStrategy::WindowsRegistry { ref dir, .. } = self.strategy
            && user_path_contains(dir).unwrap_or(false)
        {
            return Ok(ResourceState::Correct);
        }

        Ok(ResourceState::Missing)
    }
}

/// Append `dir` to the Windows user `PATH` in the registry, preserving the
/// original value type (`REG_EXPAND_SZ` by default).
///
/// This is a no-op on non-Windows targets.
#[cfg(windows)]
fn append_user_path_windows(dir: &str) -> Result<()> {
    use winreg::RegKey;
    use winreg::RegValue;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_EXPAND_SZ};

    let env = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)
        .context("opening HKCU\\Environment")?;

    let (existing_value, existing_vtype) = match env.get_raw_value("Path") {
        Ok(v) => {
            let s = decode_reg_string(&v.bytes);
            let vtype = v.vtype;
            (s, vtype)
        }
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => (String::new(), REG_EXPAND_SZ),
        Err(e) => {
            return Err(anyhow::Error::from(e).context("reading HKCU\\Environment\\Path"));
        }
    };

    if path_contains_entry(&existing_value, dir) {
        return Ok(());
    }

    let new_value = if existing_value.is_empty() {
        dir.to_string()
    } else {
        format!("{existing_value};{dir}")
    };

    let new_raw = RegValue {
        bytes: encode_reg_string(&new_value),
        vtype: existing_vtype,
    };
    env.set_raw_value("Path", &new_raw)
        .context("writing HKCU\\Environment\\Path")?;
    Ok(())
}

#[cfg(not(windows))]
#[allow(clippy::unnecessary_wraps, clippy::missing_const_for_fn)]
fn append_user_path_windows(_dir: &str) -> Result<()> {
    Ok(())
}

/// Return `true` when `dir` already appears as a `;`-separated entry in
/// `path` (case-insensitive on Windows).
#[cfg(windows)]
fn path_contains_entry(path: &str, dir: &str) -> bool {
    path.split(';').any(|entry| entry.eq_ignore_ascii_case(dir))
}

/// Return `true` when the persisted user `PATH` in the registry already
/// contains `dir`.
#[cfg(windows)]
fn user_path_contains(dir: &str) -> Result<bool> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};

    let env =
        match RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags("Environment", KEY_READ) {
            Ok(k) => k,
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => {
                return Err(anyhow::Error::from(e).context("opening HKCU\\Environment"));
            }
        };
    match env.get_raw_value("Path") {
        Ok(v) => Ok(path_contains_entry(&decode_reg_string(&v.bytes), dir)),
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(anyhow::Error::from(e).context("reading HKCU\\Environment\\Path")),
    }
}

/// Decode a UTF-16LE registry string payload, stripping any trailing NUL.
#[cfg(windows)]
fn decode_reg_string(bytes: &[u8]) -> String {
    let wide: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| {
            // chunks_exact guarantees len == 2.
            let lo = *c.first().unwrap_or(&0);
            let hi = *c.get(1).unwrap_or(&0);
            u16::from_le_bytes([lo, hi])
        })
        .collect();
    String::from_utf16_lossy(&wide)
        .trim_end_matches('\0')
        .to_string()
}

/// Encode `value` as a NUL-terminated UTF-16LE byte buffer suitable for a
/// `REG_SZ` / `REG_EXPAND_SZ` registry value.
#[cfg(windows)]
fn encode_reg_string(value: &str) -> Vec<u8> {
    let mut out: Vec<u8> = value.encode_utf16().flat_map(u16::to_le_bytes).collect();
    out.extend_from_slice(&[0, 0]);
    out
}

/// Broadcast `WM_SETTINGCHANGE` for the `"Environment"` setting so that
/// Explorer and other top-level windows re-read their environment block.
///
/// Performed by a tiny `PowerShell` helper so we avoid an `unsafe` FFI
/// dependency in this crate.  Any failure is ignored — broadcasting is a
/// best-effort convenience, not a correctness requirement.
#[cfg(windows)]
fn broadcast_environment_change(executor: &dyn Executor) -> Result<()> {
    const BROADCAST_SCRIPT: &str = "Add-Type -Namespace Win32 -Name NativeMethods -MemberDefinition '[System.Runtime.InteropServices.DllImport(\"user32.dll\", SetLastError=true, CharSet=System.Runtime.InteropServices.CharSet.Auto)] public static extern System.IntPtr SendMessageTimeout(System.IntPtr hWnd, uint Msg, System.UIntPtr wParam, string lParam, uint fuFlags, uint uTimeout, out System.UIntPtr lpdwResult);'; $r = [System.UIntPtr]::Zero; [void][Win32.NativeMethods]::SendMessageTimeout([System.IntPtr]0xffff, 0x001A, [System.UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$r)";
    let shell = if executor.which("pwsh") {
        "pwsh"
    } else {
        "powershell"
    };
    executor.run(shell, &["-NoProfile", "-Command", BROADCAST_SCRIPT])?;
    Ok(())
}

#[cfg(not(windows))]
#[allow(clippy::unnecessary_wraps, clippy::missing_const_for_fn)]
fn broadcast_environment_change(_executor: &dyn Executor) -> Result<()> {
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn make_path_entry(home: &Path, on_path: bool) -> PathEntryResource {
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
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
