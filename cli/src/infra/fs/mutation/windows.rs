//! Validate mutation targets through already-open native handles.
use std::fs::File;
use std::os::windows::io::AsRawHandle as _;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
};

pub(super) fn ensure_single_link(file: &File) -> anyhow::Result<()> {
    let information = file_information(file)?;
    anyhow::ensure!(
        information.nNumberOfLinks == 1,
        "refusing in-place mutation of a multiply linked file"
    );
    Ok(())
}

pub(super) fn shared_file_identity(file: &File) -> anyhow::Result<Option<(u64, u64)>> {
    let info = file_information(file)?;
    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    Ok((info.nNumberOfLinks > 1).then(|| (u64::from(info.dwVolumeSerialNumber), index)))
}

fn file_information(file: &File) -> anyhow::Result<BY_HANDLE_FILE_INFORMATION> {
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: The file owns a live handle and the output is a correctly sized,
    // writable structure. The query does not retain either pointer.
    let status = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &raw mut information) };
    check_status(status)?;
    Ok(information)
}

fn check_status(status: i32) -> anyhow::Result<()> {
    if status == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_native_status_is_not_accepted() {
        check_status(1).unwrap();
        // SAFETY: Last-error state belongs to this test thread. No handle or
        // memory is accessed; seed the status consumed by the native boundary.
        unsafe { windows_sys::Win32::Foundation::SetLastError(5) };
        let error = check_status(0).unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<std::io::Error>()
                .unwrap()
                .raw_os_error(),
            Some(5)
        );
    }
}
