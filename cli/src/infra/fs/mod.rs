//! File-system helpers and temporary-path guards.
//!
//! This module is the single source of truth for filesystem operations:
//!
//! - [`ensure_parent_dir`] / [`copy_dir_recursive`] —
//!   shared helper functions for resource `apply()` methods.
//! - [`write_atomic`] / [`rename_into_place`] —
//!   shared staging and rename mechanics for resources that replace an
//!   existing path in place.
use anyhow::{Context as _, Result};
use base64::Engine as _;
use sha2::{Digest as _, Sha256};
use std::path::{Path, PathBuf};

mod copy;
mod directories;
mod mutation;
mod place;
mod temp;

pub(crate) use copy::open_regular_source;
pub use copy::{copy_dir_recursive, reject_destination_link};
pub(crate) use directories::checked_directory_chain;
pub(crate) use mutation::open_in_place;
#[cfg(unix)]
pub(crate) use mutation::require_single_link;
pub use place::{rename_into_place, write_atomic, write_atomic_checked};
pub use temp::TempGuard;

/// Fingerprint owned file bytes without storing their contents in a ledger.
pub(crate) fn content_hash(bytes: impl AsRef<[u8]>) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(bytes.as_ref()))
}

/// Resolve the parents of a target while preserving its final entry.
pub(crate) fn resolve_target_entry(path: &Path) -> Result<PathBuf> {
    let parent = path.parent().context("target has no parent")?;
    let name = path.file_name().context("target has no file name")?;
    Ok(resolve_directory(parent)?.join(name))
}

// Resolve existing parents without following the final target entry. A dangling
// parent link is an error rather than a missing directory we can create.
pub(crate) fn resolve_directory(path: &Path) -> Result<PathBuf> {
    resolve_directory_with(path, |ancestor| dunce::canonicalize(ancestor))
}

pub(crate) fn resolve_directory_with(
    path: &Path,
    canonicalize: impl Fn(&Path) -> std::io::Result<PathBuf>,
) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    for ancestor in absolute.ancestors() {
        let resolved = match canonicalize(ancestor) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if symlink_metadata_optional(ancestor, "inspect target parent")?.is_none() {
                    continue;
                }
                // A parallel resource may have just created this directory.
                // A dangling link will still fail the second resolution.
                canonicalize(ancestor)
            }
            result => result,
        }
        .with_context(|| format!("resolve target parent {}", ancestor.display()))?;
        return Ok(resolved.join(absolute.strip_prefix(ancestor)?));
    }
    anyhow::bail!("cannot resolve target parent {}", path.display())
}

/// Ensure the parent directory of `path` exists, creating it (and any
/// ancestors) if necessary.
///
/// This is a shared helper for resource `apply()` methods that need to
/// create parent directories before writing a file or symlink.
///
/// # Errors
///
/// Returns an error if the directory cannot be created.
pub fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create parent: {}", parent.display()))?;
    }
    Ok(())
}

/// Read symlink metadata and return `None` when the path is absent.
///
/// # Errors
///
/// Returns an error if metadata cannot be read for a reason other than the path
/// being absent.
pub fn symlink_metadata_optional(path: &Path, action: &str) -> Result<Option<std::fs::Metadata>> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(anyhow::Error::new(error).context(format!("{action}: {}", path.display())))
        }
    }
}

/// Return a standard validation message when a required source path is missing.
///
/// # Errors
///
/// Returns an error when source inspection fails for a reason other than absence.
pub fn missing_source_reason(path: &Path) -> Result<Option<String>> {
    let exists = path
        .try_exists()
        .with_context(|| format!("inspect source: {}", path.display()))?;
    Ok((!exists).then(|| format!("source does not exist: {}", path.display())))
}

/// Read a file as bytes with consistent path context.
///
/// # Errors
///
/// Returns an error if the file cannot be read.
pub fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("read {}", path.display()))
}

/// Read a UTF-8 file with consistent path context.
///
/// # Errors
///
/// Returns an error if the file cannot be read as a string.
pub fn read_string(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))
}

/// Remove a file with consistent path context.
///
/// # Errors
///
/// Returns an error if the file cannot be removed.
pub fn remove_file(path: &Path) -> Result<()> {
    std::fs::remove_file(path).with_context(|| format!("remove {}", path.display()))
}

/// Remove a file if it exists and report whether anything was removed.
///
/// # Errors
///
/// Returns an error if metadata cannot be read or the existing file cannot be
/// removed.
pub fn remove_file_if_present(path: &Path, action: &str) -> Result<bool> {
    match symlink_metadata_optional(path, action)? {
        Some(_) => {
            remove_file(path)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Set a file executable by owner, group, and others.
///
/// # Errors
///
/// Returns an error if file metadata cannot be read or permissions cannot be
/// written.
#[cfg(unix)]
pub fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut perms = std::fs::metadata(path)
        .with_context(|| format!("read executable metadata: {}", path.display()))?
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)
        .with_context(|| format!("set executable permissions: {}", path.display()))
}

/// Canonicalize a path, stripping the Windows extended-length `\\?\` prefix.
///
/// On Windows, [`std::fs::canonicalize`] returns paths prefixed with `\\?\`,
/// which breaks many tools (e.g. `Invoke-WebRequest -OutFile`). This helper
/// uses [`dunce::canonicalize`] to strip that prefix so paths remain
/// compatible with typical Windows tooling.
///
/// # Errors
///
/// Returns an error if the path does not exist or cannot be resolved.
pub fn canonicalize(path: &Path) -> Result<PathBuf> {
    dunce::canonicalize(path).with_context(|| format!("canonicalizing {}", path.display()))
}

/// Returns `true` when `meta` describes an entry that must be removed with
/// [`std::fs::remove_dir`] rather than [`std::fs::remove_file`].
///
/// On Windows a directory symlink or junction reports `is_dir() == false`
/// through [`std::fs::symlink_metadata`], yet the OS only accepts `remove_dir`
/// for it, so the raw `FILE_ATTRIBUTE_DIRECTORY` (`0x10`) flag is what decides.
/// On Unix a symlink is always removed as a file, which is exactly what
/// `is_dir()` reports for symlink metadata.
#[must_use]
pub fn is_dir_like(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        meta.file_attributes() & 0x10 != 0
    }
    #[cfg(not(windows))]
    {
        meta.is_dir()
    }
}

/// Create a native symbolic link using the platform's required file/directory
/// distinction.
///
/// `target_is_dir` is ignored on Unix, where one symlink primitive handles both
/// kinds. Windows requires the caller to classify the link before creation.
///
/// # Errors
///
/// Returns an error when the platform cannot create the requested link.
pub fn create_native_symlink(
    target: &Path,
    link: &Path,
    target_is_dir: bool,
) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let _ = target_is_dir;
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        if target_is_dir {
            std::os::windows::fs::symlink_dir(target, link)
        } else {
            std::os::windows::fs::symlink_file(target, link)
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, link, target_is_dir);
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "symbolic links are unsupported on this platform",
        ))
    }
}

#[cfg(test)]
mod tests;
