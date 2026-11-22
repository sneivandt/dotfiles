//! Filesystem operations for installing an updated binary, plus the
//! post-install smoke test and end-to-end download orchestration.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};

use super::attestation::{GhCli, Policy, SystemGh, policy, verify_provenance};
use super::cache::write_cache_best_effort;
use super::http::{HttpClient, download_to_file, expected_checksum};
use super::paths::{asset_name, binary_path, old_binary_name, old_binary_path};

fn remove_stale_backup(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("removing stale backup {}", path.display()))
        }
    }
}

fn backup_current_binary(path: &Path) -> Result<Option<PathBuf>> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("binary path has no parent directory"))?;
    if path.try_exists().context("inspecting current binary")? {
        let old = dir.join(old_binary_name());
        remove_stale_backup(&old)?;
        // Unix can atomically replace the installed entry. Keep that entry
        // reachable while retaining its inode for smoke-test rollback.
        #[cfg(unix)]
        fs::hard_link(path, &old).context("backing up current binary")?;
        #[cfg(not(unix))]
        fs::rename(path, &old).context("backing up current binary")?;
        Ok(Some(old))
    } else {
        Ok(None)
    }
}

fn install_staged_binary(path: &Path, staged: &Path) -> Result<()> {
    let backup = backup_current_binary(path)?;

    if let Err(install_error) = fs::rename(staged, path) {
        if let Some(old) = backup {
            // On Unix the original entry is still in place. Renaming its hard
            // link over it would be a no-op and leave the backup behind.
            #[cfg(unix)]
            let recovery = fs::remove_file(&old);
            #[cfg(not(unix))]
            let recovery = fs::rename(&old, path);
            if let Err(recovery_error) = recovery {
                return Err(anyhow::anyhow!(
                    "moving new binary into place failed ({install_error}); \
                     recovering previous binary also failed ({recovery_error})"
                ));
            }
        }
        return Err(install_error).context("moving new binary into place");
    }
    Ok(())
}

// Both launchers understand this journal: nonempty means publication hasn't
// been confirmed. Keep the inode even when clean so all processes lock it.
fn begin_replacement(bin: &Path) -> Result<fs::File> {
    let name = bin
        .file_name()
        .and_then(|name| name.to_str())
        .context("invalid binary name")?;
    let journal = fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(bin.with_file_name(format!(".{name}.update-state")))
        .context("opening self-update journal")?;
    journal
        .try_lock()
        .context("another process is installing or recovering the CLI; retry")?;
    anyhow::ensure!(
        journal.metadata()?.len() == 0,
        "unfinished self-update; run the checkout wrapper to recover before retrying"
    );
    journal.set_len(1).context("marking self-update pending")?;
    journal.sync_all().context("saving self-update journal")?;
    Ok(journal)
}

/// Replace the binary at `path` with a verified staging file.
///
/// Retain the current binary at [`old_binary_name`] for smoke-test rollback.
/// Unix uses a hard link so the installed path stays available until the
/// staged binary atomically replaces it. Windows renames the old executable
/// first because a running executable cannot be replaced directly there.
pub(super) fn replace_binary(path: &Path, staged: &Path) -> Result<()> {
    if path.is_dir() {
        bail!("binary path points to a directory: {}", path.display());
    }

    install_staged_binary(path, staged)
}

/// Run the binary at `path` with the `--version` flag as a basic sanity check.
///
/// Called immediately after a self-update to verify that the new binary
/// starts correctly.  On failure the caller is expected to restore the
/// backup created by [`replace_binary`].
///
/// Spawning a freshly written executable can fail transiently: on Linux
/// `exec` reports `ETXTBSY` ("Text file busy") while the kernel still holds
/// the inode open for writing (a known race on certain CI filesystems such as
/// overlayfs), and on Windows an anti-malware scanner can hold the new file
/// open long enough to produce a sharing violation.  The function retries a
/// few times with a short sleep to work around both.
///
/// # Errors
///
/// Returns an error if the process cannot be spawned or exits with a
/// non-zero status code.
fn smoke_test_binary(path: &Path) -> Result<String> {
    const MAX_RETRIES: u32 = 5;
    const BASE_DELAY_MS: u64 = 50;

    fn is_transient_busy(e: &std::io::Error) -> bool {
        if matches!(
            e.kind(),
            std::io::ErrorKind::ResourceBusy | std::io::ErrorKind::ExecutableFileBusy
        ) {
            return true;
        }
        // `ETXTBSY` is not always exposed via [`std::io::ErrorKind`] depending
        // on libc version, so match the raw OS error code as well.
        #[cfg(unix)]
        {
            /// `ETXTBSY` ("Text file busy").
            const ETXTBSY: i32 = 26;
            e.raw_os_error() == Some(ETXTBSY)
        }
        // A scanner still holding the new file surfaces as a sharing
        // violation, which maps to `PermissionDenied`.
        #[cfg(windows)]
        {
            e.kind() == std::io::ErrorKind::PermissionDenied
        }
        #[cfg(all(not(unix), not(windows)))]
        {
            false
        }
    }
    let mut attempts = 0;
    let result = loop {
        match crate::infra::exec::run_path_smoke_test(path, &["--version"]) {
            Ok(result) => break result,
            Err(e) if e.io_error().is_some_and(is_transient_busy) && attempts < MAX_RETRIES => {
                attempts = attempts.saturating_add(1);
                std::thread::sleep(std::time::Duration::from_millis(
                    BASE_DELAY_MS * u64::from(attempts),
                ));
            }
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("spawning smoke test for {}", path.display()));
            }
        }
    };

    if result.success {
        Ok(result.stdout)
    } else {
        bail!(
            "new binary failed smoke test (exit {:?}): {}",
            result.code,
            result.stderr.trim()
        )
    }
}

