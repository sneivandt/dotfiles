use super::*;

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
        super::super::replace_binary(&bin, &staged).unwrap();
        assert_eq!(fs::read(&bin).unwrap(), new);
        assert_eq!(fs::read(&backup).unwrap(), before_retry);
        assert!(!staged.exists());
        smoke_test_binary(&bin).unwrap();
    }
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
