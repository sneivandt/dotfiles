//! Tests for file-domain resources.

mod chmod {
    use super::super::chmod::*;
    use crate::domains::files::OctalMode;
    #[cfg(unix)]
    use crate::engine::ResourceChange;
    use crate::engine::{IntrinsicState, Resource, ResourceState};
    use std::path::PathBuf;

    /// Shorthand for tests: parse an octal mode or panic.
    fn mode(s: &str) -> OctalMode {
        OctalMode::parse(s).unwrap()
    }

    // -----------------------------------------------------------------------
    // OctalMode
    // -----------------------------------------------------------------------

    #[test]
    fn octal_mode_parses_valid_modes() {
        assert_eq!(OctalMode::parse("644").unwrap().as_u32(), 0o644);
        assert_eq!(OctalMode::parse("755").unwrap().as_u32(), 0o755);
        assert_eq!(OctalMode::parse("0644").unwrap().as_u32(), 0o644);
        assert_eq!(OctalMode::parse("0755").unwrap().as_u32(), 0o755);
        assert_eq!(OctalMode::parse("600").unwrap().as_u32(), 0o600);
        assert_eq!(OctalMode::parse("777").unwrap().as_u32(), 0o777);
    }

    #[test]
    fn octal_mode_rejects_non_digits() {
        let err = OctalMode::parse("abc").unwrap_err();
        assert!(err.contains("must contain only digits"));
    }

    #[test]
    fn octal_mode_rejects_invalid_length() {
        assert!(
            OctalMode::parse("12")
                .unwrap_err()
                .contains("must be 3 or 4 digits")
        );
        assert!(
            OctalMode::parse("12345")
                .unwrap_err()
                .contains("must be 3 or 4 digits")
        );
    }

    #[test]
    fn octal_mode_rejects_invalid_octal_digits() {
        assert!(
            OctalMode::parse("888")
                .unwrap_err()
                .contains("invalid octal digit '8'")
        );
        assert!(
            OctalMode::parse("799")
                .unwrap_err()
                .contains("invalid octal digit '9'")
        );
    }

    #[test]
    fn octal_mode_display() {
        let m = OctalMode::parse("644").unwrap();
        assert_eq!(m.to_string(), "644");
        assert_eq!(m.as_str(), "644");
    }

    // -----------------------------------------------------------------------
    // ChmodResource
    // -----------------------------------------------------------------------

    #[test]
    fn chmod_resource_description() {
        let resource = ChmodResource::new(PathBuf::from("/home/user/.ssh/config"), mode("600"));
        assert!(resource.description().contains("600"));
        assert!(resource.description().contains(".ssh/config"));
    }

    #[test]
    fn chmod_resource_invalid_when_target_missing() {
        let temp_dir = tempfile::tempdir().unwrap();
        let resource = ChmodResource::new(temp_dir.path().join("nonexistent"), mode("600"));

        let state = resource.current_state().unwrap();
        assert_eq!(state, ResourceState::Missing);
    }

    #[cfg(unix)]
    #[test]
    fn chmod_resource_detects_correct_mode() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let file = temp_dir.path().join("test.txt");
        std::fs::write(&file, "test").unwrap();

        let perms = std::fs::Permissions::from_mode(0o644);
        std::fs::set_permissions(&file, perms).unwrap();

