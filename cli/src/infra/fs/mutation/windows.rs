//! Validate mutation targets through already-open native handles.
use std::fs::File;
use std::os::windows::io::AsRawHandle as _;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
};

pub(super) fn ensure_single_link(file: &File) -> anyhow::Result<()> {
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: The file owns a live handle and the output is a correctly sized,
    // writable structure. The query does not retain either pointer.
    let status = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &raw mut information) };
    check_information(status, &information)
}

fn check_information(status: i32, information: &BY_HANDLE_FILE_INFORMATION) -> anyhow::Result<()> {
    if status == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    anyhow::ensure!(
        information.nNumberOfLinks == 1,
        "refusing in-place mutation of a multiply linked file"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_native_status_is_not_accepted_even_with_ordinary_link_metadata() {
        let information = BY_HANDLE_FILE_INFORMATION {
            nNumberOfLinks: 1,
            ..Default::default()
        };
        check_information(1, &information).unwrap();
        // SAFETY: Last-error state belongs to this test thread. No handle or
        // memory is accessed; seed the status consumed by the native boundary.
        unsafe { windows_sys::Win32::Foundation::SetLastError(5) };
        let error = check_information(0, &information).unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<std::io::Error>()
                .unwrap()
                .raw_os_error(),
            Some(5)
        );
    }
}
