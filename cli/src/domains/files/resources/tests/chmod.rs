use super::super::chmod::*;
use crate::domains::files::config::chmod::OctalMode;
#[cfg(unix)]
use crate::engine::ResourceChange;
use crate::engine::{IntrinsicState, Resource, ResourceState};
use std::path::PathBuf;

/// Shorthand for tests: parse an octal mode or panic.
fn mode(s: &str) -> OctalMode {
    OctalMode::parse(s).unwrap()
}

#[cfg(unix)]
#[test]
fn permission_descriptions_show_redirections_without_repeating_ordinary_paths() {
    use crate::domains::files::config::chmod::ChmodEntry;
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let ordinary = home.join(".ordinary");
    let outside = fixture.path().join("outside");
    std::fs::write(&ordinary, "fixture").unwrap();
    std::fs::write(&outside, "fixture").unwrap();
    let ordinary_resource = ChmodResource::from_entry(&ChmodEntry::new("600", "ordinary"), &home);
    assert_eq!(
        ordinary_resource.description(),
        format!("600 {}", ordinary.display())
    );
    std::os::unix::fs::symlink(&outside, home.join(".linked")).unwrap();
    let redirected_resource = ChmodResource::from_entry(&ChmodEntry::new("600", "linked"), &home);
    assert_eq!(
        redirected_resource.description(),
        format!(
            "600 {} (resolved {})",
            home.join(".linked").display(),
            outside.display()
        )
    );
}

// -----------------------------------------------------------------------
// OctalMode
// -----------------------------------------------------------------------

#[test]
fn octal_mode_parses_valid_modes() {
    for (input, bits) in [
        ("000", 0o000),
        ("644", 0o644),
        ("755", 0o755),
        ("0644", 0o644),
        ("0755", 0o755),
        ("600", 0o600),
        ("777", 0o777),
        ("7777", 0o7777),
    ] {
        let parsed = OctalMode::parse(input).unwrap();
        assert_eq!(parsed.as_u32(), bits, "{input}: permission bits");
        assert_eq!(parsed.as_str(), input, "{input}: original spelling");
        assert_eq!(parsed.to_string(), input, "{input}: display");
    }
}

