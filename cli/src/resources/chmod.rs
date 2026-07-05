//! File permission resource (chmod).
#[cfg(unix)]
use anyhow::Context as _;
use anyhow::Result;
use std::path::PathBuf;

use super::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};

/// Unix file permission mask (all permission bits).
#[cfg(unix)]
const MODE_BITS_MASK: u32 = 0o7777;

/// A validated octal file permission mode (e.g., `"600"`, `"0755"`).
///
/// Parsing validates that the string is 3–4 ASCII octal digits, so
/// consumers can call [`as_u32`](Self::as_u32) without error handling.
///
/// # Examples
///
/// ```
/// use dotfiles_cli::testing::resources::chmod::OctalMode;
///
/// let mode = OctalMode::parse("755").unwrap();
/// assert_eq!(mode.as_u32(), 0o755);
/// assert_eq!(mode.as_str(), "755");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OctalMode {
    raw: String,
    bits: u32,
}

/// Minimum length for octal mode strings.
const OCTAL_MODE_MIN_LEN: usize = 3;

/// Maximum length for octal mode strings.
const OCTAL_MODE_MAX_LEN: usize = 4;

impl OctalMode {
    /// Parse and validate an octal mode string.
    ///
    /// Accepts 3- or 4-digit strings consisting of octal digits (`0`–`7`).
    ///
    /// # Errors
    ///
    /// Returns a human-readable error message if the string is invalid.
    pub fn parse(s: &str) -> std::result::Result<Self, String> {
        if !s.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!(
                "invalid octal mode '{s}': must contain only digits"
            ));
        }
        if s.len() < OCTAL_MODE_MIN_LEN || s.len() > OCTAL_MODE_MAX_LEN {
            return Err(format!(
                "invalid mode length '{s}': must be {OCTAL_MODE_MIN_LEN} or {OCTAL_MODE_MAX_LEN} digits"
            ));
        }
        if let Some(c) = s.chars().find(|&c| c > '7') {
            return Err(format!("invalid octal digit '{c}' in mode '{s}'"));
        }
        // Validated above, so this cannot fail.
        let bits = u32::from_str_radix(s, 8).map_err(|e| e.to_string())?;
        Ok(Self {
            raw: s.to_string(),
            bits,
        })
    }

    /// The numeric permission bits.
    #[must_use]
    #[cfg_attr(
        not(unix),
        allow(
            dead_code,
            reason = "numeric chmod bits are only consumed by Unix permission operations"
        )
    )]
    pub const fn as_u32(&self) -> u32 {
        self.bits
    }

    /// The original string representation.
    #[must_use]
    #[cfg(any(test, feature = "internal-api", doctest))]
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

impl std::fmt::Display for OctalMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.raw)
    }
}

/// A file permission resource that can be checked and applied (Unix only).
#[derive(Debug, Clone)]
pub struct ChmodResource {
    /// Target file path (absolute).
    pub target: PathBuf,
    /// Validated permission mode.
    pub mode: OctalMode,
}

impl ChmodResource {
    /// Create a new chmod resource.
    #[must_use]
    pub const fn new(target: PathBuf, mode: OctalMode) -> Self {
        Self { target, mode }
    }

    /// Create from a config entry and home directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the mode string in the entry is not valid octal.
    pub fn from_entry(
        entry: &crate::config::chmod::ChmodEntry,
        home: &std::path::Path,
    ) -> Result<Self> {
        let relative_path = entry.path.strip_prefix('.').unwrap_or(&entry.path);
        let target = home.join(format!(".{relative_path}"));
        let mode = OctalMode::parse(&entry.mode).map_err(|msg| anyhow::anyhow!("{msg}"))?;
        Ok(Self::new(target, mode))
    }
}

impl Resource for ChmodResource {
    fn description(&self) -> String {
        format!("{} {}", self.mode, self.target.display())
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mode = self.mode.as_u32();

            if self.target.is_dir() {
                apply_recursive(
                    &self.target,
                    ensure_dir_execute_bits(mode),
                    strip_file_execute_bits(mode),
                )?;
            } else {
                let perms = std::fs::Permissions::from_mode(mode);
                std::fs::set_permissions(&self.target, perms)
                    .with_context(|| format!("set permissions: {}", self.target.display()))?;
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
        if !self.target.exists() {
            return Ok(ResourceState::Missing);
        }

        // Get current mode (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let desired_mode = self.mode.as_u32();
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
fn apply_recursive(path: &std::path::Path, dir_mode: u32, file_mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let effective_mode = if path.is_dir() { dir_mode } else { file_mode };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(effective_mode))
        .with_context(|| format!("set permissions on {}", path.display()))?;

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
                std::fs::set_permissions(&entry_path, std::fs::Permissions::from_mode(file_mode))
                    .with_context(|| format!("set permissions on {}", entry_path.display()))?;
            }
        }
    }

    Ok(())
}

/// Add execute bits to a mode for each permission triplet that has read access.
/// This mirrors the conventional behaviour of `chmod -R`: files get the
/// specified mode, while directories get execute bits so they remain
/// traversable (e.g., mode 600 → dir mode 700).
#[cfg(unix)]
const fn ensure_dir_execute_bits(mode: u32) -> u32 {
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
const fn strip_file_execute_bits(mode: u32) -> u32 {
    mode & !0o111
}

#[cfg(test)]
#[path = "tests/chmod.rs"]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests;