/// Download the release asset for the given tag and install it.
///
/// The binary is replaced in place on every platform, so the caller can
/// re-exec the new binary synchronously in the same run.
///
/// # Errors
///
/// Returns an error if the download, checksum verification, build provenance
/// verification, binary replacement, or smoke test fails.  On a smoke-test
/// failure the previous binary is restored from the backup written by
/// [`replace_binary`].
pub(super) fn download_and_install(
    root: &Path,
    tag: &str,
    client: &dyn HttpClient,
    skip_attestation: bool,
) -> Result<()> {
    install_release(
        root,
        tag,
        client,
        &SystemGh,
        policy(skip_attestation),
        smoke_test_binary,
    )
}

fn install_release(
    root: &Path,
    tag: &str,
    client: &dyn HttpClient,
    gh: &dyn GhCli,
    policy: Policy,
    version_output: impl Fn(&Path) -> Result<String>,
) -> Result<()> {
    let asset = asset_name();
    let url = format!(
        "https://github.com/{repo}/releases/download/{tag}/{asset}",
        repo = super::REPO
    );
    let bin = binary_path(root);
    let directory = bin
        .parent()
        .ok_or_else(|| anyhow::anyhow!("binary path has no parent directory"))?;
    fs::create_dir_all(directory).context("creating bin directory")?;
    let (mut staged, mut file) = crate::infra::fs::TempGuard::create_unique_file_with_mode(
        directory,
        ".dotfiles-update",
        bin.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("tmp"),
        0o600,
    )
    .context("creating staged download")?;
    let expected = expected_checksum(client, tag, asset)?;
    let actual = download_to_file(client, &url, &mut file)?;
    drop(file);
    anyhow::ensure!(
        actual.eq_ignore_ascii_case(&expected),
        "checksum mismatch for {asset}: expected {expected}, got {actual}"
    );
    verify_provenance(gh, policy, asset, staged.path())?;

    let expected_version = format!("dotfiles {tag}");
    let verify_version = |path: &Path| -> Result<()> {
        anyhow::ensure!(
            version_output(path)?
                .lines()
                .eq([expected_version.as_str()]),
            "binary smoke test did not report the expected release: {expected_version}"
        );
        Ok(())
    };
    #[cfg(unix)]
    crate::infra::fs::set_executable(staged.path())?;
    // Both launchers can recover a pending publication by checking startup.
    // Do not make a runnable but wrong release eligible for that recovery.
    verify_version(staged.path()).context("checking staged release")?;
    let journal = begin_replacement(&bin)?;
    replace_binary(&bin, staged.path())?;
    staged.persist();
    if let Err(smoke_err) = verify_version(&bin) {
        let old = old_binary_path(root);
        if old
            .try_exists()
            .with_context(|| format!("{smoke_err:#}; inspecting rollback backup"))?
        {
            fs::rename(&old, &bin).with_context(|| {
                format!(
                    "{smoke_err:#}; restoring {} also failed; retry using the checkout wrapper",
                    old.display()
                )
            })?;
            journal
                .set_len(0)
                .context("finishing self-update rollback")?;
        }
        return Err(smoke_err);
    }
    journal.set_len(0).context("confirming self-update")?;
    write_cache_best_effort(root, tag);

    Ok(())
}

#[cfg(test)]
#[path = "tests/install.rs"]
mod tests;