#[test]
fn octal_mode_rejects_invalid_input() {
    for (input, reason) in [
        ("abc", "must contain only digits"),
        (" 644", "must contain only digits"),
        ("", "must be 3 or 4 digits"),
        ("12", "must be 3 or 4 digits"),
        ("12345", "must be 3 or 4 digits"),
        ("888", "invalid octal digit '8'"),
        ("799", "invalid octal digit '9'"),
    ] {
        let error = OctalMode::parse(input).unwrap_err();
        assert!(error.contains(reason), "{input:?}: {error}");
    }
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
fn chmod_state_propagates_non_permission_inspection_errors() {
    use crate::domains::files::config::chmod::ChmodEntry;
    use crate::engine::resource::ResourceError;
    use std::os::unix::fs::PermissionsExt as _;

    let fixture = tempfile::tempdir_in(".").unwrap();
    let home = fixture.path().canonicalize().unwrap();
    let file = home.join(".not-directory");
    std::fs::write(&file, "fixture").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
    let resource =
        ChmodResource::from_entry(&ChmodEntry::new("600", "not-directory/config"), &home);
    match resource.current_state() {
        Err(ResourceError::Io { source }) => {
            assert_eq!(source.kind(), std::io::ErrorKind::NotADirectory);
        }
        other => {
            panic!("a non-directory ancestor is an inspection error, not unknown: {other:?}")
        }
    }
    assert_eq!(
        file.metadata().unwrap().permissions().mode() & MODE_BITS_MASK,
        0o640
    );
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
fn from_entry_normalizes_leading_dot_path() {
    let home = std::path::Path::new("/home/user");
    for path in [
        "ssh/config",
        ".ssh/config",
        "./ssh/config",
        "./.ssh/config",
        "././ssh/./config",
    ] {
        let entry = crate::domains::files::config::chmod::ChmodEntry::new("600", path);
        let resource = ChmodResource::from_entry(&entry, home);
        assert_eq!(resource.mode.as_ref().unwrap(), &mode("600"), "{path}");
        assert_eq!(resource.target, home.join(".ssh/config"), "{path}");
    }
    #[cfg(windows)]
    for path in [r".\ssh\config", r".\.ssh\config", r".\.\ssh\.\config"] {
        let entry = crate::domains::files::config::chmod::ChmodEntry::new("600", path);
        let resource = ChmodResource::from_entry(&entry, home);
        assert_eq!(resource.target, home.join(".ssh/config"), "{path}");
    }
}

#[cfg(unix)]
#[test]
fn chmod_dot_relative_entry_changes_only_the_managed_target() {
    use crate::domains::files::config::chmod::ChmodEntry;
    use std::os::unix::fs::PermissionsExt as _;

    let fixture = tempfile::tempdir_in(".").unwrap();
    let home = fixture.path();
    for directory in [".ssh", "ssh"] {
        std::fs::create_dir(home.join(directory)).unwrap();
        let path = home.join(directory).join("config");
        std::fs::write(&path, "fixture").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    let resource = ChmodResource::from_entry(&ChmodEntry::new("600", "./ssh/config"), home);

    assert!(matches!(
        resource.current_state().unwrap(),
        ResourceState::Incorrect { .. }
    ));
    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    for (relative, expected) in [(".ssh/config", 0o600), ("ssh/config", 0o644)] {
        assert_eq!(
            std::fs::metadata(home.join(relative))
                .unwrap()
                .permissions()
                .mode()
                & MODE_BITS_MASK,
            expected,
            "{relative}"
        );
    }
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
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

#[test]
fn invalid_chmod_paths_cannot_mutate_home() {
    use crate::domains::files::config::chmod::ChmodEntry;

    let fixture = tempfile::tempdir_in(".").unwrap();
    let home = fixture.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let secret = home.join("secret");
    std::fs::write(&secret, "private content").unwrap();
    let home_permissions = std::fs::metadata(&home).unwrap().permissions();
    let secret_permissions = std::fs::metadata(&secret).unwrap().permissions();
    let paths = [
        "", " ", ".", "./", "././", ".//.//", "ssh/..", ".ssh/..", "../home", "/",
    ];
    for path in paths {
        let resource = ChmodResource::from_entry(&ChmodEntry::new("755", path), &home);
        assert!(
            matches!(
                resource.current_state().unwrap(),
                ResourceState::Invalid { .. }
            ),
            "{path:?}"
        );
        assert!(
            resource.apply().is_err(),
            "{path:?}: direct apply must also be guarded"
        );
        assert_eq!(
            std::fs::metadata(&home).unwrap().permissions(),
            home_permissions
        );
        assert_eq!(
            std::fs::metadata(&secret).unwrap().permissions(),
            secret_permissions
        );
        assert_eq!(std::fs::read_to_string(&secret).unwrap(), "private content");
    }
}

#[test]
fn chmod_rejects_home_root_directory_alias() {
    use crate::domains::files::config::chmod::ChmodEntry;

    let fixture = tempfile::tempdir_in(".").unwrap();
    let home = fixture.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let alias = home.join(".alias");
    #[cfg(unix)]
    std::os::unix::fs::symlink(std::fs::canonicalize(&home).unwrap(), &alias).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(std::fs::canonicalize(&home).unwrap(), &alias)
        .expect("native symlinks require a symlink-capable test worker");
    let before = std::fs::metadata(&home).unwrap().permissions();
    let resource = ChmodResource::from_entry(&ChmodEntry::new("755", "alias"), &home);
    assert!(matches!(
        resource.current_state().unwrap(),
        ResourceState::Invalid { reason } if reason.contains("$HOME itself")
    ));
    assert!(resource.apply().is_err());
    assert_eq!(std::fs::metadata(&home).unwrap().permissions(), before);
}

#[cfg(windows)]
#[test]
fn chmod_rejects_windows_root_spellings() {
    use crate::domains::files::config::chmod::ChmodEntry;

    for path in [
        r".\",
        r".\.\",
        r"\",
        r"C:\",
        r"C:relative",
        r"\\server\share",
    ] {
        let resource = ChmodResource::from_entry(
            &ChmodEntry::new("755", path),
            std::path::Path::new(r"C:\home"),
        );
        assert!(
            matches!(
                resource.current_state().unwrap(),
                ResourceState::Invalid { .. }
            ),
            "{path:?}"
        );
        assert!(resource.apply().is_err(), "{path:?}");
    }
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
    assert_eq!(ensure_dir_execute_bits(0o222), 0o222);
    assert_eq!(ensure_dir_execute_bits(0o7111), 0o7111);
    assert_eq!(ensure_dir_execute_bits(0o7644), 0o7755);
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
    assert_eq!(
        resource.apply().unwrap(),
        ResourceChange::AlreadyCorrect,
        "an already-correct accessible tree must not report a permission change"
    );
}

#[cfg(unix)]
#[test]
fn recursive_changes_report_the_distinct_original_mode_of_each_recovered_directory() {
    use std::os::unix::fs::PermissionsExt as _;

    let fixture = tempfile::tempdir_in(".").unwrap();
    let root = fixture.path().join("tree");
    let nested = root.join("nested");
    let deepest = nested.join("deepest");
    std::fs::create_dir_all(&deepest).unwrap();
    for (path, initial) in [(&deepest, 0o200), (&nested, 0o000), (&root, 0o000)] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(initial)).unwrap();
    }
    let resource = ChmodResource::new(root.clone(), mode("000"));
    assert_eq!(
        resource.apply().unwrap(),
        ResourceChange::Applied,
        "the deepest directory changed even though its recovered ancestors already matched"
    );
    for directory in [&root, &nested, &deepest] {
        assert_eq!(
            directory.metadata().unwrap().permissions().mode() & MODE_BITS_MASK,
            0
        );
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn restrictive_recursive_modes_finish_descendants_and_recover_on_repeat() {
    use std::os::unix::fs::PermissionsExt as _;

    for desired in [0o000, 0o200, 0o111] {
        for initial in [0o000, 0o700] {
            let fixture = tempfile::tempdir_in(".").unwrap();
            let root = fixture.path().join("tree");
            let nested = root.join("nested");
            let file = nested.join("data");
            std::fs::create_dir_all(&nested).unwrap();
            std::fs::write(&file, "fixture").unwrap();
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
            for directory in [&nested, &root] {
                std::fs::set_permissions(directory, std::fs::Permissions::from_mode(initial))
                    .unwrap();
            }
            let resource = ChmodResource::new(root.clone(), mode(&format!("{desired:03o}")));
            assert!(matches!(
                resource.current_state().unwrap(),
                ResourceState::Incorrect { .. }
            ));
            assert_eq!(
                resource.apply().unwrap(),
                ResourceChange::Applied,
                "desired={desired:o}, initial={initial:o}"
            );
            // Reopen only to assert descendants and clean up the fixture.
            for directory in [&root, &nested] {
                assert_eq!(
                    directory.metadata().unwrap().permissions().mode() & MODE_BITS_MASK,
                    desired
                );
                std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
                    .unwrap();
            }
            assert_eq!(
                file.metadata().unwrap().permissions().mode() & MODE_BITS_MASK,
                strip_file_execute_bits(desired)
            );
            for directory in [&nested, &root] {
                std::fs::set_permissions(directory, std::fs::Permissions::from_mode(desired))
                    .unwrap();
            }
            assert_eq!(
                resource.apply().unwrap(),
                ResourceChange::AlreadyCorrect,
                "repeating a restrictive declaration must converge"
            );
            for directory in [&root, &nested] {
                assert_eq!(
                    directory.metadata().unwrap().permissions().mode() & MODE_BITS_MASK,
                    desired
                );
                std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
                    .unwrap();
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn restrictive_recursive_preflight_restores_access_when_hard_links_are_found() {
    use std::os::unix::fs::PermissionsExt as _;

    let fixture = tempfile::tempdir_in(".").unwrap();
    let root = fixture.path().join("tree");
    let nested = root.join("nested");
    let outside = fixture.path().join("outside");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(&outside, "unmanaged").unwrap();
    std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::hard_link(&outside, nested.join("alias")).unwrap();
    for directory in [&nested, &root] {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o000)).unwrap();
    }
    let resource = ChmodResource::new(root.clone(), mode("000"));
    let error = resource.apply().unwrap_err();
    assert!(
        format!("{error:#}").contains("multiply linked file"),
        "{error:#}"
    );
    for directory in [&root, &nested] {
        assert_eq!(
            directory.metadata().unwrap().permissions().mode() & MODE_BITS_MASK,
            0
        );
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    assert_eq!(
        outside.metadata().unwrap().permissions().mode() & MODE_BITS_MASK,
        0o600
    );
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
fn recursive_modes_leave_ipc_endpoints_and_their_other_names_untouched() {
    use nix::sys::stat::Mode;
    use std::os::unix::fs::PermissionsExt as _;

    let fixture = tempfile::tempdir().unwrap();
    let tree = fixture.path().join("tree");
    let external = fixture.path().join("external-pipe");
    let endpoint = tree.join("endpoint");
    let config = tree.join("config");
    std::fs::create_dir(&tree).unwrap();
    std::fs::write(&config, "custom").unwrap();
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o600)).unwrap();
    nix::unistd::mkfifo(&external, Mode::S_IRWXU).unwrap();
    std::fs::hard_link(&external, &endpoint).unwrap();
    let bits = |path: &std::path::Path| path.metadata().unwrap().permissions().mode() & 0o777;
    let original_mode = bits(&external);
    let recursive = ChmodResource::new(tree, mode("640"));
    let direct = ChmodResource::new(endpoint.clone(), mode("640"));
    assert!(matches!(
        direct.current_state().unwrap(),
        ResourceState::Invalid { .. }
    ));
    assert!(direct.apply().is_err());
    assert_eq!(recursive.apply().unwrap(), ResourceChange::Applied);
    assert_eq!(bits(&config), 0o640);
    assert_eq!(bits(&endpoint), original_mode);
    assert_eq!(bits(&external), original_mode);
    assert_eq!(recursive.current_state().unwrap(), ResourceState::Correct);
    assert_eq!(recursive.apply().unwrap(), ResourceChange::AlreadyCorrect);

    std::fs::remove_file(&endpoint).unwrap();
    std::fs::write(&endpoint, "now a regular file").unwrap();
    direct.apply().unwrap();
    assert_eq!(bits(&endpoint), 0o640);
    assert_eq!(bits(&external), original_mode);
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

#[cfg(unix)]
#[test]
fn recursive_apply_never_follows_file_or_directory_symlinks() {
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    let fixture = tempfile::tempdir_in(".").unwrap();
    let root = fixture.path().canonicalize().unwrap();
    let managed = root.join("managed");
    let outside = root.join("outside");
    std::fs::create_dir(&managed).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let external_file = outside.join("private");
    std::fs::write(&external_file, "external content").unwrap();
    std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&external_file, std::fs::Permissions::from_mode(0o644)).unwrap();
    let owned = managed.join("owned");
    std::fs::write(&owned, "managed content").unwrap();
    std::fs::set_permissions(&owned, std::fs::Permissions::from_mode(0o777)).unwrap();
    symlink(&external_file, managed.join("file-link")).unwrap();
    symlink(&outside, managed.join("directory-link")).unwrap();
    symlink(root.join("missing"), managed.join("dangling-link")).unwrap();
    let resource = ChmodResource::new(managed, mode("700"));

    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    assert_eq!(
        std::fs::metadata(&owned).unwrap().permissions().mode() & MODE_BITS_MASK,
        0o600
    );
    assert_eq!(
        std::fs::metadata(&outside).unwrap().permissions().mode() & MODE_BITS_MASK,
        0o755
    );
    assert_eq!(
        std::fs::metadata(&external_file)
            .unwrap()
            .permissions()
            .mode()
            & MODE_BITS_MASK,
        0o644
    );
    assert_eq!(
        std::fs::read_to_string(external_file).unwrap(),
        "external content"
    );
}
