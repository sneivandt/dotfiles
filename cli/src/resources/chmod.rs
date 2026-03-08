//! File permission resource (chmod).
use anyhow::{Context as _, Result};
use std::path::PathBuf;

use super::{Applicable, Resource, ResourceChange, ResourceState};

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

impl Applicable for ChmodResource {
    fn description(&self) -> String {
        format!("{} {}", self.mode, self.target.display())
    }

    fn apply(&self) -> Result<ResourceChange> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mode = u32::from_str_radix(&self.mode, 8)
                .with_context(|| format!("invalid octal mode: {}", self.mode))?;

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

impl Resource for ChmodResource {
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
            let _ = desired_mode;
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
            if let state @ ResourceState::Incorrect { .. } =
                check_dir_recursive(&entry_path, base_mode)?
            {
                return Ok(state);
            }
        } else {
            let current_mode =
                std::fs::metadata(&entry_path)?.permissions().mode() & MODE_BITS_MASK;
            if current_mode != file_mode {
                return Ok(ResourceState::Incorrect {
                    current: format!("file {} has mode {current_mode:o}", entry_path.display()),
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

    #[cfg(unix)]
    #[test]
    fn strip_file_execute_bits_removes_x_bits() {
        assert_eq!(strip_file_execute_bits(0o700), 0o600);
        assert_eq!(strip_file_execute_bits(0o755), 0o644);
        assert_eq!(strip_file_execute_bits(0o644), 0o644);
    }

    #[cfg(unix)]
    #[test]
    fn chmod_directory_applies_safe_file_mode_recursively() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let nested_dir = temp_dir.path().join("nested");
        let file = temp_dir.path().join("secret.txt");
        std::fs::create_dir(&nested_dir).unwrap();
        std::fs::write(&file, "secret").unwrap();

        let resource = ChmodResource::new(temp_dir.path().to_path_buf(), "700".to_string());
        let result = resource.apply().unwrap();
        assert_eq!(result, ResourceChange::Applied);

        let root_mode = std::fs::metadata(temp_dir.path())
            .unwrap()
            .permissions()
            .mode()
            & MODE_BITS_MASK;
        let nested_mode =
            std::fs::metadata(&nested_dir).unwrap().permissions().mode() & MODE_BITS_MASK;
        let file_mode = std::fs::metadata(&file).unwrap().permissions().mode() & MODE_BITS_MASK;

        assert_eq!(root_mode, 0o700);
        assert_eq!(nested_mode, 0o700);
        assert_eq!(file_mode, 0o600);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[cfg(unix)]
    #[test]
    fn current_state_detects_wrong_file_inside_correct_directory() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let file = temp_dir.path().join("secret.txt");
        std::fs::write(&file, "secret").unwrap();

        // Set the directory to the correct mode (700), but leave the file
        // at the default mode (644). current_state should detect the file
        // has wrong permissions even though the root directory is correct.
        std::fs::set_permissions(temp_dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();

        let resource = ChmodResource::new(temp_dir.path().to_path_buf(), "700".to_string());
        let state = resource.current_state().unwrap();
        assert!(
            matches!(state, ResourceState::Incorrect { .. }),
            "expected Incorrect when a file inside has wrong perms, got {state:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn current_state_skips_symlinks_inside_directory() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let file = temp_dir.path().join("ok.txt");
        std::fs::write(&file, "ok").unwrap();

        // Create a symlink pointing to a target that doesn't exist.
        // The recursive check should skip symlinks entirely.
        std::os::unix::fs::symlink("/nonexistent", temp_dir.path().join("dangling")).unwrap();

        std::fs::set_permissions(temp_dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();

        let resource = ChmodResource::new(temp_dir.path().to_path_buf(), "700".to_string());
        assert_eq!(
            resource.current_state().unwrap(),
            ResourceState::Correct,
            "symlinks should be skipped during recursive check"
        );
    }
}
