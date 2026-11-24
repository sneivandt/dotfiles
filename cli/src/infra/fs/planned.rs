//! Directory comparisons during a preview, before a missing suffix is created.

use anyhow::Result;
use std::path::{Component, Path, PathBuf};

#[cfg(target_os = "linux")]
mod linux;

pub(crate) struct PlannedDirectory {
    existing: PathBuf,
    missing: PathBuf,
}

/// Only used on empty temporary fixture directories. Never request elevation.
#[cfg(test)]
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "set case sensitivity on the private fixture directory handle"
)]
pub(crate) fn enable_fixture_case_sensitivity(path: &Path) -> Result<bool> {
    use std::os::windows::{fs::OpenOptionsExt as _, io::AsRawHandle as _};
    use windows_sys::Win32::{
        Foundation::{
            ERROR_ACCESS_DENIED, ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER,
            ERROR_NOT_SUPPORTED, ERROR_PRIVILEGE_NOT_HELD,
        },
        Storage::FileSystem::{
            FILE_CASE_SENSITIVE_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_WRITE_ATTRIBUTES,
            FileCaseSensitiveInfo, SetFileInformationByHandle,
        },
    };
    if directory_case_sensitive(path)? {
        return Ok(true);
    }
    let dir = std::fs::File::options()
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    let info = FILE_CASE_SENSITIVE_INFO { Flags: 1 };
    // SAFETY: the live fixture handle and correctly sized input are valid.
    if unsafe {
        SetFileInformationByHandle(
            dir.as_raw_handle(),
            FileCaseSensitiveInfo,
            (&raw const info).cast(),
            u32::try_from(size_of::<FILE_CASE_SENSITIVE_INFO>())?,
        )
    } != 0
    {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    match error
        .raw_os_error()
        .and_then(|code| u32::try_from(code).ok())
    {
        Some(
            ERROR_ACCESS_DENIED
            | ERROR_PRIVILEGE_NOT_HELD
            | ERROR_INVALID_FUNCTION
            | ERROR_INVALID_PARAMETER
            | ERROR_NOT_SUPPORTED,
        ) => Ok(false),
        _ => Err(error.into()),
    }
}

impl PlannedDirectory {
    pub(crate) fn resolve(path: &Path) -> Result<Self> {
        let (existing, missing) =
            super::resolve_with_suffix(path, |ancestor| dunce::canonicalize(ancestor))?;
        anyhow::ensure!(
            !missing
                .components()
                .any(|part| part == Component::ParentDir),
            "cannot resolve {}; use a path without unresolved '..' components",
            path.display()
        );
        anyhow::ensure!(
            std::fs::metadata(&existing)?.is_dir(),
            "not a directory: {}",
            existing.display()
        );
        Ok(Self { existing, missing })
    }

    pub(crate) fn same_directory(&self, other: &Self) -> Result<bool> {
        Ok(self.existing == other.existing
            && self.missing.components().count() == other.missing.components().count()
            && self.is_within(other)?)
    }

    /// Check tree separation for copying, not the identity of a declared entry.
    pub(crate) fn trees_overlap(&self, other: &Self) -> Result<bool> {
        #[cfg(target_os = "linux")]
        {
            linux::aliased_trees_overlap(self, other)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Ok(self.is_within(other)? || other.is_within(self)?)
        }
    }

    #[cfg_attr(
        not(windows),
        allow(
            clippy::unnecessary_wraps,
            reason = "Windows must query the containing directory before comparing missing names"
        )
    )]
    fn is_within(&self, parent: &Self) -> Result<bool> {
        if parent.missing.as_os_str().is_empty() {
            return Ok(self.existing.starts_with(&parent.existing));
        }
        // If one lookup stopped sooner, its first missing entry cannot alias
        // the other's existing entry. Never fold the canonical existing prefix.
        if self.existing != parent.existing {
            return Ok(false);
        }
        if self.missing.starts_with(&parent.missing) {
            return Ok(true);
        }
        #[cfg(windows)]
        {
            self.is_within_windows_alias(parent)
        }
        #[cfg(not(windows))]
        Ok(false)
    }

    #[cfg(windows)]
    fn is_within_windows_alias(&self, parent: &Self) -> Result<bool> {
        if directory_case_sensitive(&self.existing)? {
            return Ok(false);
        }
        let mut candidate = self.missing.components();
        for part in parent.missing.components() {
            let Some(next) = candidate.next() else {
                return Ok(false);
            };
            if !ordinal_alias(part.as_os_str(), next.as_os_str())? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "read the enclosing directory's case-sensitivity flag"
)]
fn directory_case_sensitive(path: &Path) -> Result<bool> {
    use anyhow::Context as _;
    use std::os::windows::{fs::OpenOptionsExt as _, io::AsRawHandle as _};
    use windows_sys::Win32::{
        Foundation::{ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED},
        Storage::FileSystem::{
            FILE_CASE_SENSITIVE_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES,
            FileCaseSensitiveInfo, GetFileInformationByHandleEx,
        },
    };
    let directory = std::fs::File::options()
        .access_mode(FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .with_context(|| format!("opening directory {}", path.display()))?;
    let mut info = FILE_CASE_SENSITIVE_INFO::default();
    // SAFETY: the directory handle is live and info is the expected writable structure.
    let success = unsafe {
        GetFileInformationByHandleEx(
            directory.as_raw_handle(),
            FileCaseSensitiveInfo,
            (&raw mut info).cast(),
            u32::try_from(size_of::<FILE_CASE_SENSITIVE_INFO>())?,
        )
    };
    if success != 0 {
        return Ok(info.Flags & 1 != 0); // FILE_CS_FLAG_CASE_SENSITIVE_DIR
    }
    let error = std::io::Error::last_os_error();
    match error
        .raw_os_error()
        .and_then(|code| u32::try_from(code).ok())
    {
        // Volumes/versions without per-directory case sensitivity.
        Some(ERROR_INVALID_FUNCTION | ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED) => Ok(false),
        _ => Err(error).with_context(|| format!("checking case sensitivity of {}", path.display())),
    }
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Win32 ordinal comparison for not-yet-created path components"
)]
fn ordinal_alias(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> Result<bool> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
    let a: Vec<_> = a.encode_wide().collect();
    let b: Vec<_> = b.encode_wide().collect();
    // SAFETY: both buffers are live for their specified lengths.
    let result = unsafe {
        CompareStringOrdinal(
            a.as_ptr(),
            i32::try_from(a.len())?,
            b.as_ptr(),
            i32::try_from(b.len())?,
            1,
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(result == CSTR_EQUAL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_identity_requires_the_same_prefix_and_entire_missing_suffix() {
        let fixture = tempfile::tempdir().unwrap();
        let left = fixture.path().join("left");
        let right = fixture.path().join("right");
        std::fs::create_dir(&left).unwrap();
        std::fs::create_dir(&right).unwrap();

        let planned = PlannedDirectory::resolve(&left.join("next")).unwrap();
        for (candidate, same) in [
            (left.join("./next"), true),
            (left.join("next/child"), false),
            (left.join("another"), false),
            (right.join("next"), false),
        ] {
            let other = PlannedDirectory::resolve(&candidate).unwrap();
            assert_eq!(
                planned.same_directory(&other).unwrap(),
                same,
                "{candidate:?}"
            );
            assert_eq!(
                other.same_directory(&planned).unwrap(),
                same,
                "{candidate:?}"
            );
        }
    }

    #[test]
    fn missing_descendants_stay_within_their_own_existing_prefix() {
        let fixture = tempfile::tempdir().unwrap();
        let left = fixture.path().join("left");
        let right = fixture.path().join("right");
        std::fs::create_dir(&left).unwrap();
        std::fs::create_dir(&right).unwrap();

        let parent = PlannedDirectory::resolve(&left.join("next")).unwrap();
        let child = PlannedDirectory::resolve(&left.join("next/item")).unwrap();
        let different_parent = PlannedDirectory::resolve(&right.join("next")).unwrap();
        assert!(child.is_within(&parent).unwrap());
        assert!(!parent.is_within(&child).unwrap());
        assert!(!child.is_within(&different_parent).unwrap());
        assert!(parent.trees_overlap(&child).unwrap());
        assert!(child.trees_overlap(&parent).unwrap());
        assert!(!child.trees_overlap(&different_parent).unwrap());
    }
}
