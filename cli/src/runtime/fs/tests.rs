#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]

use super::*;

#[test]
fn copies_files_and_subdirectories() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();

    std::fs::write(src.path().join("a.txt"), b"aaa").unwrap();
    std::fs::create_dir(src.path().join("sub")).unwrap();
    std::fs::write(src.path().join("sub/b.txt"), b"bbb").unwrap();

    let target = dst.path().join("out");
    copy_dir_recursive(src.path(), &target, false).unwrap();

    assert_eq!(std::fs::read(target.join("a.txt")).unwrap(), b"aaa");
    assert_eq!(std::fs::read(target.join("sub/b.txt")).unwrap(), b"bbb");
}

#[test]
fn skips_git_directory_when_flag_set() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();

    std::fs::write(src.path().join("file.txt"), b"content").unwrap();
    std::fs::create_dir(src.path().join(".git")).unwrap();
    std::fs::write(src.path().join(".git/HEAD"), b"ref: refs/heads/main").unwrap();

    let target = dst.path().join("out");
    copy_dir_recursive(src.path(), &target, true).unwrap();

    assert!(target.join("file.txt").exists());
    assert!(
        !target.join(".git").exists(),
        ".git directory should be skipped"
    );
}

#[test]
fn copies_git_directory_when_flag_not_set() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();

    std::fs::write(src.path().join("file.txt"), b"content").unwrap();
    std::fs::create_dir(src.path().join(".git")).unwrap();
    std::fs::write(src.path().join(".git/HEAD"), b"ref: refs/heads/main").unwrap();

    let target = dst.path().join("out");
    copy_dir_recursive(src.path(), &target, false).unwrap();

    assert!(target.join("file.txt").exists());
    assert!(
        target.join(".git/HEAD").exists(),
        ".git directory should be copied"
    );
}

#[cfg(unix)]
#[test]
fn recreates_symlinks_in_destination() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();

    // Create a shared target directory that both symlinks point at.
    let shared = tempfile::tempdir().unwrap();
    std::fs::write(shared.path().join("shared.txt"), b"shared").unwrap();

    // Two branches each have a symlink to the same external directory.
    std::fs::create_dir(src.path().join("a")).unwrap();
    std::os::unix::fs::symlink(shared.path(), src.path().join("a/link")).unwrap();
    std::fs::create_dir(src.path().join("b")).unwrap();
    std::os::unix::fs::symlink(shared.path(), src.path().join("b/link")).unwrap();

    let target = dst.path().join("out");
    copy_dir_recursive(src.path(), &target, false).unwrap();

    // Symlinks are recreated in dst (not followed/inlined).
    let meta_a = target.join("a/link").symlink_metadata().unwrap();
    assert!(
        meta_a.file_type().is_symlink(),
        "a/link should be a symlink"
    );
    let meta_b = target.join("b/link").symlink_metadata().unwrap();
    assert!(
        meta_b.file_type().is_symlink(),
        "b/link should be a symlink"
    );
    // The recreated symlinks still point at the same external location.
    assert_eq!(
        std::fs::read_link(target.join("a/link")).unwrap(),
        shared.path()
    );
    assert_eq!(
        std::fs::read_link(target.join("b/link")).unwrap(),
        shared.path()
    );
}

#[cfg(unix)]
#[test]
fn does_not_traverse_symlink_cycles() {
    let src = tempfile::tempdir().unwrap();
    std::fs::write(src.path().join("file.txt"), b"content").unwrap();
    std::fs::create_dir(src.path().join("sub")).unwrap();
    // Create a symlink that would form a cycle if followed.
    std::os::unix::fs::symlink(src.path(), src.path().join("sub/loop")).unwrap();

    let dst = tempfile::tempdir().unwrap();
    let target = dst.path().join("out");

    // The copy should succeed: the cycle-forming symlink is recreated as
    // a symlink rather than followed, so no infinite recursion occurs.
    copy_dir_recursive(src.path(), &target, false).unwrap();

    assert!(target.join("file.txt").exists());
    assert!(target.join("sub").is_dir());
    let loop_meta = target.join("sub/loop").symlink_metadata().unwrap();
    assert!(
        loop_meta.file_type().is_symlink(),
        "sub/loop should be recreated as a symlink"
    );
}

