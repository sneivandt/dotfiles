//! File permission resource (chmod).
#[cfg(unix)]
use anyhow::Context as _;
use anyhow::Result;
use std::path::PathBuf;

use crate::domains::files::config::chmod::{OctalMode, target_path, validate_path};
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
    home: Option<PathBuf>,
    validation_error: Option<String>,
    excluded_targets: Vec<PathBuf>,
    source_roots: Vec<PathBuf>,
    allow_external_target: bool,
}

impl ChmodResource {
    /// Create a new chmod resource.
    #[must_use]
    #[cfg(test)]
    pub const fn new(target: PathBuf, mode: OctalMode) -> Self {
        Self {
            target,
            mode: Ok(mode),
            home: None,
            validation_error: None,
            excluded_targets: Vec::new(),
            source_roots: Vec::new(),
            allow_external_target: false,
        }
    }

    /// Create from a config entry and home directory.
    ///
    pub fn from_entry(
        entry: &crate::domains::files::config::chmod::ChmodEntry,
        home: &std::path::Path,
    ) -> Self {
        let target = home.join(target_path(&entry.path));
        Self {
            target,
            mode: entry.parsed_mode().clone(),
            home: Some(home.to_path_buf()),
            validation_error: validate_path(&entry.path).err(),
            excluded_targets: Vec::new(),
            source_roots: Vec::new(),
            allow_external_target: entry.allow_external_target,
        }
    }

    /// Permit links into the public and overlay managed source trees.
    #[must_use]
    pub fn with_source_roots(mut self, roots: &[PathBuf]) -> Self {
        self.source_roots = roots.to_vec();
        self
    }

    /// Leave explicitly configured descendants to their own permission resources.
    #[must_use]
    pub fn excluding(mut self, targets: &[PathBuf]) -> Self {
        // Traversal checks only child entries. The root itself and unrelated
        // configured destinations cannot match those entries.
        self.excluded_targets = targets.to_vec();
        self
    }

    pub(crate) fn resolved_target(&self) -> Result<PathBuf> {
        if self.target.try_exists()? {
            crate::infra::fs::canonicalize(&self.target)
        } else {
            crate::infra::fs::resolve_target_entry(&self.target)
        }
    }

    fn invalid_path_reason(&self) -> ResourceResult<Option<String>> {
        if let Some(reason) = &self.validation_error {
            return Ok(Some(reason.clone()));
        }
        if let Some(home) = &self.home
            && self.target.try_exists()?
        {
            let target = crate::infra::fs::canonicalize(&self.target)?;
            let home = crate::infra::fs::canonicalize(home)?;
            if target == home {
                return Ok(Some("chmod target resolves to $HOME itself".into()));
            }
            let managed = self
                .source_roots
                .iter()
                .map(|root| {
                    crate::infra::fs::resolve_directory(root).map(|root| target.starts_with(root))
                })
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .any(|allowed| allowed);
            if !target.starts_with(home) && !managed && !self.allow_external_target {
                return Ok(Some(format!(
                    "chmod target resolves outside home and managed sources to {}; set allow_external_target = true to permit it",
                    target.display()
                )));
            }
        }
        #[cfg(unix)]
        if self.target.try_exists()? {
            return permission_tree_reason(
                &crate::infra::fs::canonicalize(&self.target)?,
                &self.excluded_targets,
            );
        }
        Ok(None)
    }
}

impl Resource for ChmodResource {
    fn description(&self) -> String {
        match &self.mode {
            Ok(mode) => {
                let target = self.target.display();
                match std::fs::canonicalize(&self.target) {
                    Ok(resolved) if resolved != self.target && self.home.is_some() => {
                        format!("{mode} {target} (resolved {})", resolved.display())
                    }
                    _ => format!("{mode} {target}"),
                }
            }
            Err(reason) => format!(
                "invalid chmod mode ({reason}) for {}",
                self.target.display()
            ),
        }
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        if let Some(reason) = self.invalid_path_reason()? {
            return Err(ResourceError::conflicting_state(
                self.target.display().to_string(),
                "a file or directory beneath $HOME",
                reason,
            ));
        }
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
            let target = crate::infra::fs::canonicalize(&self.target)?;
            if target.is_dir() {
                apply_recursive(
                    &target,
                    ensure_dir_execute_bits(mode),
                    strip_file_execute_bits(mode),
                    &self.excluded_targets,
                )?;
            } else {
                set_permissions(&target, mode)?;
            }

            Ok(ResourceChange::Applied)
        }

