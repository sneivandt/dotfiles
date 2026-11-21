//! Private directories for editors that open their own sibling lock files.

use super::{LocalDescriptor, TempGuard, require_persistent_acls, same_dacl};
#[cfg(test)]
use anyhow::Context as _;
use anyhow::Result;
use std::fs::File;
use std::io;
use std::os::windows::{
    ffi::OsStrExt as _, fs::OpenOptionsExt as _, io::AsRawHandle as _, io::FromRawHandle as _,
};
use std::path::Path;
use std::ptr;
use windows_sys::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_CREATE, FILE_DIRECTORY_FILE, FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
        },
    },
    Win32::{
        Foundation::{RtlNtStatusToDosError, UNICODE_STRING},
        Security::{DACL_SECURITY_INFORMATION, SE_DACL_PROTECTED},
        Storage::FileSystem::{
            FILE_ADD_SUBDIRECTORY, FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY,
            FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, READ_CONTROL,
            SYNCHRONIZE,
        },
        System::IO::IO_STATUS_BLOCK,
    },
};

// Protected from the outer parent, effective here AND inherited by files and
// directories. A directory-only ACE would not protect a child on Windows.
const PRIVATE_TREE: &std::ffi::CStr = c"D:P(A;OICI;FA;;;OW)";

pub(in crate::infra::fs) fn create_private_dir(
    parent: &Path,
    prefix: &str,
) -> Result<(TempGuard, File)> {
    let sd = LocalDescriptor::parse(PRIVATE_TREE)?;
    let parent_handle = File::options()
        .access_mode(FILE_ADD_SUBDIRECTORY | FILE_TRAVERSE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(parent)?;
    let (guard, handle) = TempGuard::reserve_dir_with(parent, prefix, "tmp", |path| {
        create_pinned_directory(
            &parent_handle,
            path.file_name().ok_or(io::ErrorKind::InvalidInput)?,
            &sd,
        )
    })?;
    // Close the pin before the guard's error cleanup, too.
    let pin = handle;
    require_persistent_acls(&pin)?;
    verify_directory(&pin, &sd)?;
    Ok((guard, pin))
}

fn create_pinned_directory(
    parent: &File,
    name: &std::ffi::OsStr,
    sd: &LocalDescriptor,
) -> io::Result<File> {
    let mut name: Vec<_> = name.encode_wide().collect();
    let len = u16::try_from(size_of_val(name.as_slice())).map_err(io::Error::other)?;
    let relative = UNICODE_STRING {
        Length: len,
        MaximumLength: len,
        Buffer: name.as_mut_ptr(),
    };
    let attrs = OBJECT_ATTRIBUTES {
        Length: u32::try_from(size_of::<OBJECT_ATTRIBUTES>()).map_err(io::Error::other)?,
        RootDirectory: parent.as_raw_handle(),
        ObjectName: &raw const relative,
        SecurityDescriptor: sd.0.cast(),
        ..Default::default()
    };
    let mut handle = ptr::null_mut();
    let mut iosb = IO_STATUS_BLOCK::default();
    // SAFETY: counted single-component name, descriptor, parent handle and outputs are
    // live for this synchronous call. FILE_CREATE cannot open an existing name.
    // Unlike CreateDirectoryW followed by open, creation gives us the original
    // directory handle without ever sharing delete/rename.
    let status = unsafe {
        NtCreateFile(
            &raw mut handle,
            // Metadata-only handles do not participate in Windows sharing checks.
            FILE_LIST_DIRECTORY | READ_CONTROL | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            &raw const attrs,
            &raw mut iosb,
            ptr::null(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_CREATE,
            FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT,
            ptr::null(),
            0,
        )
    };
    if status < 0 {
        // SAFETY: maps a status code without dereferencing any caller memory.
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(status) }.cast_signed(),
        ));
    }
    // SAFETY: successful synchronous FILE_CREATE transferred this one owned handle.
    Ok(unsafe { File::from_raw_handle(handle) })
}

fn verify_directory(file: &File, expected: &LocalDescriptor) -> Result<()> {
    let actual = LocalDescriptor::from_file(file, DACL_SECURITY_INFORMATION)?;
    anyhow::ensure!(
        actual.control()? & SE_DACL_PROTECTED != 0 && same_dacl(actual.0, expected.0)?,
        "staging directory lacks protected, inheritable owner-only access"
    );
    Ok(())
}

pub(in crate::infra::fs) fn verify_private_child(file: &File) -> Result<()> {
    let actual = LocalDescriptor::from_file(file, DACL_SECURITY_INFORMATION)?;
    // The inherited ACE marker varies with the filesystem's inheritance model.
    for sddl in [c"D:(A;ID;FA;;;OW)", c"D:(A;;FA;;;OW)"] {
        if same_dacl(actual.0, LocalDescriptor::parse(sddl)?.0)? {
            return Ok(());
        }
    }
    anyhow::bail!("staging file did not inherit owner-only access")
}

#[cfg(test)]
pub(crate) fn check_private_fixture(path: &Path) -> Result<()> {
    use windows_sys::Win32::{Foundation::ERROR_SHARING_VIOLATION, Storage::FileSystem::DELETE};
    let file = super::open_target(path)?.context("missing staging fixture")?;
    if file.metadata()?.is_dir() {
        verify_directory(&file, &LocalDescriptor::parse(PRIVATE_TREE)?)?;
        let error = File::options()
            .access_mode(DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .unwrap_err();
        assert_eq!(
            error.raw_os_error(),
            Some(ERROR_SHARING_VIOLATION.cast_signed()),
            "private staging directory must stay pinned until the editor is finished"
        );
        Ok(())
    } else {
        verify_private_child(&file)
    }
}
