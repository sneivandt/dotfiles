#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::wildcard_imports,
    clippy::indexing_slicing,
    reason = "panicking allowed at this trust boundary"
)]
//! Integration tests for the `remove` command.
//!
//! These tests verify the structure and naming of the remove task list
//! returned by [`all_remove_tasks`].

mod common;

use dotfiles_cli::testing as test_api;

use test_api::config::ConfigStore;
use test_api::platform::{Os, Platform};
use test_api::tasks;

/// Build a remove task list backed by a store loaded from a minimal repo.
fn remove_tasks() -> Vec<Box<dyn tasks::Task>> {
    remove_tasks_for_platform(Platform::detect())
}

fn remove_tasks_for_platform(platform: Platform) -> Vec<Box<dyn tasks::Task>> {
    let ctx = common::IntegrationTestContext::new();
    let store = ConfigStore::from_config(ctx.load_config_for_platform("base", platform));
    tasks::all_remove_tasks(&store, false)
}

// ---------------------------------------------------------------------------
// Structural invariants
// ---------------------------------------------------------------------------

#[test]
fn remove_task_catalog_satisfies_structural_contract() {
    let tasks = remove_tasks();
    common::assert_task_catalog_contract("remove", &tasks);
}

// ---------------------------------------------------------------------------
// Expected task presence
// ---------------------------------------------------------------------------

#[test]
fn remove_task_catalog_contains_required_tasks() {
    let tasks = remove_tasks();
    let selectors: Vec<&str> = tasks.iter().map(|task| task.selector()).collect();
    for required in ["symlinks", "git-hooks"] {
        assert!(
            selectors.contains(&required),
            "remove task catalog is missing required selector '{required}'"
        );
    }
}

// ---------------------------------------------------------------------------
// Dry-run: task list from a minimal repository
// ---------------------------------------------------------------------------

#[test]
fn remove_catalog_applicability_tracks_managed_inputs_on_both_platforms() {
    let platforms = [
        Platform {
            os: Os::Linux,
            is_arch: false,
            is_wsl: false,
        },
        Platform {
            os: Os::Windows,
            is_arch: false,
            is_wsl: false,
        },
    ];

    for platform in platforms {
        for configured in [false, true] {
            let mut builder = common::TestContextBuilder::new();
            if configured {
                builder = builder
                    .with_config_file("symlinks.toml", "[base]\nsymlinks = ['example']\n")
                    .with_symlink_source("example")
                    .with_git_hooks_dir();
            }
            let ctx = builder.build();
            let ec = ctx.make_context_with_executor(
                "base",
                platform,
                tasks::ContextOpts {
                    dry_run: true,
                    parallel: false,
                    is_ci: Some(false),
                },
                std::sync::Arc::new(common::StubExecutor),
            );
            let mut applicable: Vec<_> = tasks::all_remove_tasks(&ec.store, false)
                .into_iter()
                .filter(|task| task.should_run(&ec.ctx))
                .map(|task| task.selector().to_string())
                .collect();
            applicable.sort_unstable();
            let expected = if configured {
                vec!["git-hooks", "launcher", "symlinks"]
            } else {
                vec!["launcher"]
            };
            assert_eq!(
                applicable, expected,
                "{platform:?}, configured={configured}"
            );
        }
    }
}

#[test]
fn invalid_remove_selection_fails_without_removing_managed_hooks() {
    for (selector, skip, diagnostic) in [
        ("unknown-task", None, "--only did not match a task selector"),
        ("git-hooks", Some("git-hooks"), "selected no tasks"),
    ] {
        let repo = common::TestContextBuilder::new()
            .with_hook_source("pre-commit", "#!/bin/sh\nexit 0\n")
            .with_git_hooks_dir()
            .build();
        let hook = repo
            .root_path()
            .join(".git")
            .join("hooks")
            .join("pre-commit");
        std::fs::write(&hook, "#!/bin/sh\nexit 0\n").unwrap();
        let home = tempfile::tempdir().unwrap();
        let mut command =
            common::cli_command(repo.root_path(), home.path(), None, "remove", selector);
        if let Some(skip) = skip {
            command.args(["--skip", skip]);
        }
        let output = command.output().unwrap();
        let text = common::combined_output(&output);
        assert!(!output.status.success(), "{selector}: {text}");
        assert!(text.contains(diagnostic), "{selector}: {text}");
        assert_eq!(
            std::fs::read_to_string(hook).unwrap(),
            "#!/bin/sh\nexit 0\n",
            "{selector}: invalid selection must not remove hooks"
        );
    }
}

