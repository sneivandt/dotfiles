use super::*;

/// A runnable fixture for the startup/recovery tests (not a release asset).
fn version_capable_binary() -> Vec<u8> {
    #[cfg(unix)]
    {
        b"#!/bin/sh\n[ \"$#:$1\" = 1:--version ] || exit 42\nprintf 'dotfiles fixture\\n'\n"
            .to_vec()
    }
    #[cfg(windows)]
    {
        static FIXTURE: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
        FIXTURE
            .get_or_init(|| {
                let dir = tempfile::tempdir().unwrap();
                let src = dir.path().join("fixture.rs");
                let exe = dir.path().join("fixture.exe");
                fs::write(
                    &src,
                    concat!(
                        "fn main() {\n",
                        "    let mut args = std::env::args().skip(1);\n",
                        "    if args.next().as_deref() != Some(\"--version\") || args.next().is_some() {\n",
                        "        std::process::exit(42);\n",
                        "    }\n",
                        "    println!(\"dotfiles fixture\");\n",
                        "}\n",
                    ),
                )
                .unwrap();
                let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
                let output = std::process::Command::new(rustc)
                    .env_remove("RUSTFLAGS")
                    .env_remove("CARGO_ENCODED_RUSTFLAGS")
                    .args(["--edition", "2024", "-o"])
                    .arg(&exe)
                    .arg(&src)
                    .output()
                    .expect("failed to invoke rustc for fixture binary");
                assert!(
                    output.status.success(),
                    "failed to compile fixture binary: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                fs::read(&exe).unwrap()
            })
            .clone()
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

struct WrapperFixture {
    _root: tempfile::TempDir,
    bin: PathBuf,
    old: PathBuf,
    wrapper_path: PathBuf,
    _lock: Option<UpdateJournal>,
}

fn setup_wrapper_phase(
    repo: &Path,
    wrapper: &str,
    phase: &str,
    keep_lock: bool,
    good: &[u8],
    bad: &[u8],
) -> WrapperFixture {
    let root = tempfile::tempdir_in(".").unwrap();
    let bin = binary_path(root.path());
    let old = old_binary_path(root.path());
    fs::create_dir(bin.parent().unwrap()).unwrap();
    let wrapper_path = root.path().join(wrapper);
    fs::copy(repo.join(wrapper), &wrapper_path).unwrap();
    let write_image = |path: &Path, data: &[u8]| {
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
            bad
        } else {
            good
        },
    );
    if !matches!(phase, "missing" | "legacy") {
        write_image(&bin, if phase == "healthy" { good } else { bad });
    }
    let lock = if phase == "legacy" {
        None
    } else {
        let active = begin_replacement(&bin).unwrap();
        if keep_lock {
            Some(active)
        } else {
            drop(active); // process exit without confirming the installation
            None
        }
    };
    WrapperFixture {
        _root: root,
        bin,
        old,
        wrapper_path,
        _lock: lock,
    }
}

fn assert_wrapper_phase(
    phase: &str,
    bin: &Path,
    old: &Path,
    locked: bool,
    good: &[u8],
    bad: &[u8],
) {
    if locked {
        assert!(!bin.exists());
        assert_eq!(fs::read(old).unwrap(), good);
        return;
    }
    match phase {
        "legacy" => {
            assert!(!bin.exists());
            assert_eq!(fs::read(old).unwrap(), good);
        }
        "both-failed" => {
            assert_eq!(fs::read(bin).unwrap(), bad);
            assert_eq!(fs::read(old).unwrap(), bad);
            assert!(begin_replacement(bin).is_err());
        }
        _ => {
            assert_eq!(fs::read(bin).unwrap(), good);
            assert_eq!(old.exists(), phase == "healthy");
            begin_replacement(bin).unwrap(); // next update no longer requires recovery
        }
    }
}

#[test]
fn checkout_wrapper_recovers_only_unlocked_unfinished_updates() {
    use std::fmt::Write as _;

    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let good = version_capable_binary();
    #[cfg(unix)]
    let bad = b"#!/bin/sh\nexit 1\n".to_vec();
    #[cfg(windows)]
    let bad =
        fs::read(PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/where.exe"))
            .unwrap();
    let cases = [
        ("missing-locked", "missing", true),
        ("missing", "missing", false),
        ("failed", "failed", false),
        ("healthy", "healthy", false),
        ("both-failed", "both-failed", false),
        ("legacy", "legacy", false),
    ];

    #[cfg(not(windows))]
    {
        let shell = which::which("sh").unwrap();
        for (label, phase, keep_lock) in cases {
            let fixture = setup_wrapper_phase(repo, "dotfiles.sh", phase, keep_lock, &good, &bad);
            let output = std::process::Command::new(&shell)
                .arg(&fixture.wrapper_path)
                .arg("--version")
                .output()
                .unwrap();
            let expect_success = !keep_lock && phase != "both-failed";
            assert_eq!(
                output.status.success(),
                expect_success,
                "dotfiles.sh {label}: {output:?}"
            );
            assert_wrapper_phase(phase, &fixture.bin, &fixture.old, keep_lock, &good, &bad);
        }
    }

    let ps_shell = which::which("pwsh")
        .ok()
        .or_else(|| cfg!(windows).then(|| which::which("powershell").unwrap()));
    if let Some(shell) = ps_shell {
        // Run all PowerShell wrapper phases in a single shell process so startup
        // overhead is paid once per test execution.
        let fixtures: Vec<_> = cases
            .iter()
            .map(|&(label, phase, keep_lock)| {
                (
                    label,
                    phase,
                    keep_lock,
                    setup_wrapper_phase(repo, "dotfiles.ps1", phase, keep_lock, &good, &bad),
                )
            })
            .collect();
        let mut driver = String::new();
        for (label, _, _, fixture) in &fixtures {
            let escaped = fixture.wrapper_path.to_string_lossy().replace('\'', "''");
            writeln!(
                driver,
                "$global:LASTEXITCODE = 0; $ok = $true; \
                 try {{ $null = & '{escaped}' --version; if ($LASTEXITCODE -ne 0) {{ $ok = $false }} }} \
                 catch {{ $ok = $false }}; \
                 Write-Output ('{label}=' + $(if ($ok) {{ 'ok' }} else {{ 'fail' }}))"
            )
            .unwrap();
        }
        let output = std::process::Command::new(&shell)
            .args(["-NoProfile", "-NonInteractive", "-Command", &driver])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "dotfiles.ps1 driver failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let actual: Vec<&str> = stdout.lines().collect();
        let expected: Vec<String> = cases
            .iter()
            .map(|&(label, phase, keep_lock)| {
                let expect_success = !keep_lock && phase != "both-failed";
                format!("{label}={}", if expect_success { "ok" } else { "fail" })
            })
            .collect();
        assert_eq!(
            actual, expected,
            "unexpected dotfiles.ps1 outcomes: {stdout}"
        );
        for (_, phase, keep_lock, fixture) in fixtures {
            assert_wrapper_phase(phase, &fixture.bin, &fixture.old, keep_lock, &good, &bad);
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

fn journal_path(bin: &Path) -> PathBuf {
    let name = bin.file_name().unwrap().to_str().unwrap();
    bin.with_file_name(format!(".{name}.update-state"))
}

#[test]
fn pre_mutation_replacement_failure_clears_journal_for_retry() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let bin = binary_path(dir.path());
    fs::create_dir_all(bin.parent().unwrap()).unwrap();
    fs::write(&bin, b"old-binary").unwrap();
    let occupied_backup = old_binary_path(dir.path());
    fs::create_dir(&occupied_backup).unwrap();

    let new_binary = b"dotfiles v1.0.0\n";
    let hash = super::super::super::hex_encode(&Sha256::digest(new_binary));
    let checksums = format!("{hash}  {}\n", asset_name());
    let client = MockHttpClient::new(vec![Ok(checksums.into_bytes()), Ok(new_binary.to_vec())]);

    let error = install_release(
        dir.path(),
        "v1.0.0",
        &client,
        &StubGh { verified: true },
        Policy::Required,
        |path| Ok(fs::read_to_string(path)?),
    )
    .unwrap_err();

    assert!(
        format!("{error:#}").contains("removing stale backup"),
        "unexpected error: {error:#}"
    );
    assert_eq!(fs::read(&bin).unwrap(), b"old-binary");
    assert_eq!(fs::metadata(journal_path(&bin)).unwrap().len(), 0);
    begin_replacement(&bin).expect("journal must be clean after pre-mutation failure");
}

#[test]
fn rename_failure_with_clean_rollback_clears_journal() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let bin = dir.path().join("dotfiles");
    let missing_staged = dir.path().join("missing-staged");
    fs::write(&bin, b"old-binary").unwrap();

    let journal = begin_replacement(&bin).unwrap();
    let replacement_err = super::super::replace_binary(&bin, &missing_staged).unwrap_err();
    let error = finish_failed_replacement(&journal, replacement_err);
    drop(journal);

    assert!(
        format!("{error:#}").contains("moving new binary into place"),
        "unexpected error: {error:#}"
    );
    assert_eq!(fs::read(&bin).unwrap(), b"old-binary");
    assert!(!dir.path().join(old_binary_name()).exists());
    assert_eq!(fs::metadata(journal_path(&bin)).unwrap().len(), 0);
    begin_replacement(&bin).expect("journal must be clean after successful rollback");
}

#[test]
fn failed_rollback_preserves_pending_journal() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let bin = dir.path().join("dotfiles");
    let staged = dir.path().join("staged");
    fs::write(&bin, b"old-binary").unwrap();
    fs::write(&staged, b"new-binary").unwrap();

    let journal = begin_replacement(&bin).unwrap();
    let replacement_err = install_staged_binary_with(
        &bin,
        &staged,
        |_, _| Err(std::io::Error::other("simulated rename failure")),
        |_, _| Err(std::io::Error::other("simulated rollback failure")),
    )
    .unwrap_err();
    let error = finish_failed_replacement(&journal, replacement_err);
    drop(journal);

    let msg = format!("{error:#}");
    assert!(msg.contains("simulated rename failure"), "{msg}");
    assert!(msg.contains("simulated rollback failure"), "{msg}");
    assert_eq!(fs::metadata(journal_path(&bin)).unwrap().len(), 1);
    let retry_err = begin_replacement(&bin).unwrap_err();
    assert!(
        format!("{retry_err:#}").contains("unfinished self-update"),
        "failed rollback must leave recovery required: {retry_err:#}"
    );
}
