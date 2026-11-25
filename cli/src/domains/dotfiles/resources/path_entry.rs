//! `PATH` entry resource.
//!
//! Ensures a directory is on the user's `PATH` by appending to
//! `~/.profile` (Unix) or modifying the user `PATH` via the registry
//! (Windows).
mod profile;

use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::engine::{
    IntrinsicState, RemovableResource, Resource, ResourceChange, ResourceResult, ResourceState,
};
#[cfg(windows)]
use crate::infra::exec::CommandSpec;
use crate::infra::exec::Executor;

/// Persisted user PATH access; independent of the invoking shell's PATH.
#[cfg_attr(test, mockall::automock)]
trait UserPath: std::fmt::Debug + Send + Sync {
    fn contains(&self, dir: &str) -> Result<bool>;
    /// Return whether a new entry was persisted.
    fn append(&self, dir: &str) -> Result<bool>;
}

#[derive(Debug)]
struct RegistryPath {
    #[cfg_attr(
        not(windows),
        allow(dead_code, reason = "registry backend is Windows-only")
    )]
    env: Arc<dyn crate::infra::env::Env>,
}

impl UserPath for RegistryPath {
    fn contains(&self, dir: &str) -> Result<bool> {
        user_path_contains(dir, &*self.env)
    }

    fn append(&self, dir: &str) -> Result<bool> {
        append_user_path(dir, &*self.env)
    }
}

/// Strategy for persisting a `PATH` addition.
#[derive(Debug)]
enum PathStrategy {
    /// Persist the guarded export in a POSIX shell profile.
    ShellProfile {
        /// The profile file to modify (e.g. `~/.profile`).
        path: PathBuf,
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
        store: Arc<dyn UserPath>,
        /// Executor for broadcasting the environment change to other
        /// processes after the registry write.
        #[cfg_attr(not(windows), allow(dead_code, reason = "used conditionally via cfg"))]
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
}

impl PathEntryResource {
    /// Create a new `PATH` entry resource.
    ///
    /// On Unix the resource appends to `~/.profile`; on Windows it modifies
    /// the user `PATH` via the registry.
    #[must_use]
    pub fn new(
        home: &Path,
        platform: crate::infra::platform::Platform,
        executor: Arc<dyn Executor>,
        env: Arc<dyn crate::infra::env::Env>,
    ) -> Self {
        let dir = home.join(".local").join("bin");

        let strategy = if platform.is_windows() {
            PathStrategy::WindowsRegistry {
                dir: dir.to_string_lossy().into_owned(),
                store: Arc::new(RegistryPath { env }),
                executor,
            }
        } else {
            PathStrategy::ShellProfile {
                path: home.join(".profile"),
            }
        };

        Self { dir, strategy }
    }

    #[allow(
        clippy::verbose_file_reads,
        reason = "read through the validated handle instead of reopening an unchecked path"
    )]
    fn shell_profile_contents(path: &Path) -> Result<(PathBuf, Option<String>)> {
        use std::io::Read as _;
        if crate::infra::fs::symlink_metadata_optional(path, "inspect shell profile")?.is_none() {
            return Ok((crate::infra::fs::resolve_target_entry(path)?, None));
        }
        let target = crate::infra::fs::canonicalize(path)?;
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        let mut file = crate::infra::fs::open_in_place(&target, &mut options)?;
        let mut content = String::new();
        file.read_to_string(&mut content)
            .with_context(|| format!("read {}", path.display()))?;
        Ok((target, Some(content)))
    }

    fn publish_shell_profile(
        path: &Path,
        before: &(PathBuf, Option<String>),
        content: String,
    ) -> Result<()> {
        crate::infra::fs::write_atomic_checked(&before.0, content, || {
            anyhow::ensure!(
                Self::shell_profile_contents(path)? == *before,
                "shell profile {} changed during update; retry",
                path.display()
            );
            Ok(())
        })
    }
}

impl Resource for PathEntryResource {
    fn description(&self) -> String {
        format!("PATH \u{2192} {}", self.dir.display())
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        match &self.strategy {
            PathStrategy::ShellProfile { path } => {
                let before = Self::shell_profile_contents(path)?;
                let original = before.1.as_deref().unwrap_or_default();
                let Some(content) = profile::update(original)? else {
                    return Ok(ResourceChange::AlreadyCorrect);
                };
                Self::publish_shell_profile(path, &before, content)?;
            }
            PathStrategy::WindowsRegistry {
                dir,
                store,
                executor,
            } => {
                if !store.append(dir)? {
                    return Ok(ResourceChange::AlreadyCorrect);
                }

                // Best-effort broadcast of the environment change so already-
                // running shells / Explorer pick up the new PATH without a
                // logoff.  SetEnvironmentVariable would do this automatically
                // but also silently rewrites the value type, which is exactly
                // what we are avoiding here — so we broadcast ourselves.
                if let Err(e) = broadcast_environment_change(&**executor) {
                    tracing::debug!("best-effort PATH broadcast failed: {e}");
                }
            }
        }
        Ok(ResourceChange::Applied)
    }
}

