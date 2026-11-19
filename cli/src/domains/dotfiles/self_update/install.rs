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
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::io::Write as _;

    use super::super::cache::cache_path;
    use super::super::http::test_support::MockHttpClient;

    // Synthetic images contain the stdout their --version invocation would return.
    fn download_and_install_with_gh(
        root: &Path,
        tag: &str,
        client: &dyn HttpClient,
        gh: &dyn GhCli,
        policy: Policy,
    ) -> Result<()> {
        install_release(root, tag, client, gh, policy, |path| {
            Ok(fs::read_to_string(path)?)
        })
    }

    fn replace_binary(path: &Path, data: &[u8]) -> Result<()> {
        let dir = path.parent().unwrap();
        fs::create_dir_all(dir)?;
        let (mut staged, mut file) =
            crate::infra::fs::TempGuard::create_unique_file(dir, ".fixture", "tmp")?;
        file.write_all(data)?;
        drop(file);
        #[cfg(unix)]
        crate::infra::fs::set_executable(staged.path())?;
        super::replace_binary(path, staged.path())?;
        staged.persist();
        Ok(())
    }

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
                super::super::attestation::Verification::Unverified("not verified".to_string())
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
        #[cfg(unix)]
        let original = fs::metadata(&bin).unwrap();

        let error = install_staged_binary(&bin, &missing_staged).unwrap_err();

        assert!(
            error.to_string().contains("moving new binary into place"),
            "unexpected replacement error: {error:#}"
        );
        assert_eq!(fs::read(&bin).unwrap(), b"old");
        assert!(!dir.path().join(old_binary_name()).exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;
            assert_eq!(fs::metadata(&bin).unwrap().ino(), original.ino());
        }
    }

    #[test]
    fn backup_failure_preserves_installed_and_staged_binaries() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("dotfiles");
        let staged = dir.path().join("staged");
        fs::write(&bin, b"old").unwrap();
        fs::write(&staged, b"new").unwrap();
        let occupied_backup = dir.path().join(old_binary_name());
        fs::create_dir(&occupied_backup).unwrap();
        fs::write(occupied_backup.join("unrelated"), b"keep").unwrap();

        let error = install_staged_binary(&bin, &staged).unwrap_err();

        assert!(error.to_string().contains("removing stale backup"));
        assert_eq!(fs::read(&bin).unwrap(), b"old");
        assert_eq!(fs::read(&staged).unwrap(), b"new");
        assert_eq!(
            fs::read(occupied_backup.join("unrelated")).unwrap(),
            b"keep"
        );
    }

    #[cfg(unix)]
    #[test]
    fn interrupted_unix_install_keeps_a_runnable_binary_and_can_retry() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        for phase in ["backup", "replace"] {
            let dir = tempfile::tempdir().unwrap();
            let bin = dir.path().join("dotfiles");
            let staged = dir.path().join("staged");
            let old = b"#!/bin/sh\n# old\nexit 0\n";
            let new = b"#!/bin/sh\n# new\nexit 0\n";
            fs::write(&bin, old).unwrap();
            fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
            fs::write(&staged, new).unwrap();
            crate::infra::fs::set_executable(&staged).unwrap();
            let original_inode = fs::metadata(&bin).unwrap().ino();

            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "domains::dotfiles::self_update::install::tests::interrupted_install_child",
                    "--ignored",
                    "--nocapture",
                ])
                .env("DOTFILES_TEST_UPDATE_ROOT", dir.path())
                .env("DOTFILES_TEST_UPDATE_PHASE", phase)
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(86), "{output:?}");
            let backup = dir.path().join(old_binary_name());
            assert_eq!(fs::read(&backup).unwrap(), old);
            assert_eq!(fs::metadata(&backup).unwrap().ino(), original_inode);
            assert_eq!(
                fs::read(&bin).unwrap(),
                if phase == "backup" { old } else { new }
            );
            smoke_test_binary(&bin).unwrap();

            // A fresh attempt must replace any interrupted attempt's backup
            // and retain the executable that was installed when retry began.
            let before_retry = fs::read(&bin).unwrap();
            fs::write(&staged, new).unwrap();
            crate::infra::fs::set_executable(&staged).unwrap();
            super::replace_binary(&bin, &staged).unwrap();
            assert_eq!(fs::read(&bin).unwrap(), new);
            assert_eq!(fs::read(&backup).unwrap(), before_retry);
            assert!(!staged.exists());
            smoke_test_binary(&bin).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "subprocess fixture launched by the interrupted self-update test"]
    fn interrupted_install_child() {
        let root = PathBuf::from(std::env::var_os("DOTFILES_TEST_UPDATE_ROOT").unwrap());
        let bin = root.join("dotfiles");
        match std::env::var("DOTFILES_TEST_UPDATE_PHASE")
            .unwrap()
            .as_str()
        {
            "backup" => assert!(backup_current_binary(&bin).unwrap().is_some()),
            "replace" => super::replace_binary(&bin, &root.join("staged")).unwrap(),
            phase => panic!("unexpected interrupted update phase: {phase}"),
        }
        // Exit without unwinding, so no caller rollback or guard cleanup runs.
        std::process::exit(86);
    }

    #[test]
    fn replace_binary_cleans_up_temp_file_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("dotfiles");
        fs::create_dir(&bin).unwrap();

        let result = replace_binary(&bin, b"new");
        assert!(result.is_err());
        let remaining: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(
            remaining,
            [std::ffi::OsString::from("dotfiles")],
            "unique staging files must be removed after replacement fails"
        );
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

    /// A runnable fixture for the startup/recovery tests (not a release asset).
    fn version_capable_binary() -> Vec<u8> {
        #[cfg(unix)]
        {
            b"#!/bin/sh\n[ \"$#:$1\" = 1:--version ] || exit 42\nprintf 'dotfiles fixture\\n'\n"
                .to_vec()
        }
        // Prefer the System32 copy explicitly: a `curl.exe` earlier on PATH
        // (for example the one shipped with Git for Windows) loads DLLs from
        // its own directory and would not run once copied elsewhere.
        #[cfg(windows)]
        {
            let system_root =
                std::env::var_os("SystemRoot").expect("SystemRoot is not set on this system");
            fs::read(Path::new(&system_root).join("System32").join("curl.exe")).unwrap()
        }
    }

    #[cfg(unix)]
    #[test]
    fn wrong_version_preserves_last_good_state_and_corrected_release_can_retry() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let bin = binary_path(dir.path());
        fs::create_dir(bin.parent().unwrap()).unwrap();
        fs::write(&bin, b"last good").unwrap();
        let old = old_binary_path(dir.path());
        fs::write(&old, b"earlier backup").unwrap();
        let cache = cache_path(dir.path());
        fs::write(&cache, b"old cache").unwrap();
        let tag = "v2026.11.19-2";
        for output in ["dotfiles v2026.11.19-1", "dotfiles v2026.11.19-2"] {
            let bytes =
                format!("#!/bin/sh\n[ \"$#:$1\" = 1:--version ] || exit 42\necho '{output}'\n");
            let hash = super::super::hex_encode(&Sha256::digest(bytes.as_bytes()));
            let client = MockHttpClient::new(vec![
                Ok(format!("{hash}  {}\n", asset_name()).into_bytes()),
                Ok(bytes.clone().into_bytes()),
            ]);
            let result = install_release(
                dir.path(),
                tag,
                &client,
                &StubGh { verified: true },
                Policy::Required,
                smoke_test_binary,
            );
            if output.ends_with("-1") {
                assert!(format!("{:#}", result.unwrap_err()).contains("expected release"));
                assert_eq!(fs::read(&bin).unwrap(), b"last good");
                assert_eq!(fs::read(&old).unwrap(), b"earlier backup");
                assert_eq!(fs::read(&cache).unwrap(), b"old cache");
                assert_eq!(fs::read_dir(bin.parent().unwrap()).unwrap().count(), 3);
            } else {
                result.unwrap();
                assert_eq!(fs::read(&bin).unwrap(), bytes.as_bytes());
                assert_eq!(fs::read(&old).unwrap(), b"last good");
                assert!(
                    fs::read_to_string(&cache)
                        .unwrap()
                        .lines()
                        .any(|line| line == tag)
                );
            }
        }
    }

    #[test]
    fn download_and_install_writes_verified_binary() {
        #[derive(Debug)]
        struct RecordingGh {
            directory: PathBuf,
            bytes: Vec<u8>,
            path: std::sync::Mutex<Option<PathBuf>>,
        }
        impl GhCli for RecordingGh {
            fn available(&self) -> bool {
                true
            }
            fn verify(
                &self,
                path: &Path,
                _: &str,
            ) -> Result<super::super::attestation::Verification> {
                assert_eq!(
                    path.parent(),
                    Some(self.directory.as_path()),
                    "verify the installation staging file, not a second temporary copy"
                );
                assert_eq!(fs::read(path).unwrap(), self.bytes);
                *self.path.lock().unwrap() = Some(path.to_path_buf());
                Ok(super::super::attestation::Verification::Verified)
            }
        }
        let dir = tempfile::tempdir_in(
            std::env::current_dir().expect("failed to get current working directory"),
        )
        .expect("failed to create temporary directory in current working directory");
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let binary_data = b"dotfiles v1.0.0\n".to_vec();

        let mut hasher = Sha256::new();
        hasher.update(&binary_data);
        let hash = super::super::hex_encode(&hasher.finalize());
        let checksums = format!("{hash}  {}\n", asset_name());

        let client = MockHttpClient::new(vec![Ok(checksums.into_bytes()), Ok(binary_data.clone())]);
        let gh = RecordingGh {
            directory: bin_dir.clone(),
            bytes: binary_data.clone(),
            path: std::sync::Mutex::new(None),
        };

        download_and_install_with_gh(dir.path(), "v1.0.0", &client, &gh, Policy::Required).unwrap();

        let installed = fs::read(binary_path(dir.path())).unwrap();
        assert_eq!(installed, binary_data);
        let verified_path = gh.path.into_inner().unwrap().unwrap();
        assert!(
            !verified_path.exists(),
            "the verified staging file must be moved into place"
        );
        assert_eq!(
            fs::read_dir(&bin_dir).unwrap().count(),
            3,
            "only the binary, clean update journal and version cache remain"
        );
        assert_eq!(
            fs::metadata(bin_dir.join(format!(
            ".{}.update-state", binary_path(dir.path()).file_name().unwrap().to_str().unwrap()
        )))
            .unwrap()
            .len(),
            0
        );
        let cache = fs::read_to_string(cache_path(dir.path())).unwrap();
        assert!(cache.starts_with(&format!("{}\nv1.0.0\n", super::super::REPO)));
        assert_eq!(
            client.request_urls(),
            [
                format!(
                    "https://github.com/{}/releases/download/v1.0.0/checksums.sha256",
                    super::super::REPO
                ),
                format!(
                    "https://github.com/{}/releases/download/v1.0.0/{}",
                    super::super::REPO,
                    asset_name()
                ),
            ]
        );
    }

    #[test]
    fn a_failed_body_read_cleans_the_stage_and_preserves_the_installed_binary() {
        #[derive(Debug)]
        struct FailingHttp;
        impl HttpClient for FailingHttp {
            fn get(&self, url: &str, _: &[(&str, &str)]) -> Result<Box<dyn std::io::Read>> {
                struct BrokenBody {
                    failed: bool,
                }
                impl std::io::Read for BrokenBody {
                    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                        assert!(
                            !self.failed,
                            "a permanent body error must terminate the read loop"
                        );
                        self.failed = true;
                        Err(std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            "fixture body failure",
                        ))
                    }
                }
                if url.ends_with("/checksums.sha256") {
                    return Ok(Box::new(std::io::Cursor::new(format!(
                        "{}  {}\n",
                        "0".repeat(64),
                        asset_name()
                    ))));
                }
                Ok(Box::new(BrokenBody { failed: false }))
            }
        }
        let root = tempfile::tempdir().unwrap();
        let bin = binary_path(root.path());
        fs::create_dir_all(bin.parent().unwrap()).unwrap();
        fs::write(&bin, b"original").unwrap();
        let error = download_and_install_with_gh(
            root.path(),
            "v1",
            &FailingHttp,
            &StubGh { verified: true },
            Policy::Required,
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("fixture body failure"));
        assert_eq!(fs::read(&bin).unwrap(), b"original");
        assert!(!cache_path(root.path()).exists());
        assert_eq!(fs::read_dir(bin.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn cache_write_failure_does_not_hide_a_successful_binary_update() {
        let dir = tempfile::tempdir_in(".").unwrap();
        fs::create_dir_all(cache_path(dir.path())).unwrap();
        let bin = binary_path(dir.path());
        fs::write(&bin, b"previous binary").unwrap();
        let data = b"dotfiles v2026.07.25-1\n".to_vec();
        let hash = super::super::hex_encode(&Sha256::digest(&data));
        let checksums = format!("{hash}  {}\n", asset_name());
        let client = MockHttpClient::new(vec![Ok(checksums.into_bytes()), Ok(data.clone())]);

        download_and_install_with_gh(
            dir.path(),
            "v2026.07.25-1",
            &client,
            &StubGh { verified: true },
            Policy::Required,
        )
        .expect("a verified, runnable update must succeed so the caller can restart");

        assert_eq!(fs::read(&bin).unwrap(), data);
        assert_eq!(
            fs::read(old_binary_path(dir.path())).unwrap(),
            b"previous binary"
        );
        assert!(
            cache_path(dir.path()).is_dir(),
            "preserve conflicting cache state"
        );
    }

    #[test]
    fn checksum_failure_does_not_attempt_provenance_or_replace_existing_state() {
        #[derive(Debug)]
        struct ForbiddenGh;
        impl GhCli for ForbiddenGh {
            fn available(&self) -> bool {
                panic!("checksum must be verified before probing provenance");
            }
            fn verify(&self, _: &Path, _: &str) -> Result<super::super::attestation::Verification> {
                panic!("checksum must be verified before invoking provenance");
            }
        }

        let root = tempfile::tempdir_in(".").unwrap();
        let bin = binary_path(root.path());
        fs::create_dir_all(bin.parent().unwrap()).unwrap();
        fs::write(&bin, b"existing binary").unwrap();
        let cache = cache_path(root.path());
        fs::write(&cache, "existing cache").unwrap();
        let client = MockHttpClient::new(vec![
            Ok(format!("{}  {}\n", "0".repeat(64), asset_name()).into_bytes()),
            Ok(b"untrusted binary".to_vec()),
        ]);

        let error = download_and_install_with_gh(
            root.path(),
            "v2026.09.28.1",
            &client,
            &ForbiddenGh,
            Policy::Required,
        )
        .unwrap_err();

        assert!(
            format!("{error:#}").contains("checksum mismatch"),
            "{error:#}"
        );
        assert_eq!(fs::read(&bin).unwrap(), b"existing binary");
        assert_eq!(fs::read_to_string(&cache).unwrap(), "existing cache");
        assert!(!old_binary_path(root.path()).exists());
        assert_eq!(client.request_urls().len(), 2);
        assert_eq!(
            fs::read_dir(bin.parent().unwrap()).unwrap().count(),
            2,
            "failed verification must clean the download staging file"
        );
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
            MockHttpClient::new(vec![Ok(checksums.into_bytes()), Ok(binary_data.to_vec())]);

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
        assert_eq!(
            fs::read_dir(&bin_dir).unwrap().count(),
            1,
            "rejected downloads must not leak staging files"
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
    fn checkout_wrapper_recovers_only_unlocked_unfinished_updates() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let good = version_capable_binary();
        #[cfg(unix)]
        let bad = b"#!/bin/sh\nexit 1\n".to_vec();
        #[cfg(windows)]
        let bad = fs::read(
            PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/where.exe"),
        )
        .unwrap();
        for wrapper in ["dotfiles.sh", "dotfiles.ps1"] {
            let shell = if wrapper == "dotfiles.sh" {
                if cfg!(windows) {
                    continue;
                }
                which::which("sh").unwrap()
            } else if let Ok(pwsh) = which::which("pwsh") {
                pwsh
            } else if cfg!(windows) {
                which::which("powershell").unwrap()
            } else {
                continue;
            };
            for phase in ["missing", "failed", "healthy", "both-failed", "legacy"] {
                let root = tempfile::tempdir_in(".").unwrap();
                let bin = binary_path(root.path());
                let old = old_binary_path(root.path());
                fs::create_dir(bin.parent().unwrap()).unwrap();
                let wrapper_path = root.path().join(wrapper);
                fs::copy(repo.join(wrapper), &wrapper_path).unwrap();
                let write_image = |path, data| {
                    fs::write(path, data).unwrap();
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt as _;
                        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
                    }
                };
                write_image(
                    &old,
                    if matches!(phase, "healthy" | "both-failed") {
                        &bad
                    } else {
                        &good
                    },
                );
                if !matches!(phase, "missing" | "legacy") {
                    write_image(&bin, if phase == "healthy" { &good } else { &bad });
                }
                let run = || {
                    let mut command = std::process::Command::new(&shell);
                    if wrapper == "dotfiles.ps1" {
                        command.args(["-NoProfile", "-NonInteractive", "-File"]);
                    }
                    command
                        .arg(&wrapper_path)
                        .arg("--version")
                        .output()
                        .unwrap()
                };
                if phase != "legacy" {
                    let active = begin_replacement(&bin).unwrap();
                    if phase == "missing" {
                        let output = run();
                        assert!(
                            !output.status.success(),
                            "{wrapper} ignored update lock: {output:?}"
                        );
                        assert!(!bin.exists());
                        assert_eq!(fs::read(&old).unwrap(), good);
                    }
                    drop(active); // process exit without confirming the installation
                }
                let output = run();
                assert_eq!(
                    output.status.success(),
                    phase != "both-failed",
                    "{wrapper} {phase}: {output:?}"
                );
                match phase {
                    "legacy" => {
                        assert!(!bin.exists());
                        assert_eq!(fs::read(old).unwrap(), good);
                    }
                    "both-failed" => {
                        assert_eq!(fs::read(&bin).unwrap(), bad);
                        assert_eq!(fs::read(old).unwrap(), bad);
                        assert!(begin_replacement(&bin).is_err());
                    }
                    _ => {
                        assert_eq!(fs::read(&bin).unwrap(), good);
                        assert_eq!(old.exists(), phase == "healthy");
                        begin_replacement(&bin).unwrap(); // next update no longer requires recovery
                    }
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn powershell_recovery_restores_backup_when_cached_binary_is_not_executable() {
        use std::os::unix::fs::PermissionsExt as _;

        let Ok(pwsh) = which::which("pwsh") else {
            return;
        };
        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let bin = binary_path(&root);
        let backup = old_binary_path(&root);
        fs::create_dir(bin.parent().unwrap()).unwrap();
        fs::write(&bin, b"#!/bin/sh\nexit 9\n").unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o644)).unwrap();
        let healthy = b"#!/bin/sh\necho 'dotfiles fixture'\n";
        fs::write(&backup, healthy).unwrap();
        fs::set_permissions(&backup, fs::Permissions::from_mode(0o755)).unwrap();
        let wrapper = root.join("dotfiles.ps1");
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        fs::copy(repo.join("dotfiles.ps1"), &wrapper).unwrap();
        let opener = root.join("xdg-open");
        fs::write(
            &opener,
            b"#!/bin/sh\n: > \"$DOTFILES_TEST_OPENER_MARKER\"\n",
        )
        .unwrap();
        fs::set_permissions(opener, fs::Permissions::from_mode(0o755)).unwrap();
        let opener_marker = root.join("opener-called");
        let path = std::env::join_paths(
            std::iter::once(root).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        drop(begin_replacement(&bin).unwrap());

        let output = std::process::Command::new(pwsh)
            .args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg("$global:LASTEXITCODE = 0; & $env:DOTFILES_TEST_WRAPPER --version")
            .env("PATH", path)
            .env("DOTFILES_TEST_WRAPPER", wrapper)
            .env("DOTFILES_TEST_OPENER_MARKER", &opener_marker)
            .env("DOTFILES_RELEASE_REPO", "invalid")
            .output()
            .unwrap();

        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"dotfiles fixture\n");
        assert_eq!(fs::read(&bin).unwrap(), healthy);
        assert!(!backup.exists());
        assert!(
            !opener_marker.exists(),
            "startup must never open a document"
        );
        begin_replacement(&bin).unwrap(); // the recovered journal is clean
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

        let bad_binary = b"dotfiles v1.0.0\n";
        let mut hasher = Sha256::new();
        hasher.update(bad_binary);
        let hash = super::super::hex_encode(&hasher.finalize());
        let checksums = format!("{hash}  {}\n", asset_name());

        let client = MockHttpClient::new(vec![Ok(checksums.into_bytes()), Ok(bad_binary.to_vec())]);

        let result = install_release(
            dir.path(),
            "v1.0.0",
            &client,
            &StubGh { verified: true },
            Policy::Required,
            |path| {
                if path == bin {
                    anyhow::bail!("installed smoke failed");
                }
                Ok(fs::read_to_string(path)?)
            },
        );
        assert!(format!("{:#}", result.unwrap_err()).contains("installed smoke failed"));

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
