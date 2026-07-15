use anyhow::{Context as _, Result};
use std::path::Path;

use crate::runtime::exec::Executor;

/// Create a symlink at `link` pointing to `target`.
#[cfg(unix)]
pub(super) fn create_symlink(target: &Path, link: &Path, _executor: &dyn Executor) -> Result<()> {
    std::os::unix::fs::symlink(target, link).with_context(|| {
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
        match std::os::windows::fs::symlink_dir(target, link) {
            Ok(()) => Ok(()),
            Err(e) => create_junction(target, link, executor)
                .with_context(|| format!("directory symlink failed: {e}")),
        }
    } else {
        std::os::windows::fs::symlink_file(target, link).map_err(anyhow::Error::from)
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

/// Remove a symlink, handling platform differences.
///
/// On Windows, directory symlinks must be removed with `remove_dir` (not
/// `remove_file`). Rust's `symlink_metadata().is_dir()` returns `false` for
/// symlinks, so the raw `FILE_ATTRIBUTE_DIRECTORY` flag detects directory
/// symlinks. If `remove_dir` still fails with OS error 5 (access denied),
/// `remove_dir_all` retries through the standard library without invoking a
/// command shell.
pub(super) fn remove_symlink(path: &Path, _executor: &dyn Executor) -> Result<()> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(e.into()),
        Err(e) => {
            return Err(
                anyhow::Error::new(e).context(format!("reading metadata: {}", path.display()))
            );
        }
    };
    if is_dir_like(&meta) {
        match std::fs::remove_dir(path) {
            Ok(()) => {}
            #[cfg(windows)]
            Err(e) if e.raw_os_error() == Some(5) => {
                std::fs::remove_dir_all(path)
                    .with_context(|| format!("removing directory symlink: {}", path.display()))?;
            }
            Err(e) => return Err(e.into()),
        }
    } else {
        std::fs::remove_file(path).with_context(|| format!("removing file: {}", path.display()))?;
    }
    Ok(())
}

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

/// Check if metadata represents a directory-like entry.
fn is_dir_like(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x10 != 0
    }
    #[cfg(not(windows))]
    {
        meta.is_dir()
    }
}

/// Create a Windows directory junction as a fallback for directory symlinks.
#[cfg(windows)]
pub(super) fn create_junction(target: &Path, link: &Path, executor: &dyn Executor) -> Result<()> {
    let link_arg = link.to_string_lossy();
    let target_arg = target.to_string_lossy();
    let result = crate::runtime::exec::windows::CmdCommand::new("mklink")
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
fn command_output(result: &crate::runtime::exec::ExecResult) -> String {
    let stdout = result.stdout.trim();
    let stderr = result.stderr.trim();
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => "no output".to_string(),
        (false, true) => stdout.to_string(),
        (true, false) => stderr.to_string(),
        (false, false) => format!("{stdout}; {stderr}"),
    }
}