impl RemovableResource for PathEntryResource {
    fn remove(&self) -> ResourceResult<ResourceChange> {
        // Leaving the directory on PATH is harmless; removing it from
        // profile files or registry is fragile and surprising.
        Ok(ResourceChange::AlreadyCorrect)
    }
}

impl IntrinsicState for PathEntryResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        match &self.strategy {
            PathStrategy::ShellProfile { path } => {
                let (_, content) = Self::shell_profile_contents(path)?;
                if profile::update(content.as_deref().unwrap_or_default())?.is_some() {
                    Ok(ResourceState::Missing)
                } else {
                    Ok(ResourceState::Correct)
                }
            }
            PathStrategy::WindowsRegistry { dir, store, .. } => match store.contains(dir) {
                Ok(true) => Ok(ResourceState::Correct),
                Ok(false) => Ok(ResourceState::Missing),
                Err(error) => Ok(ResourceState::Unknown {
                    reason: error.to_string(),
                }),
            },
        }
    }
}

/// Append `dir` to the Windows user `PATH` in the registry, preserving the
/// original value type (`REG_EXPAND_SZ` by default). Reject additions that
/// cannot survive expansion and PATH splitting without changing their meaning.
#[cfg(windows)]
fn append_user_path(dir: &str, environment: &dyn crate::infra::env::Env) -> Result<bool> {
    use winreg::RegKey;
    use winreg::RegValue;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_EXPAND_SZ};

    let env = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)
        .context("opening HKCU\\Environment")?;

    let (existing_value, existing_vtype) = match env.get_raw_value("Path") {
        Ok(v) => {
            let s = decode_reg_string(&v)?;
            let vtype = v.vtype;
            (s, vtype)
        }
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => (String::new(), REG_EXPAND_SZ),
        Err(e) => {
            return Err(anyhow::Error::from(e).context("reading HKCU\\Environment\\Path"));
        }
    };

    let Some(new_value) = appended_path(
        &existing_value,
        dir,
        existing_vtype == REG_EXPAND_SZ,
        environment,
    )?
    else {
        return Ok(false);
    };

    let new_raw = RegValue {
        bytes: encode_reg_string(&new_value).into(),
        vtype: existing_vtype,
    };
    env.set_raw_value("Path", &new_raw)
        .context("writing HKCU\\Environment\\Path")?;
    Ok(true)
}

/// Native registry access is unavailable on non-Windows hosts.
#[cfg(not(windows))]
fn append_user_path(_dir: &str, _environment: &dyn crate::infra::env::Env) -> Result<bool> {
    anyhow::bail!("user PATH registry access is only supported on Windows")
}

#[cfg(not(windows))]
fn user_path_contains(_dir: &str, _environment: &dyn crate::infra::env::Env) -> Result<bool> {
    anyhow::bail!("user PATH registry access is only supported on Windows")
}

/// Return `true` when the persisted user `PATH` in the registry already
/// contains `dir`.
#[cfg(windows)]
fn user_path_contains(dir: &str, environment: &dyn crate::infra::env::Env) -> Result<bool> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, REG_EXPAND_SZ};

    let env =
        match RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags("Environment", KEY_READ) {
            Ok(k) => k,
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => {
                return Err(anyhow::Error::from(e).context("opening HKCU\\Environment"));
            }
        };
    match env.get_raw_value("Path") {
        Ok(v) => Ok(path_contains_entry(
            &decode_reg_string(&v)?,
            dir,
            v.vtype == REG_EXPAND_SZ,
            environment,
        )),
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(anyhow::Error::from(e).context("reading HKCU\\Environment\\Path")),
    }
}

/// Broadcast `WM_SETTINGCHANGE` for the `"Environment"` setting so Explorer
/// and other top-level windows re-read their environment block.
#[cfg(windows)]
fn broadcast_environment_change(executor: &dyn Executor) -> Result<()> {
    const BROADCAST_SCRIPT: &str = "Add-Type -Namespace Win32 -Name NativeMethods -MemberDefinition '[System.Runtime.InteropServices.DllImport(\"user32.dll\", SetLastError=true, CharSet=System.Runtime.InteropServices.CharSet.Auto)] public static extern System.IntPtr SendMessageTimeout(System.IntPtr hWnd, uint Msg, System.UIntPtr wParam, string lParam, uint fuFlags, uint uTimeout, out System.UIntPtr lpdwResult);'; $r = [System.UIntPtr]::Zero; [void][Win32.NativeMethods]::SendMessageTimeout([System.IntPtr]0xffff, 0x001A, [System.UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$r)";
    let shell = if executor.which("pwsh") {
        "pwsh"
    } else {
        "powershell"
    };
    executor.execute(CommandSpec::new(shell).args(&[
        "-NoProfile",
        "-Command",
        BROADCAST_SCRIPT,
    ]))?;
    Ok(())
}