        #[cfg(not(unix))]
        {
            Ok(ResourceChange::skipped(
                "chmod not supported on this platform",
            ))
        }
    }
}

impl IntrinsicState for ChmodResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        if let Some(reason) = self.invalid_path_reason()? {
            return Ok(ResourceState::Invalid { reason });
        }
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
                check_dir_recursive(
                    &crate::infra::fs::canonicalize(&self.target)?,
                    desired_mode,
                    &self.excluded_targets,
                )
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
fn check_dir_recursive(
    path: &std::path::Path,
    base_mode: u32,
    excluded_targets: &[PathBuf],
) -> ResourceResult<ResourceState> {
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

        if excluded_targets.contains(&entry_path) || entry_path.is_symlink() {
            continue;
        }

        if entry_path.is_dir() {
            if let recursive_state @ ResourceState::Incorrect { .. } =
                check_dir_recursive(&entry_path, base_mode, excluded_targets)?
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
fn apply_recursive(
    path: &std::path::Path,
    dir_mode: u32,
    file_mode: u32,
    excluded_targets: &[PathBuf],
) -> ResourceResult<()> {
    let effective_mode = if path.is_dir() { dir_mode } else { file_mode };
    set_permissions(path, effective_mode)?;

    if path.is_dir() {
        for entry in std::fs::read_dir(path)
            .with_context(|| format!("reading directory {}", path.display()))?
        {
            let entry = entry.with_context(|| format!("reading entry in {}", path.display()))?;
            let entry_path = entry.path();
            if excluded_targets.contains(&entry_path) || entry_path.is_symlink() {
                continue;
            }
            if entry_path.is_dir() {
                apply_recursive(&entry_path, dir_mode, file_mode, excluded_targets)?;
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

    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.is_file() {
        crate::infra::fs::require_single_link(&metadata)?;
    }
    if metadata.is_symlink() {
        return Err(anyhow::anyhow!("chmod target became a symlink: {}", path.display()).into());
    }
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

#[cfg(unix)]
fn permission_tree_reason(
    path: &std::path::Path,
    excluded_targets: &[PathBuf],
) -> ResourceResult<Option<String>> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.is_file() {
        return Ok(crate::infra::fs::require_single_link(&metadata)
            .err()
            .map(|error| format!("{}: {error}", path.display())));
    }
    if metadata.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let child = entry.path();
            if excluded_targets.contains(&child) {
                continue;
            }
            if let Some(reason) = permission_tree_reason(&child, excluded_targets)? {
                return Ok(Some(reason));
            }
        }
    }
    Ok(None)
}

/// Add execute bits to a mode for each permission triplet that has read access.
/// This mirrors the conventional behaviour of `chmod -R`: files get the
/// specified mode, while directories get execute bits so they remain
/// traversable (e.g., mode 600 → dir mode 700).
#[cfg(unix)]
pub(super) const fn ensure_dir_execute_bits(mode: u32) -> u32 {
    mode | ((mode & 0o444) >> 2)
}

/// Strip execute bits from a mode before applying it to regular files during
/// recursive directory chmod operations, preserving the remaining permission
/// bits unchanged.
#[cfg(unix)]
pub(super) const fn strip_file_execute_bits(mode: u32) -> u32 {
    mode & !0o111
}

#[cfg(all(test, unix))]
mod mutation_tests {
    use super::*;

    #[test]
    fn changed_final_entries_are_rejected_at_the_permission_write() {
        use std::os::unix::fs::PermissionsExt as _;
        let fixture = tempfile::tempdir().unwrap();
        let file = fixture.path().join("private");
        let alias = fixture.path().join("alias");
        std::fs::write(&file, "private").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::hard_link(&file, &alias).unwrap();
        assert!(
            permission_tree_reason(fixture.path(), &[])
                .unwrap()
                .is_some()
        );
        assert!(
            permission_tree_reason(fixture.path(), &[file.clone(), alias.clone()])
                .unwrap()
                .is_none()
        );
        assert!(set_permissions(&file, 0o644).is_err());
        std::fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&file, &alias).unwrap();
        assert!(set_permissions(&alias, 0o644).is_err());
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        set_permissions(&file, 0o640).unwrap();
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o640);
    }
}
