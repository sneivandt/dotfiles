use super::*;

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
    let bad =
        fs::read(PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/where.exe"))
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
    let hash = super::super::super::hex_encode(&hasher.finalize());
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