        let resource = ChmodResource::new(file, mode("644"));
        let state = resource.current_state().unwrap();
        assert_eq!(state, ResourceState::Correct);
    }

    #[cfg(unix)]
    #[test]
    fn chmod_resource_detects_incorrect_mode() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let file = temp_dir.path().join("test.txt");
        std::fs::write(&file, "test").unwrap();

        let perms = std::fs::Permissions::from_mode(0o644);
        std::fs::set_permissions(&file, perms).unwrap();

        let resource = ChmodResource::new(file, mode("600"));
        let state = resource.current_state().unwrap();
        assert!(matches!(
            state,
            ResourceState::Incorrect { ref current } if current == "644"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn chmod_resource_applies_change() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let file = temp_dir.path().join("test.txt");
        std::fs::write(&file, "test").unwrap();

        let perms = std::fs::Permissions::from_mode(0o644);
        std::fs::set_permissions(&file, perms).unwrap();

        let resource = ChmodResource::new(file.clone(), mode("600"));
        let result = resource.apply().unwrap();
        assert_eq!(result, ResourceChange::Applied);

        let current_mode = std::fs::metadata(&file).unwrap().permissions().mode() & MODE_BITS_MASK;
        assert_eq!(current_mode, 0o600);
    }

    #[test]
    fn from_entry_creates_resource() {
        let entry = crate::domains::files::config::chmod::ChmodEntry::new("600", "ssh/config");

        let home = std::path::Path::new("/home/user");
        let resource = ChmodResource::from_entry(&entry, home);

        assert_eq!(resource.mode.as_ref().unwrap(), &mode("600"));
        assert_eq!(resource.target, PathBuf::from("/home/user/.ssh/config"));
    }

    #[test]
    fn from_entry_normalizes_leading_dot_path() {
        let entry = crate::domains::files::config::chmod::ChmodEntry::new("600", ".ssh/config");

        let home = std::path::Path::new("/home/user");
        let resource = ChmodResource::from_entry(&entry, home);

        assert_eq!(resource.mode.as_ref().unwrap(), &mode("600"));
        assert_eq!(resource.target, PathBuf::from("/home/user/.ssh/config"));
        assert_ne!(resource.target, PathBuf::from("/home/user/..ssh/config"));
    }

    #[test]
    fn from_entry_preserves_invalid_mode_as_invalid_state() {
        let entry = crate::domains::files::config::chmod::ChmodEntry::new("999", "ssh/config");
        let resource = ChmodResource::from_entry(&entry, std::path::Path::new("/home/user"));

        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Invalid { .. }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn ensure_dir_execute_bits_adds_x_for_read() {
        // 600 (rw-------) → 700 (rwx------) for directories
        assert_eq!(ensure_dir_execute_bits(0o600), 0o700);
        // 644 (rw-r--r--) → 755 (rwxr-xr-x)
        assert_eq!(ensure_dir_execute_bits(0o644), 0o755);
        // 640 (rw-r-----) → 750 (rwxr-x---)
        assert_eq!(ensure_dir_execute_bits(0o640), 0o750);
        // 755 stays 755
        assert_eq!(ensure_dir_execute_bits(0o755), 0o755);
        // 000 stays 000
        assert_eq!(ensure_dir_execute_bits(0o000), 0o000);
    }

    #[cfg(unix)]
    #[test]
    fn strip_file_execute_bits_removes_x_bits() {
        assert_eq!(strip_file_execute_bits(0o700), 0o600);
        assert_eq!(strip_file_execute_bits(0o755), 0o644);
        assert_eq!(strip_file_execute_bits(0o644), 0o644);
    }

    #[cfg(unix)]
    #[test]
    fn chmod_directory_applies_safe_file_mode_recursively() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let nested_dir = temp_dir.path().join("nested");
        let file = temp_dir.path().join("secret.txt");
        std::fs::create_dir(&nested_dir).unwrap();
        std::fs::write(&file, "secret").unwrap();

        let resource = ChmodResource::new(temp_dir.path().to_path_buf(), mode("700"));
        let result = resource.apply().unwrap();
        assert_eq!(result, ResourceChange::Applied);

        let root_mode = std::fs::metadata(temp_dir.path())
            .unwrap()
            .permissions()
            .mode()
            & MODE_BITS_MASK;
        let nested_mode =
            std::fs::metadata(&nested_dir).unwrap().permissions().mode() & MODE_BITS_MASK;
        let file_mode = std::fs::metadata(&file).unwrap().permissions().mode() & MODE_BITS_MASK;

        assert_eq!(root_mode, 0o700);
        assert_eq!(nested_mode, 0o700);
        assert_eq!(file_mode, 0o600);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[cfg(unix)]
    #[test]
    fn current_state_detects_wrong_file_inside_correct_directory() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let file = temp_dir.path().join("secret.txt");
        std::fs::write(&file, "secret").unwrap();

        std::fs::set_permissions(temp_dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();

        let resource = ChmodResource::new(temp_dir.path().to_path_buf(), mode("700"));
        let state = resource.current_state().unwrap();
        assert!(
            matches!(state, ResourceState::Incorrect { .. }),
            "expected Incorrect when a file inside has wrong perms, got {state:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn current_state_skips_symlinks_inside_directory() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let file = temp_dir.path().join("ok.txt");
        std::fs::write(&file, "ok").unwrap();

        std::os::unix::fs::symlink("/nonexistent", temp_dir.path().join("dangling")).unwrap();

        std::fs::set_permissions(temp_dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();

        let resource = ChmodResource::new(temp_dir.path().to_path_buf(), mode("700"));
        assert_eq!(
            resource.current_state().unwrap(),
            ResourceState::Correct,
            "symlinks should be skipped during recursive check"
        );
    }
}

mod symlink {
    use super::super::symlink::*;
    use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceState};
    use crate::infra::exec::{Executor, SystemExecutor};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    fn system_executor() -> Arc<dyn Executor> {
        Arc::new(SystemExecutor)
    }

    #[cfg(windows)]
    #[test]
    fn create_junction_invokes_mklink_with_directory_args() {
        use crate::infra::exec::{ExecResult, MockExecutor};

        let mut mock = MockExecutor::new();
        mock.expect_run_windows_cmd_unchecked()
            .once()
            .withf(|command_line| {
                command_line
                    == r#"""mklink" "/J" "C:\Users\test\.config\templates" "C:\repo\symlinks\config\git\templates"""#
            })
            .returning(|_| {
                Ok(ExecResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                    code: Some(0),
                })
            });

        create_junction(
            Path::new(r"C:\repo\symlinks\config\git\templates"),
            Path::new(r"C:\Users\test\.config\templates"),
            &mock,
        )
        .unwrap();
    }

    #[test]
    fn paths_equal_works() {
        let path1 = PathBuf::from("/tmp/test");
        let path2 = PathBuf::from("/tmp/test");
        assert!(paths_equal(&path1, &path2));

        let path3 = PathBuf::from("/tmp/other");
        assert!(!paths_equal(&path1, &path3));
    }

    #[cfg(unix)]
    #[test]
    fn paths_equal_resolves_through_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real_file");
        std::fs::write(&real, "content").unwrap();
        let link = dir.path().join("link_to_file");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        // Should be equal despite different literal paths
        assert!(paths_equal(&real, &link));
    }

    #[test]
    fn paths_equal_handles_nonexistent_paths() {
        // When both paths don't exist and are different, should not be equal
        assert!(!paths_equal(
            Path::new("/nonexistent/path/a"),
            Path::new("/nonexistent/path/b")
        ));
        // When both are the same nonexistent path, should be equal (fast path)
        assert!(paths_equal(
            Path::new("/nonexistent/same"),
            Path::new("/nonexistent/same")
        ));
    }

    #[test]
    fn symlink_resource_description() {
        let resource = SymlinkResource::new(
            PathBuf::from("/source"),
            PathBuf::from("/target"),
            system_executor(),
        );
        assert!(resource.description().contains("/source"));
        assert!(resource.description().contains("/target"));
    }

    #[test]
    fn sibling_temp_path_appends_suffix_without_clobbering_dotfile_name() {
        let bashrc_tmp = sibling_temp_path(Path::new("/home/test/.bashrc"), ".dotfiles_tmp");
        let vimrc_tmp = sibling_temp_path(Path::new("/home/test/.vimrc"), ".dotfiles_tmp");
        let ssh_tmp = sibling_temp_path(Path::new("/home/test/.ssh/config"), ".dotfiles_tmp");

        assert_eq!(bashrc_tmp, PathBuf::from("/home/test/.bashrc.dotfiles_tmp"));
        assert_eq!(vimrc_tmp, PathBuf::from("/home/test/.vimrc.dotfiles_tmp"));
        assert_eq!(
            ssh_tmp,
            PathBuf::from("/home/test/.ssh/config.dotfiles_tmp")
        );
        assert_ne!(bashrc_tmp, vimrc_tmp);
    }

    #[test]
    fn copy_dir_into_place_removes_stale_temp_directory_before_copying() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source = temp_dir.path().join("source");
        let target = temp_dir.path().join("target");
        let stale_tmp = sibling_temp_path(&target, "_dotfiles_tmp");

        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("kept.txt"), "fresh").unwrap();
        std::fs::create_dir(&stale_tmp).unwrap();
        std::fs::write(stale_tmp.join("stale.txt"), "stale").unwrap();

        let executor = system_executor();
        copy_dir_into_place(&source, &target, &*executor).unwrap();

        assert_eq!(
            std::fs::read_to_string(target.join("kept.txt")).unwrap(),
            "fresh"
        );
        assert!(!target.join("stale.txt").exists());
        assert!(!stale_tmp.exists());
    }

    #[test]
    fn symlink_resource_invalid_when_source_missing() {
        let temp_dir = tempfile::tempdir().unwrap();
        let resource = SymlinkResource::new(
            temp_dir.path().join("nonexistent"),
            temp_dir.path().join("target"),
            system_executor(),
        );

        let state = resource.current_state().unwrap();
        assert!(matches!(state, ResourceState::Invalid { .. }));
    }

    #[test]
    fn symlink_resource_missing_when_target_not_exists() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source = temp_dir.path().join("source");
        std::fs::write(&source, "test").unwrap();

        let resource =
            SymlinkResource::new(source, temp_dir.path().join("target"), system_executor());

        let state = resource.current_state().unwrap();
        assert_eq!(state, ResourceState::Missing);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_resource_correct_when_link_points_to_source() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source = temp_dir.path().join("source");
        let target = temp_dir.path().join("target");
        std::fs::write(&source, "test").unwrap();
        std::os::unix::fs::symlink(&source, &target).unwrap();

        let resource = SymlinkResource::new(source, target, system_executor());

        let state = resource.current_state().unwrap();
        assert_eq!(state, ResourceState::Correct);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_resource_incorrect_when_link_points_to_wrong_source() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source = temp_dir.path().join("source");
        let other = temp_dir.path().join("other");
        let target = temp_dir.path().join("target");
        std::fs::write(&source, "test").unwrap();
        std::fs::write(&other, "other").unwrap();
        // link target → other (not source)
        std::os::unix::fs::symlink(&other, &target).unwrap();

        let resource = SymlinkResource::new(source, target, system_executor());

        let state = resource.current_state().unwrap();
        assert!(matches!(state, ResourceState::Incorrect { .. }));
    }

    #[test]
    fn symlink_resource_incorrect_when_target_is_regular_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source = temp_dir.path().join("source");
        let target = temp_dir.path().join("target");
        std::fs::write(&source, "content").unwrap();
        std::fs::write(&target, "other content").unwrap(); // regular file, not a symlink

        let resource = SymlinkResource::new(source, target, system_executor());

        let state = resource.current_state().unwrap();
        assert!(matches!(state, ResourceState::Incorrect { .. }));
    }

    #[test]
    fn symlink_resource_warns_before_replacing_regular_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source = temp_dir.path().join("source");
        let target = temp_dir.path().join("target");
        std::fs::write(&source, "managed content").unwrap();
        std::fs::write(&target, "user content").unwrap();
        let resource = SymlinkResource::new(source, target.clone(), system_executor());

        let warning = resource.pre_apply_warning().unwrap();

        assert_eq!(
            warning.as_deref(),
            Some(
                format!(
                    "replacing existing non-symlink target without backup: {}",
                    target.display()
                )
                .as_str()
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_resource_does_not_warn_before_replacing_wrong_symlink() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source = temp_dir.path().join("source");
        let other = temp_dir.path().join("other");
        let target = temp_dir.path().join("target");
        std::fs::write(&source, "managed content").unwrap();
        std::fs::write(&other, "other content").unwrap();
        std::os::unix::fs::symlink(&other, &target).unwrap();
        let resource = SymlinkResource::new(source, target, system_executor());

        assert_eq!(resource.pre_apply_warning().unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_resource_apply_replaces_regular_file_with_symlink() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source = temp_dir.path().join("source");
        let target = temp_dir.path().join("target");
        std::fs::write(&source, "managed content").unwrap();
        std::fs::write(&target, "user content").unwrap();

        let resource = SymlinkResource::new(source.clone(), target.clone(), system_executor());

        let result = resource.apply().unwrap();
        assert!(matches!(result, ResourceChange::Applied));
        assert!(
            std::fs::symlink_metadata(&target).unwrap().is_symlink(),
            "regular file target must be replaced by a symlink"
        );
        let link_target = std::fs::read_link(&target).unwrap();
        assert_eq!(link_target, source);
    }

    /// A dangling symlink at the target (pointing to a non-existent path) must
    /// be reported as `Incorrect`, not `Missing`.  `Path::exists()` follows
    /// symlinks and returns `false` for dangling ones, so we use
    /// `symlink_metadata()` instead.
    #[cfg(unix)]
    #[test]
    fn symlink_resource_incorrect_when_target_is_dangling_symlink() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source = temp_dir.path().join("source");
        let target = temp_dir.path().join("target");
        let nowhere = temp_dir.path().join("does_not_exist");
        std::fs::write(&source, "content").unwrap();
        // Create a dangling symlink: target -> nowhere (nowhere doesn't exist)
        std::os::unix::fs::symlink(&nowhere, &target).unwrap();
        assert!(!nowhere.exists(), "precondition: nowhere must not exist");
        assert!(
            target.symlink_metadata().is_ok(),
            "precondition: dangling symlink must be present"
        );

        let resource = SymlinkResource::new(source, target, system_executor());

        let state = resource.current_state().unwrap();
        assert!(
            matches!(state, ResourceState::Incorrect { .. }),
            "dangling symlink should be Incorrect, got {state:?}"
        );
    }

    /// After `remove()` the target must be a regular file containing the
    /// original source content, not a symlink.
    #[test]
    #[allow(clippy::redundant_clone, reason = "clone keeps test intent explicit")]
    fn remove_file_symlink_materializes_content() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source = temp_dir.path().join("source.txt");
        let target = temp_dir.path().join("target.txt");
        std::fs::write(&source, b"hello dotfiles").unwrap();

        let resource = SymlinkResource::new(source.clone(), target.clone(), system_executor());
        resource.apply().unwrap();
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Correct
        ));

        resource.remove().unwrap();

        // Must be a regular file, not a symlink.
        let meta = std::fs::symlink_metadata(&target).unwrap();
        assert!(
            !meta.is_symlink(),
            "target should not be a symlink after materialize"
        );
        assert!(meta.is_file(), "target should be a regular file");
        assert_eq!(std::fs::read(&target).unwrap(), b"hello dotfiles");
    }

    /// After `remove()` on a directory symlink the target must be a real
    /// directory containing copies of all source files.
    #[test]
    #[allow(clippy::redundant_clone, reason = "clone keeps test intent explicit")]
    fn remove_dir_symlink_materializes_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source_dir = temp_dir.path().join("src_dir");
        let target_dir = temp_dir.path().join("target_dir");
        std::fs::create_dir(&source_dir).unwrap();
        std::fs::write(source_dir.join("a.txt"), b"aaa").unwrap();
        std::fs::create_dir(source_dir.join("sub")).unwrap();
        std::fs::write(source_dir.join("sub").join("b.txt"), b"bbb").unwrap();

        let resource =
            SymlinkResource::new(source_dir.clone(), target_dir.clone(), system_executor());
        resource.apply().unwrap();
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Correct
        ));

        resource.remove().unwrap();

        // Must be a real directory, not a symlink.
        let meta = std::fs::symlink_metadata(&target_dir).unwrap();
        assert!(
            !meta.is_symlink(),
            "target should not be a symlink after materialize"
        );
        assert!(meta.is_dir(), "target should be a real directory");
        assert_eq!(std::fs::read(target_dir.join("a.txt")).unwrap(), b"aaa");
        assert_eq!(
            std::fs::read(target_dir.join("sub").join("b.txt")).unwrap(),
            b"bbb"
        );
    }

    /// `remove()` on a file symlink must succeed even when the symlink has
    /// already been manually deleted — source content is materialized at the
    /// target path regardless.
    #[cfg(unix)]
    #[test]
    #[allow(clippy::redundant_clone, reason = "clone keeps test intent explicit")]
    fn remove_file_symlink_materializes_content_when_target_already_gone() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source = temp_dir.path().join("source.txt");
        let target = temp_dir.path().join("target.txt");
        std::fs::write(&source, b"hello dotfiles").unwrap();

        let resource = SymlinkResource::new(source.clone(), target.clone(), system_executor());
        resource.apply().unwrap();

        // Manually remove the symlink before calling remove().
        std::fs::remove_file(&target).unwrap();
        assert!(
            target.symlink_metadata().is_err(),
            "precondition: target must be absent"
        );

        // remove() must not error and must materialize source content.
        resource.remove().unwrap();

        let meta = std::fs::symlink_metadata(&target).unwrap();
        assert!(!meta.is_symlink(), "target should not be a symlink");
        assert!(meta.is_file(), "target should be a regular file");
        assert_eq!(std::fs::read(&target).unwrap(), b"hello dotfiles");
    }

    /// `remove()` on a directory symlink must succeed even when the symlink
    /// has already been manually deleted.
    #[cfg(unix)]
    #[test]
    #[allow(clippy::redundant_clone, reason = "clone keeps test intent explicit")]
    fn remove_dir_symlink_materializes_content_when_target_already_gone() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source_dir = temp_dir.path().join("src_dir");
        let target_dir = temp_dir.path().join("target_dir");
        std::fs::create_dir(&source_dir).unwrap();
        std::fs::write(source_dir.join("a.txt"), b"aaa").unwrap();

        let resource =
            SymlinkResource::new(source_dir.clone(), target_dir.clone(), system_executor());
        resource.apply().unwrap();

        // Manually remove the symlink before calling remove().
        std::fs::remove_file(&target_dir).unwrap();
        assert!(
            target_dir.symlink_metadata().is_err(),
            "precondition: target must be absent"
        );

        // remove() must not error and must materialize source content.
        resource.remove().unwrap();

        let meta = std::fs::symlink_metadata(&target_dir).unwrap();
        assert!(!meta.is_symlink(), "target should not be a symlink");
        assert!(meta.is_dir(), "target should be a real directory");
        assert_eq!(std::fs::read(target_dir.join("a.txt")).unwrap(), b"aaa");
    }

    /// `remove()` must not overwrite a real file at the target path — doing so
    /// would destroy user data.  The result must be `Skipped` and the original
    /// file content must remain intact.
    #[test]
    fn remove_does_not_overwrite_real_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source = temp_dir.path().join("source.txt");
        let target = temp_dir.path().join("target.txt");
        std::fs::write(&source, b"source content").unwrap();
        // Write a real file (not a symlink) at target, simulating a user-managed file.
        std::fs::write(&target, b"user content").unwrap();

        let resource = SymlinkResource::new(source, target.clone(), system_executor());
        let result = resource.remove().unwrap();

        assert!(
            matches!(result, ResourceChange::Skipped { .. }),
            "remove() must skip a non-symlink target to prevent data loss, got {result:?}"
        );
        // User content must be completely intact.
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"user content",
            "real file content must not be modified"
        );
    }

    /// `remove()` must not overwrite a real directory at the target path —
    /// doing so would destroy user data.  The result must be `Skipped` and the
    /// directory contents must remain intact.
    #[cfg(unix)]
    #[test]
    fn remove_does_not_overwrite_real_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source_dir = temp_dir.path().join("src_dir");
        let target_dir = temp_dir.path().join("target_dir");
        std::fs::create_dir(&source_dir).unwrap();
        std::fs::write(source_dir.join("source.txt"), b"source content").unwrap();
        // Create a real directory (not a symlink) at target.
        std::fs::create_dir(&target_dir).unwrap();
        std::fs::write(target_dir.join("user.txt"), b"user content").unwrap();

        let resource = SymlinkResource::new(source_dir, target_dir.clone(), system_executor());
        let result = resource.remove().unwrap();

        assert!(
            matches!(result, ResourceChange::Skipped { .. }),
            "remove() must skip a non-symlink target directory to prevent data loss, got {result:?}"
        );
        // User content must be completely intact.
        assert_eq!(
            std::fs::read(target_dir.join("user.txt")).unwrap(),
            b"user content",
            "real directory content must not be modified"
        );
    }
}
