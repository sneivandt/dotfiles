use super::*;

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
        let bytes = format!("#!/bin/sh\n[ \"$#:$1\" = 1:--version ] || exit 42\necho '{output}'\n");
        let hash = super::super::super::hex_encode(&Sha256::digest(bytes.as_bytes()));
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
        ) -> Result<super::super::super::attestation::Verification> {
            assert_eq!(
                path.parent(),
                Some(self.directory.as_path()),
                "verify the installation staging file, not a second temporary copy"
            );
            assert_eq!(fs::read(path).unwrap(), self.bytes);
            *self.path.lock().unwrap() = Some(path.to_path_buf());
            Ok(super::super::super::attestation::Verification::Verified)
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
    let hash = super::super::super::hex_encode(&hasher.finalize());
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
    assert!(cache.starts_with(&format!("{}\nv1.0.0\n", super::super::super::REPO)));
    assert_eq!(
        client.request_urls(),
        [
            format!(
                "https://github.com/{}/releases/download/v1.0.0/checksums.sha256",
                super::super::super::REPO
            ),
            format!(
                "https://github.com/{}/releases/download/v1.0.0/{}",
                super::super::super::REPO,
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
    let hash = super::super::super::hex_encode(&Sha256::digest(&data));
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
        fn verify(
            &self,
            _: &Path,
            _: &str,
        ) -> Result<super::super::super::attestation::Verification> {
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
    let hash = super::super::super::hex_encode(&hasher.finalize());
    let checksums = format!("{hash}  {}\n", asset_name());
    let client = MockHttpClient::new(vec![Ok(checksums.into_bytes()), Ok(binary_data.to_vec())]);

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
