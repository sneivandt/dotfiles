//! Read the owner of a lock held by a release predating restart leases.
use std::fs::File;
use std::io;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::path::Path;

use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::{
    Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    },
    Memory::{CreateFileMappingW, FILE_MAP_READ, MapViewOfFile, PAGE_READONLY, UnmapViewOfFile},
};

pub(super) fn legacy_owner(path: &Path) -> anyhow::Result<(Vec<u8>, u32)> {
    let file = File::open(path)?;
    let len = usize::try_from(file.metadata()?.len())?;
    anyhow::ensure!(
        (1..=4096).contains(&len),
        "invalid legacy run-lock metadata size"
    );

    // Windows denies ordinary reads of the locked bytes, but a read-only
    // mapped view can read them.
    // SAFETY: The mapping is read-only and bounded by the file size. All handles
    // and the view stay live through the copy, and each is released once.
    let bytes = unsafe {
        let mapping = CreateFileMappingW(
            file.as_raw_handle(),
            std::ptr::null(),
            PAGE_READONLY,
            0,
            0,
            std::ptr::null(),
        );
        if mapping.is_null() {
            return Err(io::Error::last_os_error().into());
        }
        let _mapping = OwnedHandle::from_raw_handle(mapping);
        let view = MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, len);
        if view.Value.is_null() {
            return Err(io::Error::last_os_error().into());
        }
        let bytes = std::slice::from_raw_parts(view.Value.cast::<u8>(), len).to_vec();
        UnmapViewOfFile(view);
        bytes
    };
    Ok((bytes, parent_pid()?))
}

fn parent_pid() -> io::Result<u32> {
    // SAFETY: Use an owned snapshot handle and a zero-initialized output
    // structure with the size required by the process enumeration API.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let _snapshot = OwnedHandle::from_raw_handle(snapshot);
        let mut entry = PROCESSENTRY32W {
            dwSize: u32::try_from(size_of::<PROCESSENTRY32W>()).map_err(io::Error::other)?,
            ..PROCESSENTRY32W::default()
        };
        let mut found = Process32FirstW(snapshot, &raw mut entry);
        while found != 0 {
            if entry.th32ProcessID == std::process::id() {
                return Ok(entry.th32ParentProcessID);
            }
            found = Process32NextW(snapshot, &raw mut entry);
        }
        Err(io::Error::last_os_error())
    }
}
