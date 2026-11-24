//! Account storage and file identity for Windows checkouts.

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt as _;
use std::os::windows::fs::OpenOptionsExt as _;
use std::os::windows::io::AsRawHandle as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use windows_sys::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_INFO, FILE_READ_ATTRIBUTES, FileIdInfo,
    GetFileInformationByHandleEx,
};
use windows_sys::Win32::System::Com::{
    COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows_sys::Win32::UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath};

pub(super) fn checkout_lock_name(common_dir: &Path) -> Result<String> {
    let directory = std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(common_dir)?;
    anyhow::ensure!(
        directory.metadata()?.is_dir(),
        "Git common directory is not a directory"
    );
    let mut info = FILE_ID_INFO::default();
    // SAFETY: The file handle is live and the buffer has the expected size
    // and layout for FileIdInfo. Neither argument is retained by the call.
    let status = unsafe {
        GetFileInformationByHandleEx(
            directory.as_raw_handle(),
            FileIdInfo,
            (&raw mut info).cast(),
            u32::try_from(size_of::<FILE_ID_INFO>())?,
        )
    };
    if status == 0 {
        return Err(io::Error::last_os_error().into());
    }
    let file_id = u128::from_be_bytes(info.FileId.Identifier);
    anyhow::ensure!(
        file_id != 0,
        "Windows filesystem returned an empty file identity"
    );
    Ok(format!(
        "{:016x}-{file_id:032x}.lock",
        info.VolumeSerialNumber
    ))
}

pub(super) fn account_lock_directory() -> Result<PathBuf> {
    let directory = local_app_data()?.join("dotfiles").join("checkout-locks");
    std::fs::create_dir_all(&directory)
        .with_context(|| format!("creating {}", directory.display()))?;
    Ok(directory)
}

fn local_app_data() -> Result<PathBuf> {
    let _com = ComApartment::initialize()?;
    let mut folder = KnownFolder(core::ptr::null_mut());
    // SAFETY: The ID and pointer to the result remain valid for the call.
    // A null token requests the current Windows user's known folder.
    let status = unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_LocalAppData,
            0,
            core::ptr::null_mut(),
            &raw mut folder.0,
        )
    };
    anyhow::ensure!(
        status >= 0 && !folder.0.is_null(),
        "reading Windows LocalAppData known folder failed with HRESULT {status:#x}"
    );
    // SAFETY: The known folder API returned a live NUL-terminated UTF-16
    // buffer, which remains owned by the local guard until the path is copied.
    let path = unsafe {
        let length = (0..=usize::from(u16::MAX))
            .find(|offset| *folder.0.add(*offset) == 0)
            .context("Windows LocalAppData path has no terminator")?;
        PathBuf::from(OsString::from_wide(std::slice::from_raw_parts(
            folder.0, length,
        )))
    };
    anyhow::ensure!(
        path.is_absolute(),
        "Windows LocalAppData path is not absolute"
    );
    Ok(path)
}

struct KnownFolder(*mut u16);

impl Drop for KnownFolder {
    fn drop(&mut self) {
        // SAFETY: Any allocation returned by SHGetKnownFolderPath belongs to
        // this guard, including on failure. CoTaskMemFree accepts null.
        unsafe { CoTaskMemFree(self.0.cast()) };
    }
}

struct ComApartment(bool);

impl ComApartment {
    fn initialize() -> Result<Self> {
        let apartment = u32::try_from(COINIT_MULTITHREADED)?;
        // SAFETY: The reserved argument is null and the guard balances every
        // successful initialization on this same thread.
        let status = unsafe { CoInitializeEx(core::ptr::null(), apartment) };
        anyhow::ensure!(
            status >= 0 || status == RPC_E_CHANGED_MODE,
            "initializing COM for Windows LocalAppData failed with HRESULT {status:#x}"
        );
        // RPC_E_CHANGED_MODE means COM is already available in another mode.
        Ok(Self(status >= 0))
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: This guard owns one successful initialization and never
            // leaves the thread that initialized it.
            unsafe { CoUninitialize() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Com::COINIT_APARTMENTTHREADED;

    #[test]
    fn com_apartment_tracks_ownership_across_fresh_and_sta_threads() {
        std::thread::spawn(|| {
            let com = ComApartment::initialize().expect("fresh MTA thread");
            assert!(com.0, "fresh thread must own its COM initialization");
        })
        .join()
        .unwrap();

        std::thread::spawn(|| {
            let sta = u32::try_from(COINIT_APARTMENTTHREADED).unwrap();
            // SAFETY: Balanced with CoUninitialize at the end of this thread.
            let status = unsafe { CoInitializeEx(core::ptr::null(), sta) };
            assert!(status >= 0, "expected STA init to succeed: {status:#x}");
            let com = ComApartment::initialize().expect("existing STA thread");
            assert!(!com.0, "RPC_E_CHANGED_MODE must not claim COM ownership");
            drop(com);
            // SAFETY: Balances the STA CoInitializeEx call above.
            unsafe { CoUninitialize() };
        })
        .join()
        .unwrap();
    }
}
