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
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &raw mut information) } == 0 {
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
    fn pipe_metadata_failure_preserves_the_native_error() {
        use std::os::windows::io::OwnedHandle;
        let mut child = std::process::Command::new("cmd")
            .args(["/c", "echo", "fixture"])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let handle: OwnedHandle = child.stdout.take().unwrap().into();
        let file = File::from(handle);
        let error = ensure_single_link(&file).unwrap_err();
        assert!(
            error.downcast_ref::<std::io::Error>().is_some(),
            "{error:#}"
        );
        assert!(child.wait().unwrap().success());
    }
}
