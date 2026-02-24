use anyhow::{Context as _, Result};
use std::path::PathBuf;

use super::{Resource, ResourceChange, ResourceState};

/// Unix file permission mask (all permission bits).
#[cfg(unix)]
const MODE_BITS_MASK: u32 = 0o7777;

/// A file permission resource that can be checked and applied (Unix only).
#[derive(Debug, Clone)]
pub struct ChmodResource {
    /// Target file path (absolute).
    pub target: PathBuf,
    /// Permission mode (e.g., "600", "755").
    pub mode: String,
}

impl ChmodResource {
    /// Create a new chmod resource.
    #[must_use]
    pub const fn new(target: PathBuf, mode: String) -> Self {
        Self { target, mode }
    }

    /// Create from a config entry and home directory.
    #[must_use]
    pub fn from_entry(entry: &crate::config::chmod::ChmodEntry, home: &std::path::Path) -> Self {
        let target = home.join(format!(".{}", entry.path));
        Self::new(target, entry.mode.clone())
    }
}

impl Resource for ChmodResource {
    fn description(&self) -> String {
        format!("{} {}", self.mode, self.target.display())
    }

    fn current_state(&self) -> Result<ResourceState> {
        // Check if target exists
        if !self.target.exists() {
            return Ok(ResourceState::Invalid {
                reason: format!("target does not exist: {}", self.target.display()),
            });
        }

        // Parse the desired mode
        let desired_mode = u32::from_str_radix(&self.mode, 8)
            .with_context(|| format!("invalid octal mode: {}", self.mode))?;

        // Get current mode (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
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

        #[cfg(not(unix))]
        {
            let _ = desired_mode; // Suppress unused warning
            Ok(ResourceState::Invalid {
                reason: "chmod not supported on this platform".to_string(),
            })
        }
    }

    fn apply(&self) -> Result<ResourceChange> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mode = u32::from_str_radix(&self.mode, 8)
                .with_context(|| format!("invalid octal mode: {}", self.mode))?;

            if self.target.is_dir() {
                apply_recursive(&self.target, mode)?;
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

#[cfg(unix)]
fn apply_recursive(path: &std::path::Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    // For directories, ensure the execute bit is set for each permission
    // triplet that has read access, so directories remain traversable.
    let dir_mode = ensure_dir_execute_bits(mode);

    let effective_mode = if path.is_dir() { dir_mode } else { mode };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(effective_mode))
        .with_context(|| format!("set permissions on {}", path.display()))?;

    if path.is_dir() {
        for entry in std::fs::read_dir(path)
            .with_context(|| format!("reading directory {}", path.display()))?
        {
            let entry = entry.with_context(|| format!("reading entry in {}", path.display()))?;
            let entry_path = entry.path();
            if entry_path.is_dir() {
                apply_recursive(&entry_path, mode)?;
            } else {
                std::fs::set_permissions(&entry_path, std::fs::Permissions::from_mode(mode))
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

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic
)]
mod tests {
    use super::*;

    #[test]
    fn chmod_resource_description() {
        let resource =
            ChmodResource::new(PathBuf::from("/home/user/.ssh/config"), "600".to_string());
        assert!(resource.description().contains("600"));
        assert!(resource.description().contains(".ssh/config"));
    }

    #[test]
    fn chmod_resource_invalid_when_target_missing() {
        let temp_dir = tempfile::tempdir().unwrap();
        let resource = ChmodResource::new(temp_dir.path().join("nonexistent"), "600".to_string());

        let state = resource.current_state().unwrap();
        assert!(matches!(state, ResourceState::Invalid { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn chmod_resource_detects_correct_mode() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let file = temp_dir.path().join("test.txt");
        std::fs::write(&file, "test").unwrap();

        // Set to 644
        let perms = std::fs::Permissions::from_mode(0o644);
        std::fs::set_permissions(&file, perms).unwrap();

        let resource = ChmodResource::new(file, "644".to_string());
        let state = resource.current_state().unwrap();
        assert_eq!(state, ResourceState::Correct);
    }

    #[cfg(unix)]
    #[test]
    fn chmod_resource_detects_incorrect_mode() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let file = temp_dir.path().join("test.txt");
        std::fs::write(&file, "test").unwrap();

        // Set to 644
        let perms = std::fs::Permissions::from_mode(0o644);
        std::fs::set_permissions(&file, perms).unwrap();

        // Check for 600
        let resource = ChmodResource::new(file, "600".to_string());
        let state = resource.current_state().unwrap();
        match state {
            ResourceState::Incorrect { current } => {
                assert_eq!(current, "644");
            }
            _ => panic!("Expected Incorrect state"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn chmod_resource_applies_change() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let file = temp_dir.path().join("test.txt");
        std::fs::write(&file, "test").unwrap();

        // Set to 644
        let perms = std::fs::Permissions::from_mode(0o644);
        std::fs::set_permissions(&file, perms).unwrap();

        // Apply 600
        let resource = ChmodResource::new(file.clone(), "600".to_string());
        let result = resource.apply().unwrap();
        assert_eq!(result, ResourceChange::Applied);

        // Verify the change
        let current_mode = std::fs::metadata(&file).unwrap().permissions().mode() & MODE_BITS_MASK;
        assert_eq!(current_mode, 0o600);
    }

    #[test]
    fn from_entry_creates_resource() {
        let entry = crate::config::chmod::ChmodEntry {
            mode: "600".to_string(),
            path: "ssh/config".to_string(),
        };

        let home = std::path::Path::new("/home/user");
        let resource = ChmodResource::from_entry(&entry, home);

        assert_eq!(resource.mode, "600");
        assert_eq!(
            resource.target,
            std::path::PathBuf::from("/home/user/.ssh/config")
        );
    }

    #[cfg(unix)]
    #[test]
    fn ensure_dir_execute_bits_adds_x_for_read() {
        // 600 (rw-------) → 700 (rwx------) for directories
        assert_eq!(ensure_dir_execute_bits(0o600), 0o700);
        // 644 (rw-r--r--) → 755 (rwxr-xr-x)
        assert_eq!(ensure_dir_execute_bits(0o644), 0o755);
        // 640 (rw-r-----) → 750 (rwxr-x---)
        assert_eq!(ensure_dir_execute_bits(0o640), 0o750);
        // 755 stays 755
        assert_eq!(ensure_dir_execute_bits(0o755), 0o755);
        // 000 stays 000
        assert_eq!(ensure_dir_execute_bits(0o000), 0o000);
    }
}
