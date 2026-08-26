//! Filesystem operations for installing an updated binary, plus the
//! post-install smoke test and end-to-end download orchestration.

use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{Context as _, Result, bail};

use super::attestation::{GhCli, Policy, SystemGh, policy_from_env, verify_provenance};
use super::cache::write_cache;
use super::http::{HttpClient, download_bytes, verify_checksum};
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

fn install_staged_binary(path: &Path, staged: &Path) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("binary path has no parent directory"))?;
    let backup = if path.exists() {
        let old = dir.join(old_binary_name());
        remove_stale_backup(&old)?;
        fs::rename(path, &old).context("backing up current binary")?;
        Some(old)
    } else {
        None
    };

    if let Err(install_error) = fs::rename(staged, path) {
        if let Some(old) = backup
            && let Err(restore_error) = fs::rename(&old, path)
        {
            return Err(anyhow::anyhow!(
                "moving new binary into place failed ({install_error}); \
                 restoring previous binary also failed ({restore_error})"
            ));
        }
        return Err(install_error).context("moving new binary into place");
    }
    Ok(())
}

/// Replace the binary at `path` with `data`, handling platform differences.
///
/// The current binary is renamed to [`old_binary_name`] first rather than
/// deleted, so the caller can restore it when the post-install smoke test
/// fails.  Windows blocks *deleting* a running executable but still allows
/// *renaming* it, which is what makes the same in-place update work there.
pub(super) fn replace_binary(path: &Path, data: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("binary path has no parent directory"))?;
    fs::create_dir_all(dir).context("creating bin directory")?;

    // A unique staging name keeps two concurrent updates from clobbering each
    // other: with a fixed path, one run's `TempGuard` deletes the partially
    // written binary the other run is about to rename into place.
    let mut tmp = crate::infra::fs::TempGuard::unique_file(dir, ".dotfiles-update", "tmp");

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

    install_staged_binary(path, tmp.path())?;
    tmp.persist();
    Ok(())
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
pub(super) fn smoke_test_binary(path: &Path) -> Result<()> {
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
        Ok(())
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
pub(super) fn download_and_install(root: &Path, tag: &str, client: &dyn HttpClient) -> Result<()> {
    download_and_install_with_gh(root, tag, client, &SystemGh, policy_from_env())
}

fn download_and_install_with_gh(
    root: &Path,
    tag: &str,
    client: &dyn HttpClient,
    gh: &dyn GhCli,
    policy: Policy,
) -> Result<()> {
    let asset = asset_name();
    let url = format!(
        "https://github.com/{repo}/releases/download/{tag}/{asset}",
        repo = super::REPO
    );
    let data = download_bytes(client, &url)?;
    verify_checksum(client, tag, asset, &data)?;
    verify_provenance(gh, policy, asset, &data)?;

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

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    use super::super::cache::cache_path;
    use super::super::http::test_support::MockHttpClient;

    #[derive(Debug)]
    struct StubGh {
        verified: bool,
    }

    impl GhCli for StubGh {
        fn available(&self) -> bool {
            true
        }

        fn verify(
            &self,
            _path: &Path,
            _repo: &str,
        ) -> Result<super::super::attestation::Verification> {
            Ok(if self.verified {
                super::super::attestation::Verification::Verified
            } else {
                super::super::attestation::Verification::Unverified
            })
        }
    }

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
    fn failed_staged_install_restores_existing_binary() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("dotfiles");
        let missing_staged = dir.path().join("missing-staged-binary");
        fs::write(&bin, b"old").unwrap();

        let error = install_staged_binary(&bin, &missing_staged).unwrap_err();

        assert!(
            error.to_string().contains("moving new binary into place"),
            "unexpected replacement error: {error:#}"
        );
        assert_eq!(fs::read(&bin).unwrap(), b"old");
        assert!(!dir.path().join(old_binary_name()).exists());
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
    fn replace_binary_backs_up_existing_as_a_runnable_image() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("dotfiles.exe");
        fs::write(&bin, b"old-content").unwrap();

        replace_binary(&bin, b"new-content").unwrap();

        assert_eq!(fs::read(&bin).unwrap(), b"new-content");
        assert_eq!(
            fs::read(dir.path().join(".dotfiles-old.exe")).unwrap(),
            b"old-content"
        );
    }

    /// A real, self-contained executable that exits 0 for `--version`.
    ///
    /// `download_and_install` smoke-tests the binary it just wrote, so the
    /// fixture has to be something the OS will actually execute from an
    /// arbitrary directory.
    fn version_capable_binary() -> Vec<u8> {
        #[cfg(unix)]
        let path = which::which("true").expect("'true' binary not found on PATH");
        // Prefer the System32 copy explicitly: a `curl.exe` earlier on PATH
        // (for example the one shipped with Git for Windows) loads DLLs from
        // its own directory and would not run once copied elsewhere.
        #[cfg(windows)]
        let path = {
            let system_root =
                std::env::var_os("SystemRoot").expect("SystemRoot is not set on this system");
            Path::new(&system_root).join("System32").join("curl.exe")
        };
        fs::read(&path).unwrap_or_else(|error| {
            panic!("reading smoke-test fixture {}: {error}", path.display())
        })
    }

    #[test]
    fn download_and_install_writes_verified_binary() {
        let dir = tempfile::tempdir_in(
            std::env::current_dir().expect("failed to get current working directory"),
        )
        .expect("failed to create temporary directory in current working directory");
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        // `download_and_install` smoke-tests the binary after writing it, so a
        // shell script is not enough: CI runners may restrict shebang
        // interpreter execution on the workspace filesystem, and Windows will
        // not execute it at all.
        let binary_data = version_capable_binary();

        let mut hasher = Sha256::new();
        hasher.update(&binary_data);
        let hash = super::super::hex_encode(&hasher.finalize());
        let checksums = format!("{hash}  {}\n", asset_name());

        let client = MockHttpClient::new(vec![Ok(binary_data.clone()), Ok(checksums.into_bytes())]);

        download_and_install_with_gh(
            dir.path(),
            "v1.0.0",
            &client,
            &StubGh { verified: true },
            Policy::Required,
        )
        .unwrap();

        let installed = fs::read(binary_path(dir.path())).unwrap();
        assert_eq!(installed, binary_data);
        let cache = fs::read_to_string(cache_path(dir.path())).unwrap();
        assert!(cache.starts_with("v1.0.0\n"));
    }

    #[test]
    fn unverified_download_leaves_existing_binary_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let bin = binary_path(dir.path());
        fs::write(&bin, b"old-content").unwrap();

        let binary_data = b"new-content";
        let mut hasher = Sha256::new();
        hasher.update(binary_data);
        let hash = super::super::hex_encode(&hasher.finalize());
        let checksums = format!("{hash}  {}\n", asset_name());
        let client =
            MockHttpClient::new(vec![Ok(binary_data.to_vec()), Ok(checksums.into_bytes())]);

        let error = download_and_install_with_gh(
            dir.path(),
            "v1.0.0",
            &client,
            &StubGh { verified: false },
            Policy::Required,
        )
        .unwrap_err()
        .to_string();

        assert!(
            error.contains("build provenance verification failed"),
            "unexpected error: {error}"
        );
        assert_eq!(fs::read(&bin).unwrap(), b"old-content");
        assert!(
            !cache_path(dir.path()).exists(),
            "cache should not be written after rejected provenance"
        );
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

    #[test]
    fn smoke_test_binary_uses_supported_version_flag() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join(if cfg!(windows) {
            "version-check.exe"
        } else {
            "version-check"
        });
        replace_binary(&bin, &version_capable_binary()).unwrap();

        let result = smoke_test_binary(&bin);
        assert!(
            result.is_ok(),
            "binary: {}, error: {:?}",
            bin.display(),
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

    #[cfg(windows)]
    #[test]
    fn smoke_test_binary_fails_for_bad_binary() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bad.exe");
        fs::write(&bin, b"not a portable executable").unwrap();

        let result = smoke_test_binary(&bin);
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("smoke test"),
            "expected 'smoke test' in: {msg}"
        );
    }

    #[test]
    fn download_and_install_restores_on_smoke_test_failure() {
        let dir = tempfile::tempdir_in(
            std::env::current_dir().expect("failed to get current working directory"),
        )
        .expect("failed to create temporary directory in current working directory");
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let old_binary = b"#!/bin/sh\necho v0.9.0\n";
        let bin = binary_path(dir.path());
        fs::write(&bin, old_binary).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
        }

        // Not executable on any platform: Unix runs it and it exits 1, Windows
        // refuses to spawn it.  Either way the smoke test must fail.
        let bad_binary = b"#!/bin/sh\nexit 1\n";
        let mut hasher = Sha256::new();
        hasher.update(bad_binary);
        let hash = super::super::hex_encode(&hasher.finalize());
        let checksums = format!("{hash}  {}\n", asset_name());

        let client = MockHttpClient::new(vec![Ok(bad_binary.to_vec()), Ok(checksums.into_bytes())]);

        let result = download_and_install_with_gh(
            dir.path(),
            "v1.0.0",
            &client,
            &StubGh { verified: true },
            Policy::Required,
        );
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
