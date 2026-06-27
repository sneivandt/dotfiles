use super::*;

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
    assert!(matches!(state, ResourceState::Invalid { .. }));
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
    let entry = crate::config::chmod::ChmodEntry {
        mode: "600".to_string(),
        path: "ssh/config".to_string(),
    };

    let home = std::path::Path::new("/home/user");
    let resource = ChmodResource::from_entry(&entry, home).unwrap();

    assert_eq!(resource.mode, mode("600"));
    assert_eq!(resource.target, PathBuf::from("/home/user/.ssh/config"));
}

#[test]
fn from_entry_normalizes_leading_dot_path() {
    let entry = crate::config::chmod::ChmodEntry {
        mode: "600".to_string(),
        path: ".ssh/config".to_string(),
    };

    let home = std::path::Path::new("/home/user");
    let resource = ChmodResource::from_entry(&entry, home).unwrap();

    assert_eq!(resource.mode, mode("600"));
    assert_eq!(resource.target, PathBuf::from("/home/user/.ssh/config"));
    assert_ne!(resource.target, PathBuf::from("/home/user/..ssh/config"));
}

#[test]
fn from_entry_rejects_invalid_mode() {
    let entry = crate::config::chmod::ChmodEntry {
        mode: "999".to_string(),
        path: "ssh/config".to_string(),
    };
    let home = std::path::Path::new("/home/user");
    assert!(ChmodResource::from_entry(&entry, home).is_err());
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
    let nested_mode = std::fs::metadata(&nested_dir).unwrap().permissions().mode() & MODE_BITS_MASK;
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