/// Non-Windows environment broadcast fallback.
#[cfg(not(windows))]
#[allow(
    clippy::unnecessary_wraps,
    clippy::missing_const_for_fn,
    reason = "matches Windows backend signature"
)]
fn broadcast_environment_change(_executor: &dyn Executor) -> Result<()> {
    Ok(())
}

#[cfg(any(windows, test))]
fn appended_path(
    existing: &str,
    dir: &str,
    expandable: bool,
    environment: &dyn crate::infra::env::Env,
) -> Result<Option<String>> {
    if path_contains_entry(existing, dir, expandable, environment) {
        return Ok(None);
    }
    let updated = if existing.is_empty() {
        dir.to_string()
    } else {
        format!("{existing};{dir}")
    };
    anyhow::ensure!(
        path_contains_entry(&updated, dir, expandable, environment),
        "cannot persist literal directory in Windows PATH without changing existing entries: {dir}"
    );
    Ok(Some(updated))
}

/// Return `true` when `dir` already appears as a `;`-separated entry in
/// `path` (case-insensitive on Windows). Only `REG_EXPAND_SZ` values expand
/// environment variables, before splitting the resulting PATH into entries.
/// The directory being added is a literal filesystem path.
#[cfg(any(windows, test))]
fn path_contains_entry(
    path: &str,
    dir: &str,
    expandable: bool,
    environment: &dyn crate::infra::env::Env,
) -> bool {
    let path = if expandable {
        expand_env_vars(path, environment)
    } else {
        path.to_string()
    };
    let target = normalize_path_entry(dir);
    path.split(';')
        .map(normalize_path_entry)
        .any(|entry| entry.eq_ignore_ascii_case(&target))
}

/// Expand `%VAR%` tokens in `value` against the current process environment.
#[cfg(any(windows, test))]
fn expand_env_vars(value: &str, environment: &dyn crate::infra::env::Env) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(end) = after.find('%') {
            let name = &after[..end];
            if !name.is_empty()
                && let Some(val) = environment.var_os(name)
            {
                out.push_str(&val.to_string_lossy());
                rest = &after[end + 1..];
                continue;
            }
            out.push('%');
            out.push_str(name);
            out.push('%');
            rest = &after[end + 1..];
        } else {
            out.push('%');
            out.push_str(after);
            rest = "";
        }
    }
    out.push_str(rest);
    out
}

/// Normalize a path entry for comparison.
#[cfg(any(windows, test))]
fn normalize_path_entry(entry: &str) -> String {
    let trimmed = entry.trim();
    let unified: String = trimmed
        .chars()
        .map(|c| if c == '/' { '\\' } else { c })
        .collect();
    unified.trim_end_matches('\\').to_string()
}

/// Decode a registry string without silently corrupting unsupported or malformed data.
#[cfg(windows)]
fn decode_reg_string(value: &winreg::RegValue) -> Result<String> {
    use winreg::enums::{REG_EXPAND_SZ, REG_SZ};

    anyhow::ensure!(
        matches!(value.vtype, REG_SZ | REG_EXPAND_SZ),
        "HKCU\\Environment\\Path has unsupported registry type {:?}",
        value.vtype
    );
    let (pairs, remainder) = value.bytes.as_chunks::<2>();
    anyhow::ensure!(
        remainder.is_empty(),
        "HKCU\\Environment\\Path has an incomplete UTF-16 code unit"
    );
    let wide: Vec<u16> = pairs.iter().copied().map(u16::from_le_bytes).collect();
    let decoded = String::from_utf16(&wide).context("decoding HKCU\\Environment\\Path")?;
    let decoded = decoded.trim_end_matches('\0');
    anyhow::ensure!(
        !decoded.contains('\0'),
        "HKCU\\Environment\\Path contains an embedded NUL"
    );
    Ok(decoded.to_string())
}

/// Encode `value` as a NUL-terminated UTF-16LE byte buffer.
#[cfg(windows)]
fn encode_reg_string(value: &str) -> Vec<u8> {
    let mut out: Vec<u8> = value.encode_utf16().flat_map(u16::to_le_bytes).collect();
    out.extend_from_slice(&[0, 0]);
    out
}

#[cfg(test)]
mod tests;
