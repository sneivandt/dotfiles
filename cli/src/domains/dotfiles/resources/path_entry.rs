//! `PATH` entry resource.
//!
//! Ensures a directory is on the user's `PATH` by appending to
//! `~/.profile` (Unix) or modifying the user `PATH` via the registry
//! (Windows).
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
                line: "export PATH=\"$HOME/.local/bin:$PATH\"".to_string(),
            }
        };

        Self { dir, strategy }
    }

    fn shell_profile_contains_managed_line(path: &Path, line: &str) -> Result<bool> {
        if !path.exists() {
            return Ok(false);
        }

        Ok(std::fs::read_to_string(path)
            .with_context(|| format!("read {}", path.display()))?
            .lines()
            .any(|existing| existing.trim() == line))
    }
}

impl Resource for PathEntryResource {
    fn description(&self) -> String {
        format!("PATH \u{2192} {}", self.dir.display())
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        use std::io::Write;

        match &self.strategy {
            PathStrategy::ShellProfile { path, line } => {
                if Self::shell_profile_contains_managed_line(path, line)? {
                    return Ok(ResourceChange::AlreadyCorrect);
                }

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
                Ok(ResourceChange::Applied)
            }
        }
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
            PathStrategy::ShellProfile { path, line } => {
                if Self::shell_profile_contains_managed_line(path, line)? {
                    Ok(ResourceState::Correct)
                } else {
                    Ok(ResourceState::Missing)
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
/// original value type (`REG_EXPAND_SZ` by default).
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
            let s = decode_reg_string(&v.bytes);
            let vtype = v.vtype;
            (s, vtype)
        }
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => (String::new(), REG_EXPAND_SZ),
        Err(e) => {
            return Err(anyhow::Error::from(e).context("reading HKCU\\Environment\\Path"));
        }
    };

    if path_contains_entry(&existing_value, dir, environment) {
        return Ok(false);
    }

    let new_value = if existing_value.is_empty() {
        dir.to_string()
    } else {
        format!("{existing_value};{dir}")
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
        Ok(v) => Ok(path_contains_entry(
            &decode_reg_string(&v.bytes),
            dir,
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

/// Return `true` when `dir` already appears as a `;`-separated entry in
/// `path` (case-insensitive on Windows).
#[cfg(any(windows, test))]
fn path_contains_entry(path: &str, dir: &str, environment: &dyn crate::infra::env::Env) -> bool {
    let target = normalize_path_entry(&expand_env_vars(dir, environment));
    path.split(';')
        .map(|entry| normalize_path_entry(&expand_env_vars(entry, environment)))
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

/// Decode a UTF-16LE registry string payload, stripping any trailing NUL.
#[cfg(windows)]
fn decode_reg_string(bytes: &[u8]) -> String {
    let wide: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .copied()
        .map(u16::from_le_bytes)
        .collect();
    String::from_utf16_lossy(&wide)
        .trim_end_matches('\0')
        .to_string()
}

/// Encode `value` as a NUL-terminated UTF-16LE byte buffer.
#[cfg(windows)]
fn encode_reg_string(value: &str) -> Vec<u8> {
    let mut out: Vec<u8> = value.encode_utf16().flat_map(u16::to_le_bytes).collect();
    out.extend_from_slice(&[0, 0]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::env::MapEnv;
    use crate::infra::exec::MockExecutor;
    use crate::infra::platform::{Os, Platform};

    fn windows_resource(home: &Path, store: MockUserPath) -> PathEntryResource {
        let executor = MockExecutor::new();
        #[cfg(windows)]
        let executor = {
            let mut executor = executor;
            executor
                .expect_which()
                .with(mockall::predicate::eq("pwsh"))
                .returning(|_| true);
            executor
                .expect_execute()
                .once()
                .withf(|spec| spec.program() == "pwsh")
                .returning(|_| Ok(crate::infra::exec::ExecResult::success("")));
            executor
        };
        let env = MapEnv::new()
            .with("PATH", home.join(".local/bin").into_os_string())
            .into_handle();
        let mut resource = PathEntryResource::new(
            home,
            Platform::new(Os::Windows, false),
            Arc::new(executor),
            env,
        );
        if let PathStrategy::WindowsRegistry { store: backend, .. } = &mut resource.strategy {
            *backend = Arc::new(store);
        }
        resource
    }

    #[test]
    fn transient_runtime_path_does_not_skip_persistence_and_repeated_apply_is_current() {
        let home = tempfile::tempdir().unwrap();
        let mut store = MockUserPath::new();
        let mut sequence = mockall::Sequence::new();
        store
            .expect_contains()
            .once()
            .in_sequence(&mut sequence)
            .returning(|_| Ok(false));
        store
            .expect_append()
            .once()
            .in_sequence(&mut sequence)
            .returning(|_| Ok(true));
        store
            .expect_contains()
            .once()
            .in_sequence(&mut sequence)
            .returning(|_| Ok(true));
        store
            .expect_append()
            .once()
            .in_sequence(&mut sequence)
            .returning(|_| Ok(false));
        let resource = windows_resource(home.path(), store);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
        assert_eq!(resource.apply().unwrap(), ResourceChange::AlreadyCorrect);
    }

    #[test]
    fn persisted_path_read_and_write_errors_cannot_report_success() {
        let home = tempfile::tempdir().unwrap();
        let mut store = MockUserPath::new();
        store
            .expect_contains()
            .once()
            .returning(|_| anyhow::bail!("fixture registry read failure"));
        store
            .expect_append()
            .once()
            .returning(|_| anyhow::bail!("fixture registry write failure"));
        let resource = PathEntryResource {
            dir: home.path().join(".local/bin"),
            strategy: PathStrategy::WindowsRegistry {
                dir: "fixture".into(),
                store: Arc::new(store),
                executor: Arc::new(MockExecutor::new()),
            },
        };
        assert_eq!(
            resource.current_state().unwrap(),
            ResourceState::Unknown {
                reason: "fixture registry read failure".into()
            }
        );
        assert!(resource.apply().is_err());
    }

    #[test]
    fn registry_preview_only_reads_persisted_state() {
        let home = tempfile::tempdir().unwrap();
        let mut store = MockUserPath::new();
        store.expect_contains().once().returning(|_| Ok(false));
        let resource = PathEntryResource {
            dir: home.path().join(".local/bin"),
            strategy: PathStrategy::WindowsRegistry {
                dir: "fixture".into(),
                store: Arc::new(store),
                executor: Arc::new(MockExecutor::new()),
            },
        };
        let ctx =
            crate::test_helpers::make_linux_context(home.path().into(), None).with_dry_run(true);
        let result = crate::engine::process_resources(
            &ctx,
            [resource],
            &crate::engine::ProcessOpts::strict("configure"),
        )
        .unwrap();
        assert_eq!(crate::test_helpers::task_batch(&result).changed_count(), 1);
        assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
    }
    #[test]
    fn persisted_entries_match_expanded_windows_paths_without_runtime_path() {
        let env = MapEnv::new().with("USERPROFILE", r"C:\Users\Fixture User");
        for path in [
            r"%USERPROFILE%/.local/bin",
            r"c:\users\fixture user\.local\bin\",
        ] {
            assert!(
                path_contains_entry(path, r"C:\Users\Fixture User\.local\bin", &env),
                "{path}"
            );
        }
        assert!(!path_contains_entry(
            r"C:\Other\bin",
            r"C:\Users\Fixture User\.local\bin",
            &env
        ));
        assert_eq!(expand_env_vars(r"%UNKNOWN%\bin", &env), r"%UNKNOWN%\bin");
    }
}
