use super::*;

#[test]
fn unsafe_elevated_child_scope_is_rejected_before_startup_writes() {
    for (verb, selector, inherited_marker) in [
        ("update", "symlinks", false),
        ("update", "registry", false),
        ("remove", "symlinks", false),
        ("update", "launcher", true),
        ("check", "config-files", true),
    ] {
        let fixture = common::IntegrationTestContext::new();
        let home = tempfile::tempdir().unwrap();
        // If the startup guard regresses, configuration loading must still
        // prevent native registry or other task mutations in this fixture.
        std::fs::write(
            fixture.root_path().join("conf/symlinks.toml"),
            "invalid = [",
        )
        .unwrap();
        let mut command =
            common::cli_command(fixture.root_path(), home.path(), None, verb, selector);
        if inherited_marker {
            command.env("DOTFILES_ELEVATED_CHILD", "1");
        } else {
            command.arg("--elevated-child");
        }
        let repository = git2::Repository::open(fixture.root_path()).unwrap();
        repository
            .config()
            .unwrap()
            .set_str("dotfiles.profile", "original-profile")
            .unwrap();
        let config_path = repository.path().join("config");
        let original_config = std::fs::read(&config_path).unwrap();

        let result = command.output().unwrap();
        let output = common::combined_output(&result);
        assert!(!result.status.success(), "{verb}/{selector}: {output}");
        assert!(
            output.contains("elevated children"),
            "{verb}/{selector}: {output}"
        );
        assert_eq!(
            std::fs::read(config_path).unwrap(),
            original_config,
            "rejected scope must not persist a different profile"
        );
        assert_eq!(
            std::fs::read_dir(home.path()).unwrap().count(),
            0,
            "rejected scope must not create user settings, logs, or run locks"
        );
    }
}

#[test]
fn selected_overlay_marker_precedes_config_and_blocks_managed_removal() {
    let main = common::IntegrationTestContext::new();
    let root = main.root_path();
    let home = tempfile::tempdir().unwrap();
    let overlay = tempfile::tempdir().unwrap();
    let marker = git2::Repository::init(overlay.path())
        .unwrap()
        .path()
        .join("dotfiles-merge-in-progress");
    // Even an empty marker is evidence of an unfinished update.
    std::fs::write(&marker, "").unwrap();
    let run = |verb, only| common::cli_command(root, home.path(), None, verb, only);
    let install = run("update", "launcher").output().unwrap();
    assert!(install.status.success(), "unselected marker: {install:?}");
    let launcher = home.path().join(".local/bin").join(if cfg!(windows) {
        "dotfiles.cmd"
    } else {
        "dotfiles"
    });
    let original = std::fs::read(&launcher).unwrap();
    let repo = git2::Repository::open(root).unwrap();
    let repo_config = repo.path().join("config");
    let original_config = std::fs::read(&repo_config).unwrap();
    let blocked = |cmd: &mut std::process::Command| {
        let out = cmd.output().unwrap();
        assert!(!out.status.success(), "{out:?}");
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(
            error.contains("Unverified repository merge")
                && error.contains(marker.to_str().unwrap()),
            "{out:?}"
        );
        assert_eq!(std::fs::read(&launcher).unwrap(), original);
        assert!(marker.is_file());
    };
    blocked(
        run("remove", "launcher")
            .arg("--overlay")
            .arg(overlay.path()),
    );
    assert_eq!(
        std::fs::read(&repo_config).unwrap(),
        original_config,
        "rejected overlay must not be persisted"
    );

    repo.config()
        .unwrap()
        .set_str("dotfiles.overlay", overlay.path().to_str().unwrap())
        .unwrap();
    let check = run("check", "config-diagnostics")
        .arg("--no-symbols")
        .output()
        .unwrap();
    assert!(!check.status.success(), "marked checkout: {check:?}");
    let stdout = String::from_utf8_lossy(&check.stdout);
    let stderr = String::from_utf8_lossy(&check.stderr);
    assert!(
        stdout.contains("PASSED Configuration diagnostics"),
        "{check:?}"
    );
    assert!(stderr.contains("Unverified repository merge"), "{check:?}");
    assert!(stderr.contains(marker.to_str().unwrap()), "{check:?}");
    assert!(marker.is_file());
    std::fs::write(root.join("conf/symlinks.toml"), "[broken").unwrap();
    blocked(&mut run("check", "config-diagnostics"));
    blocked(&mut run("update", "symlinks"));
    blocked(
        common::isolated_cli_command(root, home.path(), None, "update", "repository", true)
            .arg("--dry-run"),
    );
}

