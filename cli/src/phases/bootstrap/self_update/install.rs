//! Filesystem operations for installing or staging an updated binary, plus
//! the post-install smoke test and end-to-end download orchestration.

use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{Context as _, Result, bail};

use super::cache::write_cache;
use super::http::{HttpClient, download_bytes, verify_checksum};
use super::paths::asset_name;
#[cfg(not(windows))]
use super::paths::{binary_path, old_binary_path};
#[cfg(windows)]
use super::paths::{pending_binary_path, pending_version_path};

/// Replace the binary at `path` with `data`, handling platform differences.
#[cfg_attr(windows, allow(dead_code))]
pub(super) fn replace_binary(path: &Path, data: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("binary path has no parent directory"))?;
    fs::create_dir_all(dir).context("creating bin directory")?;

    let tmp_path = dir.join(".dotfiles-update.tmp");
    let mut tmp = crate::fs::TempPath::new(tmp_path);

    {
        let mut f = fs::File::create(tmp.path()).context("creating temp file")?;
        f.write_all(data).context("writing binary data")?;
        f.flush().context("flushing binary data")?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o755))
            .context("setting executable permission")?;
    }

    if path.is_dir() {
        bail!("binary path points to a directory: {}", path.display());
    }

    // On Windows, the running binary is locked. Rename the current binary out
    // of the way first, then move the new one into place.
    #[cfg(windows)]
    if path.exists() {
        let old = dir.join(".dotfiles-old.exe");
        fs::remove_file(&old).ok();
        fs::rename(path, &old).context("renaming current binary to .old")?;
    }

    // On Unix the running binary can be overwritten, but we keep a backup
    // so that the smoke test in `download_and_install` can restore it on
    // failure.
    #[cfg(unix)]
    if path.exists() {
        let old = dir.join(".dotfiles.old");
        fs::remove_file(&old).ok();
        fs::rename(path, &old).context("backing up current binary to .old")?;
    }

    fs::rename(tmp.path(), path).context("moving new binary into place")?;
    tmp.persist();
    Ok(())
}

/// Stage an update for later promotion by the wrapper.
#[cfg(windows)]
pub(super) fn stage_binary(root: &Path, tag: &str, data: &[u8]) -> Result<()> {
    let pending = pending_binary_path(root);
    let dir = pending
        .parent()
        .ok_or_else(|| anyhow::anyhow!("pending binary path has no parent directory"))?;
    fs::create_dir_all(dir).context("creating bin directory")?;

    let tmp_path = dir.join(".dotfiles-update.tmp");
    let mut tmp = crate::fs::TempPath::new(tmp_path);
    {
        let mut f = fs::File::create(tmp.path()).context("creating temp staged file")?;
        f.write_all(data).context("writing staged binary data")?;
        f.flush().context("flushing staged binary data")?;
    }

    if pending.exists() {
        fs::remove_file(&pending).context("removing previous staged binary")?;
    }
    fs::rename(tmp.path(), &pending).context("moving staged binary into place")?;
    tmp.persist();
    fs::write(pending_version_path(root), format!("{tag}\n"))
        .context("writing staged update metadata")?;
    Ok(())
}

/// Run the binary at `path` with the `version` subcommand as a basic sanity
/// check.
///
/// Called immediately after a self-update to verify that the new binary
/// starts correctly.  On failure the caller is expected to restore the
/// backup created by [`replace_binary`].
///
/// On Linux, `exec` can transiently fail with `ETXTBSY` ("Text file busy")
/// right after writing a binary when the kernel hasn't fully released the
/// inode.  This is a known race on certain CI filesystems (e.g. overlayfs).
/// The function retries a few times with a short sleep to work around it.
///
/// # Errors
///
/// Returns an error if the process cannot be spawned or exits with a
/// non-zero status code.
#[cfg(not(windows))]
pub(super) fn smoke_test_binary(path: &Path) -> Result<()> {
    const MAX_RETRIES: u32 = 5;
    const BASE_DELAY_MS: u64 = 50;

    let mut attempts = 0;
    let output = loop {
        match std::process::Command::new(path).arg("version").output() {
            Ok(output) => break output,
            Err(e) if e.kind() == std::io::ErrorKind::ResourceBusy && attempts < MAX_RETRIES => {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_millis(
                    BASE_DELAY_MS * u64::from(attempts),
                ));
            }
            Err(e) => {
                return Err(anyhow::Error::from(e))
                    .with_context(|| format!("spawning smoke test for {}", path.display()));
            }
        }
    };

    if output.status.success() {
        Ok(())
    } else {
        bail!(
            "new binary failed smoke test (exit {:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        )
    }
}

