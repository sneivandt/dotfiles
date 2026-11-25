use anyhow::{Context as _, Result};
use std::path::Path;

use crate::infra::exec::Executor;

/// Create a symlink at `link` pointing to `target`.
pub(super) fn create_symlink(target: &Path, link: &Path, executor: &dyn Executor) -> Result<()> {
    #[cfg(unix)]
    {
        let _ = executor;
        crate::infra::fs::create_native_symlink(target, link, false).with_context(|| {
            format!(
                "creating symlink {} -> {}",
                link.display(),
                target.display()
            )
        })?;
        Ok(())
    }
    #[cfg(windows)]
    {
        create_symlink_windows(target, link, executor)
    }
}

#[cfg(windows)]
fn create_symlink_windows(target: &Path, link: &Path, executor: &dyn Executor) -> Result<()> {
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

/// Check if metadata represents a managed link-like entry.
#[cfg_attr(
    not(windows),
    expect(
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