#[test]
fn update_and_remove_leave_inactive_profile_links_untouched() {
    for parallel in [false, true] {
        let repo = common::TestContextBuilder::new()
            .with_config_file(
                "symlinks.toml",
                "[base]\nsymlinks = ['active']\n\
                 [desktop]\nsymlinks = ['inactive', 'inactive-missing']\n",
            )
            .with_symlink_source_content("active", "active source\n")
            .with_symlink_source_content("inactive", "inactive source\n")
            .with_symlink_source_content("inactive-missing", "not installed\n")
            .build();
        // Use the same temporary root as the test repository. The Windows
        // runner's checkout and temp volume have different inherited ACLs.
        let home = tempfile::tempdir().unwrap();
        let home_path = dunce::canonicalize(home.path()).unwrap();
        for source in ["active", "inactive"] {
            let source_path = repo.root_path().join("symlinks").join(source);
            let target = home_path.join(format!(".{source}"));
            #[cfg(unix)]
            std::os::unix::fs::symlink(&source_path, &target).unwrap();
            #[cfg(windows)]
            std::os::windows::fs::symlink_file(&source_path, &target)
                .expect("native file symlinks require Developer Mode or an elevated test worker");
        }
        let active = home_path.join(".active");
        let inactive = home_path.join(".inactive");

        for (verb, dry_run, materialized) in [
            ("update", false, false),
            ("remove", true, false),
            ("remove", false, true),
            ("remove", false, true),
        ] {
            let mut command =
                common::cli_command(repo.root_path(), &home_path, None, verb, "symlinks");
            if dry_run {
                command.arg("--dry-run");
            }
            if !parallel {
                command.arg("--no-parallel");
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{verb}, dry_run={dry_run}, parallel={parallel}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                std::fs::symlink_metadata(&active)
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                !materialized,
                "{verb}, dry_run={dry_run}: only explicit removal materializes active links"
            );
            assert_eq!(std::fs::read(&active).unwrap(), b"active source\n");
            assert!(
                std::fs::symlink_metadata(&inactive)
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                "{verb}, dry_run={dry_run}: selecting base must not uninstall desktop state"
            );
            assert_eq!(
                dunce::canonicalize(&inactive).unwrap(),
                dunce::canonicalize(repo.root_path().join("symlinks/inactive")).unwrap(),
                "{verb}: the inactive link must retain its source"
            );
            assert_eq!(std::fs::read(&inactive).unwrap(), b"inactive source\n");
            assert!(
                home_path
                    .join(".inactive-missing")
                    .symlink_metadata()
                    .is_err(),
                "{verb}: removal must not materialize an inactive missing target"
            );
            for (source, expected) in [
                ("active", "active source\n"),
                ("inactive", "inactive source\n"),
                ("inactive-missing", "not installed\n"),
            ] {
                assert_eq!(
                    std::fs::read_to_string(repo.root_path().join("symlinks").join(source))
                        .unwrap(),
                    expected,
                    "{verb}: managed sources must not be modified"
                );
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn remove_removes_active_overlay_script_state_by_default() {
    let repo = common::IntegrationTestContext::new();
    let overlay = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(overlay.path().join("conf")).unwrap();
    std::fs::create_dir_all(overlay.path().join("scripts")).unwrap();
    std::fs::write(
        overlay.path().join("conf/scripts.toml"),
        "[base]\nscripts = [{ name = 'Private tools', path = 'scripts/tools.sh' }]\n",
    )
    .unwrap();
    std::fs::write(overlay.path().join("state"), "managed\n").unwrap();
    std::fs::write(
        overlay.path().join("scripts/tools.sh"),
        "#!/bin/sh\ncase \"$1\" in\n  --check) test -f state ;;\n  --remove) rm state ;;\n  --dryrun) touch unexpected-dryrun ;;\n  *) exit 2 ;;\nesac\n",
    )
    .unwrap();

    let run = |dry_run| {
        let mut command = common::cli_command(
            repo.root_path(),
            home.path(),
            Some(overlay.path()),
            "remove",
            "script-private-tools",
        );
        if dry_run {
            command.arg("--dry-run");
        }
        command.output().unwrap()
    };

    let preview = run(true);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    assert!(overlay.path().join("state").exists());
    assert!(!overlay.path().join("unexpected-dryrun").exists());

    let removed = run(false);
    assert!(
        removed.status.success(),
        "{}",
        String::from_utf8_lossy(&removed.stderr)
    );
    assert!(!overlay.path().join("state").exists());

    let repeated = run(false);
    assert!(
        repeated.status.success(),
        "{}",
        String::from_utf8_lossy(&repeated.stderr)
    );
}

#[test]
fn missing_symlink_targets_are_only_materialized_on_request() {
    let repo = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", "[base]\nsymlinks = ['fixture']\n")
        .with_symlink_source_content("fixture", "configured content\n")
        .build();
    let home = tempfile::tempdir().unwrap();
    let target = home.path().join(".fixture");
    for restore in [false, true] {
        for preview in [true, false] {
            let mut command =
                common::cli_command(repo.root_path(), home.path(), None, "remove", "symlinks");
            command.arg("--no-symbols");
            if restore {
                command.arg("--restore-missing");
            }
            if preview {
                command.arg("--dry-run");
            }
            let output = command.output().unwrap();
            assert!(output.status.success(), "{output:?}");
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert_eq!(target.exists(), restore && !preview);
            assert_eq!(
                stdout.contains("copy ~/.fixture from symlinks/fixture"),
                restore,
                "{stdout}",
            );
            assert!(!stdout.contains("materialize"), "{stdout}");
            assert!(!stdout.contains(" · planned"), "{stdout}");
        }
    }
    assert_eq!(
        std::fs::read_to_string(target).unwrap(),
        "configured content\n"
    );
}

#[test]
fn removal_only_plans_and_copies_links_directly_to_the_checkout() {
    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "symlinks.toml",
            "[base]\nsymlinks = [{source='settings',target='.first'}, {source='settings/nested',target='.second'}, {source='settings',target='.third'}]\n",
        )
        .with_symlink_source_content("settings/nested", "original")
        .build();
    let home = tempfile::tempdir().unwrap();
    let first = home.path().join(".first");
    let second = home.path().join(".second");
    let third = home.path().join(".third");
    let source = repo.root_path().join("symlinks/settings");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&source, &first).unwrap();
        std::os::unix::fs::symlink(".first/nested", &second).unwrap();
        std::os::unix::fs::symlink(".first", &third).unwrap();
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(&source, &first).unwrap();
        std::os::windows::fs::symlink_file(r".first\nested", &second).unwrap();
        std::os::windows::fs::symlink_dir(".first", &third).unwrap();
    }

    let update = common::cli_command(repo.root_path(), home.path(), None, "update", "symlinks")
        .arg("--dry-run")
        .output()
        .unwrap();
    assert!(!update.status.success(), "{update:?}");
    for (preview, copies) in [(true, true), (false, true), (false, false)] {
        let mut command =
            common::cli_command(repo.root_path(), home.path(), None, "remove", "symlinks");
        command.args(["--no-parallel", "--verbose"]);
        if preview {
            command.arg("--dry-run");
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert_eq!(stdout.contains("copy ~/.first"), copies, "{stdout}");
        assert!(!stdout.contains("copy ~/.second"), "{stdout}");
        assert!(!stdout.contains("copy ~/.third"), "{stdout}");
        assert_eq!(first.is_symlink(), preview);
        assert_eq!(
            std::fs::read_link(&second).unwrap(),
            std::path::Path::new(".first/nested")
        );
        assert_eq!(
            std::fs::read_link(&third).unwrap(),
            std::path::Path::new(".first")
        );
    }
    std::fs::write(first.join("nested"), "independent content").unwrap();
    assert_eq!(
        std::fs::read_to_string(source.join("nested")).unwrap(),
        "original"
    );
}

#[test]
fn removal_preserves_hard_links_within_the_copy_and_severs_source_identity() {
    let repo = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", "[base]\nsymlinks = ['directory']\n")
        .with_symlink_source_content("directory/nested/primary", "original")
        .build();
    let home = tempfile::tempdir().unwrap();
    let source = repo.root_path().join("symlinks/directory");
    std::fs::hard_link(source.join("nested/primary"), source.join("alias")).unwrap();
    let target = home.path().join(".directory");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&source, &target).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&source, &target).unwrap();
    let output = common::cli_command(repo.root_path(), home.path(), None, "remove", "symlinks")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(target.is_dir() && !target.is_symlink());
    std::fs::write(target.join("nested/primary"), "local update").unwrap();
    assert_eq!(
        std::fs::read_to_string(target.join("alias")).unwrap(),
        "local update"
    );
    assert_eq!(
        std::fs::read_to_string(source.join("alias")).unwrap(),
        "original"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn removal_preserves_sparse_extents_in_top_level_and_nested_files() {
    use std::io::{Seek as _, SeekFrom, Write as _};
    use std::os::unix::fs::{MetadataExt as _, symlink};

    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "symlinks.toml",
            "[base]\nsymlinks=['sparse', 'directory']\n",
        )
        .with_symlink_source_content("directory/fixture", "fixture")
        .build();
    let home = tempfile::tempdir().unwrap();
    let sources = repo.root_path().join("symlinks");
    for path in ["sparse", "directory/nested"] {
        let source = sources.join(path);
        let mut file = std::fs::File::create(source).unwrap();
        file.write_all(b"first extent").unwrap();
        file.seek(SeekFrom::Start(2 * 1024 * 1024)).unwrap();
        file.write_all(b"second extent").unwrap();
        file.set_len(4 * 1024 * 1024).unwrap();
    }
    symlink(sources.join("sparse"), home.path().join(".sparse")).unwrap();
    symlink(sources.join("directory"), home.path().join(".directory")).unwrap();
    let result = common::cli_command(repo.root_path(), home.path(), None, "remove", "symlinks")
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    for (source, target) in [
        ("sparse", ".sparse"),
        ("directory/nested", ".directory/nested"),
    ] {
        let source = sources.join(source);
        let target = home.path().join(target);
        assert_eq!(
            std::fs::read(&target).unwrap(),
            std::fs::read(&source).unwrap()
        );
        if rustix::fs::seek(
            std::fs::File::open(&source).unwrap(),
            rustix::fs::SeekFrom::Hole(0),
        )
        .is_ok_and(|offset| offset < source.metadata().unwrap().len())
        {
            assert!(target.metadata().unwrap().blocks() * 512 < target.metadata().unwrap().len());
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn materialization_preserves_linux_acls_and_extended_attributes() {
    use rustix::fs::{XattrFlags, getxattr, setxattr};
    use std::os::unix::fs::{MetadataExt as _, symlink};

    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "symlinks.toml",
            "[base]\nsymlinks = ['single', 'directory', 'plain']\n",
        )
        .with_symlink_source_content("single", "top-level content")
        .with_symlink_source_content("directory/nested", "nested content")
        .with_symlink_source_content("plain", "content without an ACL")
        .build();
    let home = tempfile::tempdir().unwrap();
    let source = repo.root_path().join("symlinks");
    let directory = source.join("directory");
    let uid = directory.metadata().unwrap().uid();
    // Linux POSIX ACL xattrs encode a version followed by tag/permission/ID
    // entries. The group ACL entry is intentionally different from the mask.
    let acl = |owner, named, mask| {
        let mut bytes = 2_u32.to_le_bytes().to_vec();
        for (tag, permissions, id) in [
            (1_u16, owner, u32::MAX),
            (2, named, uid),
            (4, 0, u32::MAX),
            (16, mask, u32::MAX),
            (32, 0, u32::MAX),
        ] {
            bytes.extend(tag.to_le_bytes());
            bytes.extend(u16::to_le_bytes(permissions));
            bytes.extend(id.to_le_bytes());
        }
        bytes
    };
    let file_acl = acl(6, 4, 4);
    let directory_acl = acl(7, 5, 5);
    let attribute = "user.dotfiles_fixture";
    let access = "system.posix_acl_access";
    let default = "system.posix_acl_default";
    for path in [source.join("single"), directory.join("nested")] {
        setxattr(&path, access, &file_acl, XattrFlags::empty()).unwrap();
        setxattr(&path, attribute, b"fixture metadata", XattrFlags::empty()).unwrap();
    }
    for name in [access, default] {
        setxattr(&directory, name, &directory_acl, XattrFlags::empty()).unwrap();
    }
    setxattr(home.path(), default, &directory_acl, XattrFlags::empty()).unwrap();
    symlink(source.join("single"), home.path().join(".single")).unwrap();
    symlink(&directory, home.path().join(".directory")).unwrap();
    symlink(source.join("plain"), home.path().join(".plain")).unwrap();
    let output = common::cli_command(repo.root_path(), home.path(), None, "remove", "symlinks")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let read_attribute = |path: &std::path::Path, name| {
        let mut buffer = vec![0; 128];
        let size = getxattr(path, name, buffer.as_mut_slice()).unwrap();
        buffer.truncate(size);
        buffer
    };
    let copied_directory = home.path().join(".directory");
    for name in [access, default] {
        assert_eq!(read_attribute(&copied_directory, name), directory_acl);
    }
    for path in [home.path().join(".single"), copied_directory.join("nested")] {
        assert_eq!(read_attribute(&path, access), file_acl);
        assert_eq!(read_attribute(&path, attribute), b"fixture metadata");
    }
    assert!(!copied_directory.is_symlink());
    assert!(!home.path().join(".single").is_symlink());
    assert_eq!(
        getxattr(home.path().join(".plain"), access, &mut [0; 128][..]),
        Err(rustix::io::Errno::NODATA),
        "a copy must not inherit a destination ACL absent from the source"
    );
}

#[test]
fn materialization_refuses_nested_absolute_links_to_the_active_overlay() {
    let repo = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", "[base]\nsymlinks = ['fixture']\n")
        .with_symlink_source_content("fixture/file", "content")
        .build();
    let overlay = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let source = repo.root_path().join("symlinks/fixture");
    let target = home.path().join(".fixture");
    let external_source = overlay.path().join("external");
    std::fs::write(&external_source, "overlay content").unwrap();
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(&source, &target).unwrap();
        std::os::windows::fs::symlink_file(&external_source, source.join("nested")).unwrap();
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&source, &target).unwrap();
        std::os::unix::fs::symlink(&external_source, source.join("nested")).unwrap();
    }
    for preview in [true, false] {
        let mut command = common::cli_command(
            repo.root_path(),
            home.path(),
            Some(overlay.path()),
            "remove",
            "symlinks",
        );
        if preview {
            command.arg("--dry-run");
        }
        let output = command.output().unwrap();
        assert!(!output.status.success(), "{output:?}");
        let text = common::combined_output(&output);
        assert!(text.contains("depends on the checkout"), "{text}");
        assert!(std::fs::read_link(&target).is_ok());
        assert_eq!(
            dunce::canonicalize(&target).unwrap(),
            dunce::canonicalize(&source).unwrap()
        );
    }
}
