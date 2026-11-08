//! Existing user state must survive preview, convergence, conflicts and removal.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "isolated lifecycle assertions"
)]

mod common;

fn output(command: &mut std::process::Command, success: bool) -> String {
    let result = command.output().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.status.success(), success, "{text}");
    text
}

#[test]
fn checks_reject_hard_linked_locks_without_overwriting_user_data() {
    let repo = common::IntegrationTestContext::new();
    let home = tempfile::tempdir().unwrap();
    let unrelated = home.path().join("unrelated");
    std::fs::write(&unrelated, "valuable user data").unwrap();
    // Initialize the fixture repository before installing the conflicting lock.
    let _command = common::cli_command(
        repo.root_path(),
        home.path(),
        None,
        "check",
        "config-diagnostics",
    );
    let lock = repo.root_path().join(".git/dotfiles-run.lock");
    std::fs::hard_link(&unrelated, &lock).unwrap();
    let text = output(
        &mut common::cli_command(
            repo.root_path(),
            home.path(),
            None,
            "check",
            "config-diagnostics",
        ),
        false,
    );
    assert!(text.contains("multiply linked"), "{text}");
    assert_eq!(
        std::fs::read_to_string(&unrelated).unwrap(),
        "valuable user data"
    );
    std::fs::remove_file(lock).unwrap();
    output(
        &mut common::cli_command(
            repo.root_path(),
            home.path(),
            None,
            "check",
            "config-diagnostics",
        ),
        true,
    );
    assert_eq!(
        std::fs::read_to_string(unrelated).unwrap(),
        "valuable user data"
    );
}

#[cfg(unix)]
#[test]
fn pam_conflicts_are_reported_by_configuration_diagnostics() {
    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "system-files.toml",
            "[base]\nfiles=[{source='pam.d/test',merge='pam'}]\n",
        )
        .build();
    let home = tempfile::tempdir().unwrap();
    let source = repo.root_path().join("system/pam.d/test");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(
        source,
        "auth optional pam_example.so first\n-auth required pam_example.so second\n",
    )
    .unwrap();
    let text = output(
        &mut common::cli_command(
            repo.root_path(),
            home.path(),
            None,
            "check",
            "config-diagnostics",
        ),
        false,
    );
    assert!(text.contains("conflicting rules"), "{text}");
}

#[cfg(unix)]
#[test]
fn non_interactive_login_shell_never_calls_an_authentication_prompt() {
    use std::os::unix::fs::PermissionsExt as _;
    if nix::unistd::Uid::effective().is_root() {
        return;
    }
    let repo = common::IntegrationTestContext::new();
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let prompt_marker = home.path().join("unexpected-prompt");
    for (name, script) in [
        ("zsh", "exit 0".to_string()),
        (
            "getent",
            "printf 'fixture:x:1000:1000::/fixture:/bin/bash\\n'".to_string(),
        ),
        ("sudo", "exit 1".to_string()),
        (
            "chsh",
            format!("printf prompt > '{}'; exit 0", prompt_marker.display()),
        ),
    ] {
        let path = bin.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut command =
        common::cli_command(repo.root_path(), home.path(), None, "update", "login-shell");
    command
        .env("PATH", &bin)
        .env("USER", "fixture")
        .arg("--fail-on-skip");
    let text = output(&mut command, false);
    assert!(text.contains("authentication"), "{text}");
    assert!(
        !prompt_marker.exists(),
        "non-interactive execution must not invoke chsh"
    );
}

#[test]
fn launcher_preserves_user_files_and_modified_installations() {
    let repo = common::IntegrationTestContext::new();
    let home = tempfile::tempdir().unwrap();
    let name = if cfg!(windows) {
        "dotfiles.cmd"
    } else {
        "dotfiles"
    };
    let target = home.path().join(".local/bin").join(name);
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "user launcher").unwrap();
    for preview in [true, false] {
        let mut command =
            common::cli_command(repo.root_path(), home.path(), None, "update", "launcher");
        command.arg("--fail-on-skip");
        if preview {
            command.arg("--dry-run");
        }
        output(&mut command, false);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "user launcher");
    }
    std::fs::remove_file(&target).unwrap();
    output(
        common::cli_command(repo.root_path(), home.path(), None, "update", "launcher")
            .arg("--dry-run"),
        true,
    );
    assert!(!target.exists(), "preview must not install a launcher");
    output(
        &mut common::cli_command(repo.root_path(), home.path(), None, "update", "launcher"),
        true,
    );
    let installed = std::fs::read(&target).unwrap();
    let text = output(
        &mut common::cli_command(repo.root_path(), home.path(), None, "update", "launcher"),
        true,
    );
    assert!(text.contains("No changes"), "{text}");
    assert_eq!(std::fs::read(&target).unwrap(), installed);
    std::fs::write(&target, "user edit").unwrap();
    output(
        &mut common::cli_command(repo.root_path(), home.path(), None, "remove", "launcher"),
        true,
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "user edit");
    std::fs::write(&target, &installed).unwrap();
    output(
        &mut common::cli_command(repo.root_path(), home.path(), None, "remove", "launcher"),
        true,
    );
    assert!(!target.exists());
}

