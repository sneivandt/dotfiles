//! File permission resource (chmod).
#[cfg(unix)]
use anyhow::Context as _;
use anyhow::Result;
use std::path::PathBuf;

use crate::domains::files::config::chmod::OctalMode;
#[cfg(unix)]
use crate::engine::resource::ResourceError;
use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};

/// Unix file permission mask (all permission bits).
#[cfg(unix)]
pub(super) const MODE_BITS_MASK: u32 = 0o7777;

/// A file permission resource that can be checked and applied (Unix only).
#[derive(Debug, Clone)]
pub struct ChmodResource {
    /// Target file path (absolute).
    pub target: PathBuf,
    pub(super) mode: Result<OctalMode, String>,
}

impl ChmodResource {
    /// Create a new chmod resource.
    #[must_use]
    #[cfg(test)]
    pub const fn new(target: PathBuf, mode: OctalMode) -> Self {
        Self {
            target,
            mode: Ok(mode),
        }
    }

    /// Create from a config entry and home directory.
    ///
    pub fn from_entry(
        entry: &crate::domains::files::config::chmod::ChmodEntry,
        home: &std::path::Path,
    ) -> Self {
        let relative_path = entry.path.strip_prefix('.').unwrap_or(&entry.path);
        let target = home.join(format!(".{relative_path}"));
        Self {
            target,
            mode: entry.parsed_mode().clone(),
        }
    }
}

impl Resource for ChmodResource {
    fn description(&self) -> String {
        match &self.mode {
            Ok(mode) => format!("{mode} {}", self.target.display()),
            Err(reason) => format!(
                "invalid chmod mode ({reason}) for {}",
                self.target.display()
            ),
        }
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        #[cfg(unix)]
        {
            let mode = self.mode.as_ref().map_err(|reason| {
                ResourceError::conflicting_state(
                    self.target.display().to_string(),
                    "valid octal mode",
                    reason,
                )
            })?;
            let mode = mode.as_u32();

            if self.target.is_dir() {
                apply_recursive(
                    &self.target,
                    ensure_dir_execute_bits(mode),
                    strip_file_execute_bits(mode),
                )?;
            } else {
                set_permissions(&self.target, mode)?;
            }

            Ok(ResourceChange::Applied)
        }

        #[cfg(not(unix))]
        {
            Ok(ResourceChange::Skipped {
                reason: "chmod not supported on this platform".to_string(),
            })
        }
    }
}

impl IntrinsicState for ChmodResource {
    fn current_state(&self) -> Result<ResourceState> {
        if let Err(reason) = &self.mode {
            return Ok(ResourceState::Invalid {
                reason: reason.clone(),
            });
        }
        if !self.target.exists() {
            return Ok(ResourceState::Missing);
        }

        // Get current mode (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = self
                .mode
                .as_ref()
                .map_err(|reason| anyhow::anyhow!("{reason}"))?;
            let desired_mode = mode.as_u32();
            if self.target.is_dir() {
                check_dir_recursive(&self.target, desired_mode)
            } else {
                let current_mode =
                    std::fs::metadata(&self.target)?.permissions().mode() & MODE_BITS_MASK;
                if current_mode == desired_mode {
                    Ok(ResourceState::Correct)
                } else {
                    Ok(ResourceState::Incorrect {
                        current: format!("{current_mode:o}"),
                    })
                }
            }
        }
        #[cfg(not(unix))]
        {
            Ok(ResourceState::Invalid {
                reason: "chmod not supported on this platform".to_string(),
            })
        }
    }
}

/// Recursively check a directory and its contents against the desired mode.
///
/// Directories are compared with execute bits added (via [`ensure_dir_execute_bits`]),
/// files are compared with execute bits stripped (via [`strip_file_execute_bits`]),
/// matching the logic in [`apply_recursive`].
#[cfg(unix)]
fn check_dir_recursive(path: &std::path::Path, base_mode: u32) -> Result<ResourceState> {
    use std::os::unix::fs::PermissionsExt;

    let dir_mode = ensure_dir_execute_bits(base_mode);
    let file_mode = strip_file_execute_bits(base_mode);

    let current_mode = std::fs::metadata(path)?.permissions().mode() & MODE_BITS_MASK;
    if current_mode != dir_mode {
        return Ok(ResourceState::Incorrect {
            current: format!("directory {} has mode {current_mode:o}", path.display()),
        });
    }

    let entries =
        std::fs::read_dir(path).with_context(|| format!("reading directory {}", path.display()))?;

    for entry in entries {
        let entry = entry.with_context(|| format!("reading entry in {}", path.display()))?;
        let entry_path = entry.path();

        if entry_path.is_symlink() {
            continue;
        }

        if entry_path.is_dir() {
            if let recursive_state @ ResourceState::Incorrect { .. } =
                check_dir_recursive(&entry_path, base_mode)?
            {
                return Ok(recursive_state);
            }
        } else {
            let entry_mode = std::fs::metadata(&entry_path)?.permissions().mode() & MODE_BITS_MASK;
            if entry_mode != file_mode {
                return Ok(ResourceState::Incorrect {
                    current: format!("file {} has mode {entry_mode:o}", entry_path.display()),
                });
            }
        }
    }

    Ok(ResourceState::Correct)
}

#[cfg(unix)]
fn apply_recursive(path: &std::path::Path, dir_mode: u32, file_mode: u32) -> ResourceResult<()> {
    let effective_mode = if path.is_dir() { dir_mode } else { file_mode };
    set_permissions(path, effective_mode)?;

    if path.is_dir() {
        for entry in std::fs::read_dir(path)
            .with_context(|| format!("reading directory {}", path.display()))?
        {
            let entry = entry.with_context(|| format!("reading entry in {}", path.display()))?;
            let entry_path = entry.path();
            if entry_path.is_symlink() {
                continue;
            }
            if entry_path.is_dir() {
                apply_recursive(&entry_path, dir_mode, file_mode)?;
            } else {
                set_permissions(&entry_path, file_mode)?;
            }
        }
    }

    Ok(())
}

#[cfg(unix)]
fn set_permissions(path: &std::path::Path, mode: u32) -> ResourceResult<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(|error| {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            ResourceError::permission_denied(path.display().to_string())
        } else {
            anyhow::Error::new(error)
                .context(format!("set permissions on {}", path.display()))
                .into()
        }
    })
}

/// Add execute bits to a mode for each permission triplet that has read access.
/// This mirrors the conventional behaviour of `chmod -R`: files get the
/// specified mode, while directories get execute bits so they remain
/// traversable (e.g., mode 600 → dir mode 700).
#[cfg(unix)]
pub(super) const fn ensure_dir_execute_bits(mode: u32) -> u32 {
    let mut m = mode;
    if m & 0o400 != 0 {
        m |= 0o100;
    }
    if m & 0o040 != 0 {
        m |= 0o010;
    }
    if m & 0o004 != 0 {
        m |= 0o001;
    }
    m
}

/// Strip execute bits from a mode before applying it to regular files during
/// recursive directory chmod operations, preserving the remaining permission
/// bits unchanged.
#[cfg(unix)]
pub(super) const fn strip_file_execute_bits(mode: u32) -> u32 {
    mode & !0o111
}