#[test]
fn shared_home_lock_blocks_other_checkouts_before_configuration_mutations() {
    use std::io::{Read as _, Seek as _};

    let first = common::IntegrationTestContext::new();
    let second = common::IntegrationTestContext::new();
    let home = tempfile::tempdir().unwrap();
    let launcher = home.path().join(".local/bin").join(if cfg!(windows) {
        "dotfiles.cmd"
    } else {
        "dotfiles"
    });
    let lock_path = if cfg!(windows) {
        home.path().join("AppData/Local/dotfiles/home-run.lock")
    } else {
        home.path().join(".local/state/dotfiles/home-run.lock")
    };
    std::fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
    let mut lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .unwrap();
    lock.try_lock().unwrap();
    let contents = b"fixture owner";
    std::io::Write::write_all(&mut lock, contents).unwrap();

    for repo in [&first, &second] {
        for (verb, selector) in [
            ("update", "launcher"),
            ("remove", "launcher"),
            ("check", "config-files"),
        ] {
            let mut command =
                common::cli_command(repo.root_path(), home.path(), None, verb, selector);
            command.env("XDG_STATE_HOME", home.path().join("other-state"));
            command.env("LOCALAPPDATA", home.path().join("other-local"));
            let result = command.output().unwrap();
            let output = common::combined_output(&result);
            assert!(!result.status.success(), "{verb}: {output}");
            assert!(output.contains("another dotfiles run"), "{verb}: {output}");
            assert!(output.contains("home-run.lock"), "{verb}: {output}");
            assert!(
                !launcher.exists(),
                "a blocked command must not install the launcher"
            );
        }
    }
    lock.rewind().unwrap();
    assert_eq!(
        lock.metadata().unwrap().len(),
        u64::try_from(contents.len()).unwrap()
    );
    let mut owner = vec![0; contents.len()];
    lock.read_exact(&mut owner).unwrap();
    assert_eq!(owner, contents, "contenders must preserve lock metadata");
    drop(lock);
    let result = common::cli_command(second.root_path(), home.path(), None, "update", "launcher")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        launcher.is_file(),
        "retry must install the launcher after the lock is released"
    );
}

#[test]
fn nested_cli_fixture_preserves_ancestor_settings_and_run_lock() {
    use std::io::{Read as _, Seek as _, Write as _};

    let ancestor = common::TestContextBuilder::new()
        .with_git_hooks_dir()
        .build();
    let repository = git2::Repository::open(ancestor.root_path()).unwrap();
    let mut settings = repository.config().unwrap();
    settings.set_str("dotfiles.profile", "desktop").unwrap();
    settings
        .set_str("dotfiles.overlay", "ancestor-overlay-sentinel")
        .unwrap();
    drop(settings);
    let config_path = repository.path().join("config");
    let original_config = std::fs::read(&config_path).unwrap();
    let lock_path = repository.path().join("dotfiles-run.lock");
    let original_lock = b"ancestor run-lock sentinel\n";
    let mut lock = std::fs::OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    lock.write_all(original_lock).unwrap();
    lock.sync_all().unwrap();

    for locked in [false, true] {
        if locked {
            lock.try_lock().expect("hold the ancestor's run lock");
        }
        let root = ancestor.root_path().join(format!("nested-{locked}"));
        common::setup_minimal_repo(&root);
        assert_eq!(
            dunce::canonicalize(git2::Repository::discover(&root).unwrap().path()).unwrap(),
            dunce::canonicalize(repository.path()).unwrap(),
            "before isolation, this fixture must discover the ancestor"
        );
        let home = tempfile::tempdir().unwrap();
        let overlay = tempfile::tempdir().unwrap();
        let output = common::cli_command(
            &root,
            home.path(),
            Some(overlay.path()),
            "update",
            "completions",
        )
        .output()
        .unwrap();
        assert!(
            output.status.success(),
            "ancestor_locked={locked}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let child = git2::Repository::open(&root).unwrap();
        let child_settings = child.config().unwrap();
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("base profile"),
            "the child must use its requested profile rather than the ancestor's desktop selection"
        );
        assert_eq!(
            child_settings.get_string("dotfiles.overlay").unwrap(),
            overlay.path().to_string_lossy(),
            "the explicit overlay must be persisted only in the child"
        );
        assert!(child.path().join("dotfiles-run.lock").is_file());
        assert_eq!(
            std::fs::read(&config_path).unwrap(),
            original_config,
            "ancestor_locked={locked}: ancestor settings must remain byte-for-byte unchanged"
        );
        lock.rewind().unwrap();
        assert_eq!(
            lock.metadata().unwrap().len(),
            u64::try_from(original_lock.len()).unwrap(),
            "ancestor_locked={locked}: ancestor lock length must not change"
        );
        let mut contents = vec![0; original_lock.len()];
        lock.read_exact(&mut contents).unwrap();
        assert_eq!(
            contents, original_lock,
            "ancestor_locked={locked}: ancestor lock must not be overwritten"
        );
        if locked {
            lock.unlock().unwrap();
        }
    }
}