#[test]
fn included_git_settings_converge_without_modifying_included_files() {
    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "git-config.toml",
            "[base]\nsettings = [{ key = 'core.autocrlf', value = 'false' }]\n",
        )
        .build();
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".gitconfig");
    let included = home.path().join("included.gitconfig");
    std::fs::write(
        &config,
        "[core]\n autocrlf = true\n[include]\n path = included.gitconfig\n",
    )
    .unwrap();
    std::fs::write(&included, "[core]\n autocrlf = true\n").unwrap();
    let before = std::fs::read(&config).unwrap();
    output(
        common::cli_command(repo.root_path(), home.path(), None, "update", "git").arg("--dry-run"),
        true,
    );
    assert_eq!(std::fs::read(&config).unwrap(), before);
    output(
        &mut common::cli_command(repo.root_path(), home.path(), None, "update", "git"),
        true,
    );
    let after = std::fs::read(&config).unwrap();
    let text = output(
        &mut common::cli_command(repo.root_path(), home.path(), None, "update", "git"),
        true,
    );
    assert!(text.contains("No changes"), "{text}");
    assert_eq!(std::fs::read(&config).unwrap(), after);
    assert_eq!(
        std::fs::read_to_string(&included).unwrap(),
        "[core]\n autocrlf = true\n"
    );
    assert_eq!(
        git2::Config::open(&config)
            .unwrap()
            .get_string("core.autocrlf")
            .unwrap(),
        "false"
    );
}

