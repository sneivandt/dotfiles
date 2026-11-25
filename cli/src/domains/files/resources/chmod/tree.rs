//! Unix recursive permission inspection, temporary directory recovery, and application.

use anyhow::Context as _;
use std::path::{Path, PathBuf};

use super::targets::error_is_permission_denied;
use crate::engine::resource::ResourceError;
use crate::engine::{ResourceResult, ResourceState};

/// Unix file permission mask (all permission bits).
pub(in crate::domains::files::resources) const MODE_BITS_MASK: u32 = 0o7777;

pub(in crate::domains::files::resources) struct RecoveredDirectory {
    pub(super) path: PathBuf,
    pub(super) original: u32,
    directory: std::fs::File,
    pub(super) finalized: bool,
}

impl RecoveredDirectory {
    pub(in crate::domains::files::resources) fn acquire(
        path: &Path,
        original: u32,
    ) -> ResourceResult<Self> {
        use rustix::fs::{Mode, OFlags};

        set_permissions(path, original | 0o500)?;
        let flags = OFlags::RDONLY
            .union(OFlags::CLOEXEC)
            .union(OFlags::DIRECTORY)
            .union(OFlags::NOFOLLOW);
        let opened = rustix::fs::open(path, flags, Mode::empty())
            .map(std::fs::File::from)
            .map_err(std::io::Error::from);
        match opened {
            Ok(directory) => Ok(Self {
                path: path.to_path_buf(),
                original,
                directory,
                finalized: false,
            }),
            Err(error) => {
                let error = anyhow::Error::new(error)
                    .context(format!("open recovered directory {}", path.display()));
                match set_permissions(path, original) {
                    Ok(()) => Err(error.into()),
                    Err(restore_error) => Err(error
                        .context(format!(
                            "could not restore temporary directory access on {}: {restore_error}",
                            path.display()
                        ))
                        .into()),
                }
            }
        }
    }
}

pub(in crate::domains::files::resources) fn restore_directory_access<'a>(
    recovered: impl DoubleEndedIterator<Item = &'a RecoveredDirectory>,
) -> ResourceResult<()> {
    use std::os::unix::fs::PermissionsExt as _;

    let mut failures = Vec::new();
    for recovery in recovered.rev() {
        // A completed restrictive ancestor may no longer be traversable.
        // Restore the original directory itself, without resolving its path again.
        if let Err(error) = recovery
            .directory
            .set_permissions(std::fs::Permissions::from_mode(recovery.original))
        {
            failures.push(format!("{}: {error}", recovery.path.display()));
        }
    }
    if !failures.is_empty() {
        return Err(anyhow::anyhow!(
            "could not restore temporary directory access: {}",
            failures.join("; ")
        )
        .into());
    }
    Ok(())
}