#[cfg(unix)]
#[test]
fn update_console_separates_tasks_and_keeps_no_op_compact() {
    use std::os::unix::fs::PermissionsExt as _;

    for verbose in [false, true] {
        for symbols in [false, true] {
            let repo = common::TestContextBuilder::new()
                .with_config_file("symlinks.toml", "[base]\nsymlinks = [\"example\"]\n")
                .with_symlink_source_content("example", "example\n")
                .with_config_file(
                    "chmod.toml",
                    "[base]\npermissions = [{ path = \"example\", mode = \"600\" }]\n",
                )
                .build();
            std::fs::set_permissions(
                repo.root_path().join("symlinks/example"),
                std::fs::Permissions::from_mode(0o644),
            )
            .unwrap();
            let home = tempfile::tempdir().unwrap();
            let overlay = tempfile::tempdir().unwrap();
            let mut command = common::cli_command(
                repo.root_path(),
                home.path(),
                Some(overlay.path()),
                "update",
                "symlinks,file-permissions",
            );
            if verbose {
                command.arg("--verbose");
            }
            if !symbols {
                command.arg("--no-symbols");
            }

            // Both runs mutate only the temporary repository and home.
            // The second run exercises hidden current rows and verbose rows.
            for changed in [true, false] {
                let output = command.output().expect("run isolated update");
                let text = String::from_utf8(output.stdout).unwrap();
                assert!(
                    output.status.success(),
                    "{text}\n{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(
                    !text.contains('\u{1b}'),
                    "piped output must be plain: {text:?}"
                );
                assert!(!text.contains("\n\n\n"), "extra blank line: {text:?}");
                let blocks: Vec<_> = text.trim().split("\n\n").collect();
                assert_eq!(
                    blocks.len(),
                    if changed || verbose { 4 } else { 2 },
                    "{text}"
                );
                if changed || verbose {
                    let status = match (changed, symbols) {
                        (true, true) => "✓",
                        (true, false) => "CHANGED",
                        (false, true) => "○",
                        (false, false) => "UNCHANGED",
                    };
                    for (block, name) in blocks[1..3]
                        .iter()
                        .zip(["Home symlinks", "File permissions"])
                    {
                        assert!(block.starts_with(&format!("{status} {name}")), "{text}");
                    }
                }
                let summary = if changed {
                    "Tasks · 2 changed"
                } else {
                    "Tasks · 2 unchanged"
                };
                assert!(blocks.last().unwrap().starts_with(summary), "{text}");
            }
        }
    }
}

#[test]
fn conflicting_overlay_desired_state_stops_update_before_selected_tasks_run() {
    let mut cases = vec![(
        "git-config.toml",
        "[base]\nsettings = [{ key = \"core.editor\", value = \"vim\" }]\n",
        "[base]\nsettings = [\
         { key = \"core.editor\", value = \"vim\" },\
         { key = \"CORE.EDITOR\", value = \"nano\" }]\n",
        "git.conflicting-values",
    )];
    if cfg!(windows) {
        cases.push((
            "registry.toml",
            "[console]\npath = 'HKCU:\\Console'\n[console.values]\nFontSize = 14\n",
            "[console]\npath = 'HKCU:\\Console'\n[console.values]\nFontSize = 14\n\
             [other]\npath = 'hkcu:\\console'\n[other.values]\nfontsize = 15\n",
            "registry.conflicting-values",
        ));
    }
    for (file, main, overlay_content, code) in cases {
        for dry_run in [false, true] {
            let repo = common::TestContextBuilder::new()
                .with_config_file(file, main)
                .with_config_file(
                    "symlinks.toml",
                    "[base]\nsymlinks = [\"conflict-sentinel\"]\n",
                )
                .with_symlink_source_content("conflict-sentinel", "must not be installed")
                .build();
            let overlay = tempfile::tempdir().unwrap();
            let home = tempfile::tempdir().unwrap();
            std::fs::create_dir(overlay.path().join("conf")).unwrap();
            std::fs::write(overlay.path().join("conf").join(file), overlay_content).unwrap();
            let mut command = common::cli_command(
                repo.root_path(),
                home.path(),
                Some(overlay.path()),
                "update",
                "symlinks",
            );
            let git_config = git2::Repository::open(repo.root_path())
                .unwrap()
                .path()
                .join("config");
            let saved = std::fs::read(&git_config).unwrap();

            if dry_run {
                command.arg("--dry-run");
            }
            let output = command.output().expect("run isolated update");
            let text = common::combined_output(&output);
            assert!(
                !output.status.success(),
                "{file}, dry_run={dry_run}: {text}"
            );
            assert!(text.contains(code), "{file}, dry_run={dry_run}: {text}");
            assert_eq!(
                std::fs::read(&git_config).unwrap(),
                saved,
                "a failed load must leave the saved overlay unchanged"
            );
            assert!(
                text.contains(&overlay.path().join("conf").join(file).display().to_string()),
                "{text}"
            );
            assert!(
                home.path()
                    .join(".conflict-sentinel")
                    .symlink_metadata()
                    .is_err(),
                "even an unrelated selected task must not mutate before validation succeeds"
            );
        }
    }
}