/// Download the release asset for the given tag and install it.
///
/// # Errors
///
/// Returns an error if the download, checksum verification, binary
/// replacement, or smoke test fails.  On a smoke-test failure the previous
/// binary is restored from the `.dotfiles.old` backup.
pub(super) fn download_and_install(root: &Path, tag: &str, client: &dyn HttpClient) -> Result<()> {
    let asset = asset_name();
    let url = format!(
        "https://github.com/{repo}/releases/download/{tag}/{asset}",
        repo = super::REPO
    );
    let data = download_bytes(client, &url)?;
    verify_checksum(client, tag, asset, &data)?;

    #[cfg(windows)]
    {
        stage_binary(root, tag, &data)?;
        write_cache(root, tag)?;
    }

    #[cfg(not(windows))]
    {
        let bin = binary_path(root);
        replace_binary(&bin, &data)?;
        if let Err(smoke_err) = smoke_test_binary(&bin) {
            let old = old_binary_path(root);
            if old.exists()
                && let Err(restore_err) = fs::rename(&old, &bin)
            {
                tracing::warn!(
                    "CRITICAL: smoke-test failed and automatic rollback also failed ({restore_err:#}). \
                     Manual intervention required: restore {old:?} to {bin:?}"
                );
            }
            return Err(smoke_err);
        }
        write_cache(root, tag)?;
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    use super::super::cache::cache_path;
    use super::super::http::test_support::MockHttpClient;

    #[test]
    fn replace_binary_writes_and_sets_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("dotfiles");
        replace_binary(&bin, b"#!/bin/sh\necho ok").unwrap();
        assert!(bin.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&bin).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o755);
        }
    }

    #[test]
    fn replace_binary_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("dotfiles");
        fs::write(&bin, b"old").unwrap();
        replace_binary(&bin, b"new").unwrap();
        assert_eq!(fs::read(&bin).unwrap(), b"new");
    }

    #[test]
    fn replace_binary_cleans_up_temp_file_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("dotfiles");
        fs::create_dir(&bin).unwrap();

        let result = replace_binary(&bin, b"new");
        assert!(result.is_err());
        assert!(!dir.path().join(".dotfiles-update.tmp").exists());
    }

    #[cfg(windows)]
    #[test]
    fn stage_binary_writes_pending_files() {
        let dir = tempfile::tempdir().unwrap();

        stage_binary(dir.path(), "v1.2.3", b"new-binary").unwrap();

        assert_eq!(
            fs::read(pending_binary_path(dir.path())).unwrap(),
            b"new-binary"
        );
        assert_eq!(
            fs::read_to_string(pending_version_path(dir.path())).unwrap(),
            "v1.2.3\n"
        );
    }

    #[test]
    fn download_and_install_writes_verified_binary() {
        let dir = tempfile::tempdir_in(
            std::env::current_dir().expect("failed to get current working directory"),
        )
        .expect("failed to create temporary directory in current working directory");
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        // On Unix, download_and_install calls smoke_test_binary after writing
        // the binary.  Use the system `true` binary so the smoke test passes
        // on CI runners where the workspace filesystem restricts shebang
        // interpreter execution.
        #[cfg(unix)]
        let binary_data: Vec<u8> = {
            let true_path = which::which("true").expect("'true' binary not found on PATH");
            fs::read(&true_path).expect("reading 'true' binary")
        };
        #[cfg(not(unix))]
        let binary_data: Vec<u8> = b"#!/bin/sh\necho updated".to_vec();

        let mut hasher = Sha256::new();
        hasher.update(&binary_data);
        let hash = format!("{:x}", hasher.finalize());
        let checksums = format!("{hash}  {}\n", asset_name());

        let client = MockHttpClient::new(vec![Ok(binary_data.clone()), Ok(checksums.into_bytes())]);

        download_and_install(dir.path(), "v1.0.0", &client).unwrap();

        #[cfg(windows)]
        {
            let staged = fs::read(pending_binary_path(dir.path())).unwrap();
            assert_eq!(staged, binary_data);
            assert_eq!(
                fs::read_to_string(pending_version_path(dir.path())).unwrap(),
                "v1.0.0\n"
            );
            let cache = fs::read_to_string(cache_path(dir.path())).unwrap();
            assert!(cache.starts_with("v1.0.0\n"));
        }

        #[cfg(not(windows))]
        {
            let installed = fs::read(binary_path(dir.path())).unwrap();
            assert_eq!(installed, binary_data);
            let cache = fs::read_to_string(cache_path(dir.path())).unwrap();
            assert!(cache.starts_with("v1.0.0\n"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn replace_binary_backs_up_existing_on_unix() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("dotfiles");
        fs::write(&bin, b"old-content").unwrap();

        replace_binary(&bin, b"new-content").unwrap();

        assert_eq!(fs::read(&bin).unwrap(), b"new-content");
        assert_eq!(
            fs::read(dir.path().join(".dotfiles.old")).unwrap(),
            b"old-content"
        );
    }

    #[cfg(unix)]
    #[test]
    fn smoke_test_binary_passes_for_valid_binary() {
        let true_path = which::which("true").expect("'true' binary not found on PATH");

        let result = smoke_test_binary(&true_path);
        assert!(
            result.is_ok(),
            "binary: {}, error: {:?}",
            true_path.display(),
            result.unwrap_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn smoke_test_binary_fails_for_bad_binary() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bad");
        fs::write(&bin, b"#!/bin/sh\nexit 1\n").unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();

        let result = smoke_test_binary(&bin);
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("smoke test"),
            "expected 'smoke test' in: {msg}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn download_and_install_restores_on_smoke_test_failure() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir_in(
            std::env::current_dir().expect("failed to get current working directory"),
        )
        .expect("failed to create temporary directory in current working directory");
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let old_binary = b"#!/bin/sh\necho v0.9.0\n";
        let bin = binary_path(dir.path());
        fs::write(&bin, old_binary).unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();

        let bad_binary = b"#!/bin/sh\nexit 1\n";
        let mut hasher = Sha256::new();
        hasher.update(bad_binary);
        let hash = format!("{:x}", hasher.finalize());
        let checksums = format!("{hash}  {}\n", asset_name());

        let client = MockHttpClient::new(vec![Ok(bad_binary.to_vec()), Ok(checksums.into_bytes())]);

        let result = download_and_install(dir.path(), "v1.0.0", &client);
        assert!(result.is_err(), "expected smoke-test failure");

        let restored = fs::read(&bin).unwrap();
        assert_eq!(
            restored, old_binary,
            "old binary was not restored after smoke-test failure"
        );

        assert!(
            !cache_path(dir.path()).exists(),
            "cache should not be written after a failed update"
        );
    }
}