#[cfg(unix)]
#[test]
fn does_not_copy_external_symlink_directory_contents() {
    let external = tempfile::tempdir().unwrap();
    std::fs::write(external.path().join("secret.txt"), b"secret").unwrap();

    let src = tempfile::tempdir().unwrap();
    std::fs::write(src.path().join("local.txt"), b"local").unwrap();
    // Symlink inside src pointing outside the source tree.
    std::os::unix::fs::symlink(external.path(), src.path().join("ext_link")).unwrap();

    let dst = tempfile::tempdir().unwrap();
    let target = dst.path().join("out");
    copy_dir_recursive(src.path(), &target, false).unwrap();

    // Local file is copied normally.
    assert_eq!(std::fs::read(target.join("local.txt")).unwrap(), b"local");

    // The external symlink is recreated as a symlink — not inlined as a
    // real directory containing the external contents.
    let ext_meta = target.join("ext_link").symlink_metadata().unwrap();
    assert!(
        ext_meta.file_type().is_symlink(),
        "ext_link should be recreated as a symlink, not a real directory"
    );
    assert_eq!(
        std::fs::read_link(target.join("ext_link")).unwrap(),
        external.path()
    );
}

// -----------------------------------------------------------------------
// ensure_parent_dir
// -----------------------------------------------------------------------

#[test]
fn ensure_parent_dir_creates_missing_parents() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("a").join("b").join("file.txt");
    ensure_parent_dir(&nested).unwrap();
    assert!(dir.path().join("a").join("b").exists());
}

#[test]
fn ensure_parent_dir_noop_when_parent_exists() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("file.txt");
    ensure_parent_dir(&file).unwrap();
    assert!(dir.path().exists());
}

// -----------------------------------------------------------------------
// remove_existing
// -----------------------------------------------------------------------

#[test]
fn remove_existing_removes_regular_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("target");
    std::fs::write(&file, "content").unwrap();
    remove_existing(&file).unwrap();
    assert!(!file.exists());
}

#[test]
fn remove_existing_removes_empty_directory() {
    let dir = tempfile::tempdir().unwrap();
    let target_dir = dir.path().join("target");
    std::fs::create_dir(&target_dir).unwrap();
    remove_existing(&target_dir).unwrap();
    assert!(!target_dir.exists());
}

#[test]
fn remove_existing_noop_when_path_absent() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("nonexistent");
    remove_existing(&file).unwrap();
}

#[cfg(unix)]
#[test]
fn remove_existing_removes_broken_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink("/nonexistent/target", &link).unwrap();
    assert!(link.symlink_metadata().is_ok());
    remove_existing(&link).unwrap();
    assert!(link.symlink_metadata().is_err());
}

// -----------------------------------------------------------------------
// TempPath
// -----------------------------------------------------------------------

#[test]
fn temp_path_removes_file_on_drop() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("tmp_file");
    std::fs::write(&file, "data").unwrap();
    assert!(file.exists());

    {
        let _guard = TempPath::new(file.clone());
    }
    assert!(!file.exists(), "file should be removed on drop");
}

#[test]
fn temp_path_persist_prevents_removal() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("keep_file");
    std::fs::write(&file, "data").unwrap();

    {
        let mut guard = TempPath::new(file.clone());
        guard.persist();
    }
    assert!(file.exists(), "file should remain after persist + drop");
}

#[test]
fn temp_path_noop_when_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("nonexistent");
    // Should not panic when the file doesn't exist
    let _guard = TempPath::new(file);
}

// -----------------------------------------------------------------------
// TempDir
// -----------------------------------------------------------------------

#[test]
fn temp_dir_removes_directory_on_drop() {
    let dir = tempfile::tempdir().unwrap();
    let td = dir.path().join("tmp_dir");
    std::fs::create_dir(&td).unwrap();
    std::fs::write(td.join("child.txt"), "data").unwrap();
    assert!(td.exists());

    {
        let _guard = TempDir::new(td.clone());
    }
    assert!(!td.exists(), "directory should be removed on drop");
}

#[test]
fn temp_dir_persist_prevents_removal() {
    let dir = tempfile::tempdir().unwrap();
    let td = dir.path().join("keep_dir");
    std::fs::create_dir(&td).unwrap();

    {
        let mut guard = TempDir::new(td.clone());
        guard.persist();
    }
    assert!(td.exists(), "directory should remain after persist + drop");
}
