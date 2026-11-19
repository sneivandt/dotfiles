use anyhow::{Context as _, Result};
use std::path::Path;

use crate::infra::exec::Executor;

/// Create a symlink at `link` pointing to `target`.
#[cfg(unix)]
pub(super) fn create_symlink(target: &Path, link: &Path, _executor: &dyn Executor) -> Result<()> {
    crate::infra::fs::create_native_symlink(target, link, false).with_context(|| {
        format!(
            "creating symlink {} -> {}",
            link.display(),
            target.display()
        )
    })?;

    Ok(())
}

/// Create a symlink at `link` pointing to `target`.
#[cfg(windows)]
pub(super) fn create_symlink(target: &Path, link: &Path, executor: &dyn Executor) -> Result<()> {
    let is_dir = target.is_dir();
    if is_dir {
        match crate::infra::fs::create_native_symlink(target, link, true) {
            Ok(()) => Ok(()),
            Err(e) => create_junction(target, link, executor)
                .with_context(|| format!("directory symlink failed: {e}")),
        }
    } else {
        crate::infra::fs::create_native_symlink(target, link, false).map_err(anyhow::Error::from)
    }
    .with_context(|| {
        format!(
            "creating symlink {} -> {} (enable Developer Mode or run as administrator)",
            link.display(),
            target.display()
        )
    })?;

    Ok(())
}

/// Publish a staged sibling without replacing an entry created by another writer.
#[cfg(any(target_os = "linux", target_vendor = "apple"))]
pub(super) fn rename_noreplace(source: &Path, target: &Path) -> std::io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    renameat_with(CWD, source, CWD, target, RenameFlags::NOREPLACE)?;
    Ok(())
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Win32 provides the no-replace rename missing from std"
)]
pub(super) fn rename_noreplace_windows(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::io;
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

    fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
        // Canonicalize only the parent to get a verbatim path for long names
        // without following the final entry, which may itself be a link.
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let name = path.file_name().ok_or(io::ErrorKind::InvalidInput)?;
        let absolute_path = std::fs::canonicalize(parent)?.join(name);
        let mut wide: Vec<_> = absolute_path.as_os_str().encode_wide().collect();
        if wide.contains(&0) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        wide.push(0);
        Ok(wide)
    }

    let source = wide_path(source)?;
    let target = wide_path(target)?;
    // SAFETY: Both buffers are NUL-terminated and live throughout the call.
    // Zero flags disallow replacing an entry or falling back to a copy.
    if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(any(windows, target_os = "linux", target_vendor = "apple")))]
pub(super) fn rename_noreplace_unsupported(_source: &Path, _target: &Path) -> std::io::Result<()> {
    Err(std::io::ErrorKind::Unsupported.into())
}

#[cfg(not(any(windows, target_os = "linux", target_vendor = "apple")))]
pub(super) use rename_noreplace_unsupported as rename_noreplace;
#[cfg(windows)]
pub(super) use rename_noreplace_windows as rename_noreplace;

/// Check if metadata represents a managed link-like entry.
#[cfg_attr(
    not(windows),
    allow(
        unused_variables,
        reason = "path is only needed to validate Windows reparse points"
    )
)]
pub(super) fn is_link_like(path: &Path, meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0 && std::fs::read_link(path).is_ok()
    }
    #[cfg(not(windows))]
    {
        meta.is_symlink()
    }
}

/// Create a Windows directory junction as a fallback for directory symlinks.
#[cfg(windows)]
pub(super) fn create_junction(target: &Path, link: &Path, executor: &dyn Executor) -> Result<()> {
    let link_arg = link.to_string_lossy();
    let target_arg = target.to_string_lossy();
    let result = crate::infra::exec::windows::CmdCommand::new("mklink")
        .arg("/J")
        .arg(link_arg.as_ref())
        .arg(target_arg.as_ref())
        .run_unchecked(executor)?;
    if result.success {
        Ok(())
    } else {
        Err(crate::engine::resource::ResourceError::command_failed(
            "mklink",
            format!(
                "create junction '{}' -> '{}': {}",
                link.display(),
                target.display(),
                command_output(&result)
            ),
        )
        .into())
    }
}

#[cfg(windows)]
fn command_output(result: &crate::infra::exec::ExecResult) -> String {
    let stdout = result.stdout.trim();
    let stderr = result.stderr.trim();
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => "no output".to_string(),
        (false, true) => stdout.to_string(),
        (true, false) => stderr.to_string(),
        (false, false) => format!("{stdout}; {stderr}"),
    }
}