/// Recursively check a directory and its contents against the desired mode.
///
/// Directories are compared with execute bits added (via [`ensure_dir_execute_bits`]),
/// files are compared with execute bits stripped (via [`strip_file_execute_bits`]),
/// matching the logic in [`apply_recursive`].
pub(super) fn check_dir_recursive(
    path: &Path,
    base_mode: u32,
    is_excluded: &impl Fn(&Path) -> ResourceResult<bool>,
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
    if needs_directory_access(current_mode) {
        return Ok(ResourceState::Incorrect {
            current: format!(
                "directory {} needs temporary owner access to inspect contents",
                path.display()
            ),
        });
    }

    let entries =
        std::fs::read_dir(path).with_context(|| format!("reading directory {}", path.display()))?;

    for entry in entries {
        let entry = entry.with_context(|| format!("reading entry in {}", path.display()))?;
        let entry_path = entry.path();

        let kind = entry.file_type()?;
        if !(kind.is_dir() || kind.is_file()) || is_excluded(&entry_path)? {
            continue;
        }

        if kind.is_dir() {
            if let recursive_state @ ResourceState::Incorrect { .. } =
                check_dir_recursive(&entry_path, base_mode, is_excluded)?
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

pub(super) fn apply_recursive(
    path: &Path,
    dir_mode: u32,
    file_mode: u32,
    is_excluded: &impl Fn(&Path) -> ResourceResult<bool>,
    recovered: &mut [RecoveredDirectory],
) -> ResourceResult<bool> {
    use std::os::unix::fs::PermissionsExt as _;

    let mut changed = false;
    if path.is_dir() {
        for entry in std::fs::read_dir(path)
            .with_context(|| format!("reading directory {}", path.display()))?
        {
            let entry = entry.with_context(|| format!("reading entry in {}", path.display()))?;
            let entry_path = entry.path();
            let kind = entry.file_type()?;
            if !(kind.is_dir() || kind.is_file()) || is_excluded(&entry_path)? {
                continue;
            }
            if kind.is_dir() {
                changed |=
                    apply_recursive(&entry_path, dir_mode, file_mode, is_excluded, recovered)?;
            } else {
                let current_mode =
                    std::fs::metadata(&entry_path)?.permissions().mode() & MODE_BITS_MASK;
                if current_mode != file_mode {
                    changed = true;
                    set_permissions(&entry_path, file_mode)?;
                }
            }
        }
    }

    // Keep directories traversable until every descendant has been processed.
    let effective_mode = if path.is_dir() { dir_mode } else { file_mode };
    let recovery = recovered.iter_mut().find(|recovery| recovery.path == path);
    let (current, was_recovered) = match &recovery {
        Some(rec) => (rec.original, true),
        None => (
            std::fs::metadata(path)?.permissions().mode() & MODE_BITS_MASK,
            false,
        ),
    };
    if current != effective_mode {
        changed = true;
    }
    if current != effective_mode || was_recovered {
        set_permissions(path, effective_mode)?;
    }
    if let Some(rec) = recovery {
        rec.finalized = true;
    }
    Ok(changed)
}

pub(in crate::domains::files::resources) fn set_permissions(
    path: &Path,
    mode: u32,
) -> ResourceResult<()> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.is_symlink() {
        return Err(anyhow::anyhow!("chmod target became a symlink: {}", path.display()).into());
    }
    if !metadata.is_dir() {
        crate::infra::fs::require_single_link(&metadata)?;
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(|error| {
        if error_is_permission_denied(&error) {
            ResourceError::permission_denied(path.display().to_string())
        } else {
            anyhow::Error::new(error)
                .context(format!("set permissions on {}", path.display()))
                .into()
        }
    })
}

pub(in crate::domains::files::resources) fn permission_tree_reason(
    path: &Path,
    is_excluded: &impl Fn(&Path) -> ResourceResult<bool>,
    prepare_directory: &mut impl FnMut(&Path, &std::fs::Metadata) -> ResourceResult<bool>,
) -> ResourceResult<Option<String>> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir() && !metadata.is_symlink() {
        return Ok(crate::infra::fs::require_single_link(&metadata)
            .err()
            .map(|error| format!("{}: {error}", path.display())));
    }
    if metadata.is_dir() && prepare_directory(path, &metadata)? {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let child = entry.path();
            let kind = entry.file_type()?;
            if !(kind.is_dir() || kind.is_file()) || is_excluded(&child)? {
                continue;
            }
            if let Some(reason) = permission_tree_reason(&child, is_excluded, prepare_directory)? {
                return Ok(Some(reason));
            }
        }
    }
    Ok(None)
}

pub(super) const fn needs_directory_access(current: u32) -> bool {
    current & 0o500 != 0o500
}

/// Add execute bits to a mode for each permission triplet that has read access.
/// This mirrors the conventional behaviour of `chmod -R`: files get the
/// specified mode, while directories get execute bits so they remain
/// traversable (e.g., mode 600 → dir mode 700).
pub(in crate::domains::files::resources) const fn ensure_dir_execute_bits(mode: u32) -> u32 {
    mode | ((mode & 0o444) >> 2)
}

/// Strip execute bits from a mode before applying it to regular files during
/// recursive directory chmod operations, preserving the remaining permission
/// bits unchanged.
pub(in crate::domains::files::resources) const fn strip_file_execute_bits(mode: u32) -> u32 {
    mode & !0o111
}