#[cfg(unix)]
#[test]
fn redirected_chmod_is_blocked_until_explicitly_allowed() {
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    let repo = common::IntegrationTestContext::new();
    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let private = outside.path().join("private");
    std::fs::write(&private, "private").unwrap();
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o600)).unwrap();
    symlink(outside.path(), home.path().join(".redirect")).unwrap();
    let config = repo.root_path().join("conf/chmod.toml");
    std::fs::write(
        &config,
        "[base]\npermissions = [{ mode = '755', path = 'redirect' }]\n",
    )
    .unwrap();
    for preview in [true, false] {
        let mut command = common::cli_command(
            repo.root_path(),
            home.path(),
            None,
            "update",
            "file-permissions",
        );
        command.arg("--fail-on-skip");
        if preview {
            command.arg("--dry-run");
        }
        let text = output(&mut command, false);
        assert!(text.contains("allow_external_target"), "{text}");
        assert_eq!(
            private.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    std::fs::write(&config, "[base]\npermissions = [{ mode = '755', path = 'redirect', allow_external_target = true }]\n").unwrap();
    output(
        common::cli_command(
            repo.root_path(),
            home.path(),
            None,
            "update",
            "file-permissions",
        )
        .arg("--dry-run"),
        true,
    );
    assert_eq!(
        private.metadata().unwrap().permissions().mode() & 0o777,
        0o600
    );
    output(
        &mut common::cli_command(
            repo.root_path(),
            home.path(),
            None,
            "update",
            "file-permissions",
        ),
        true,
    );
    assert_eq!(
        private.metadata().unwrap().permissions().mode() & 0o777,
        0o644
    );
    let text = output(
        &mut common::cli_command(
            repo.root_path(),
            home.path(),
            None,
            "update",
            "file-permissions",
        ),
        true,
    );
    assert!(text.contains("No changes"), "{text}");
}

#[cfg(unix)]
#[test]
fn chmod_managed_source_links_converge_in_public_and_overlay_trees() {
    use std::os::unix::fs::PermissionsExt as _;
    for use_overlay in [false, true] {
        let repo = common::IntegrationTestContext::new();
        let overlay = tempfile::tempdir().unwrap();
        let origin = if use_overlay {
            overlay.path()
        } else {
            repo.root_path()
        };
        std::fs::create_dir_all(origin.join("conf")).unwrap();
        std::fs::create_dir_all(origin.join("symlinks/actual")).unwrap();
        std::fs::write(
            origin.join("conf/symlinks.toml"),
            "[base]\nsymlinks = [{ source = 'actual', target = '.custom' }]\n",
        )
        .unwrap();
        std::fs::write(origin.join("conf/chmod.toml"), "[base]\npermissions = [{ mode = '700', path = 'custom' }, { mode = '755', path = 'custom/run' }]\n").unwrap();
        let source = origin.join("symlinks/actual");
        std::fs::write(source.join("config"), "private").unwrap();
        std::fs::write(source.join("run"), "fixture").unwrap();
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(source.join("run"), std::fs::Permissions::from_mode(0o600))
            .unwrap();
        let home = tempfile::tempdir().unwrap();
        let overlay_path = use_overlay.then_some(overlay.path());
        output(
            common::cli_command(
                repo.root_path(),
                home.path(),
                overlay_path,
                "update",
                "symlinks,file-permissions",
            )
            .arg("--fail-on-skip"),
            true,
        );
        assert_eq!(
            source
                .join("config")
                .metadata()
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            source.join("run").metadata().unwrap().permissions().mode() & 0o777,
            0o755
        );
        let text = output(
            common::cli_command(
                repo.root_path(),
                home.path(),
                overlay_path,
                "update",
                "symlinks,file-permissions",
            )
            .arg("--fail-on-skip"),
            true,
        );
        assert!(text.contains("No changes"), "{text}");
    }
}

#[test]
fn permission_source_checks_follow_explicit_and_overlay_directory_mappings() {
    for path in ["custom/config", ".custom/config", "./.custom/config"] {
        let repo = common::TestContextBuilder::new()
            .with_config_file(
                "chmod.toml",
                &format!("[base]\npermissions = [{{ mode = '600', path = '{path}' }}]\n"),
            )
            .build();
        let overlay = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(overlay.path().join("conf")).unwrap();
        std::fs::create_dir_all(overlay.path().join("symlinks/actual")).unwrap();
        std::fs::write(
            overlay.path().join("conf/symlinks.toml"),
            "[base]\nsymlinks = [{ source = 'actual', target = '.custom' }]\n",
        )
        .unwrap();
        let source = overlay.path().join("symlinks/actual/config");
        std::fs::write(&source, "fixture").unwrap();
        let home = tempfile::tempdir().unwrap();
        output(
            &mut common::cli_command(
                repo.root_path(),
                home.path(),
                Some(overlay.path()),
                "check",
                "symlink-sources",
            ),
            true,
        );
        std::fs::remove_file(&source).unwrap();
        let text = output(
            &mut common::cli_command(
                repo.root_path(),
                home.path(),
                Some(overlay.path()),
                "check",
                "symlink-sources",
            ),
            false,
        );
        assert!(text.contains("file-permission source missing"), "{text}");
    }
}

#[test]
fn permission_source_checks_accept_home_public_and_overlay_sources() {
    for location in ["home", "public", "overlay"] {
        let repo = common::TestContextBuilder::new()
            .with_config_file(
                "chmod.toml",
                "[base]\npermissions = [{ mode = '600', path = '.custom' }]\n",
            )
            .build();
        let home = tempfile::tempdir().unwrap();
        let overlay = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(overlay.path().join("symlinks")).unwrap();
        let source = match location {
            "home" => home.path().join(".custom"),
            "public" => repo.root_path().join("symlinks/custom"),
            _ => overlay.path().join("symlinks/custom"),
        };
        std::fs::write(&source, "fixture").unwrap();
        output(
            &mut common::cli_command(
                repo.root_path(),
                home.path(),
                Some(overlay.path()),
                "check",
                "symlink-sources",
            ),
            true,
        );
        std::fs::remove_file(source).unwrap();
        output(
            &mut common::cli_command(
                repo.root_path(),
                home.path(),
                Some(overlay.path()),
                "check",
                "symlink-sources",
            ),
            false,
        );
    }
}

#[test]
fn git_update_lock_contention_retry_and_fresh_home_converge() {
    for include in [false, true] {
        let repo = common::TestContextBuilder::new()
            .with_config_file(
                "git-config.toml",
                "[base]\nsettings = [{ key = 'core.editor', value = 'fixture-editor' }]\n",
            )
            .build();
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(".gitconfig");
        let lock = home.path().join(".gitconfig.lock");
        if include {
            std::fs::write(&path, "[include]\n path = included\n").unwrap();
            std::fs::write(
                home.path().join("included"),
                "[core]\n editor = inherited\n",
            )
            .unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
        }
        std::fs::write(&lock, "foreign writer").unwrap();
        output(
            &mut common::cli_command(repo.root_path(), home.path(), None, "update", "git"),
            false,
        );
        assert_eq!(std::fs::read_to_string(&lock).unwrap(), "foreign writer");
        assert_eq!(
            path.exists(),
            include,
            "a failed first install must leave the config absent"
        );
        std::fs::remove_file(&lock).unwrap();
        output(
            &mut common::cli_command(repo.root_path(), home.path(), None, "update", "git"),
            true,
        );
        let after = std::fs::read(&path).unwrap();
        assert_eq!(
            git2::Config::open(&path)
                .unwrap()
                .get_string("core.editor")
                .unwrap(),
            "fixture-editor"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        }
        output(
            &mut common::cli_command(repo.root_path(), home.path(), None, "update", "git"),
            true,
        );
        assert_eq!(std::fs::read(&path).unwrap(), after);
        assert!(!lock.exists());
    }
}
