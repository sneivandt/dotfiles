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
fn missing_overlay_cannot_replace_the_saved_selection_or_apply_base_settings() {
    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "git-config.toml",
            "[base]\nsettings=[{key='core.editor',value='vim'}]\n",
        )
        .build();
    let home = tempfile::tempdir().unwrap();
    let overlay = tempfile::tempdir().unwrap();
    std::fs::create_dir(overlay.path().join("conf")).unwrap();
    std::fs::write(
        overlay.path().join("conf/git-config.toml"),
        "[base]\nsettings=[{key='core.editor',value='nano'}]\n",
    )
    .unwrap();
    let run = |selected_overlay, success| {
        output(
            &mut common::cli_command(
                repo.root_path(),
                home.path(),
                selected_overlay,
                "update",
                "git",
            ),
            success,
        )
    };
    run(Some(overlay.path()), true);
    let message = run(Some(&overlay.path().join("missing")), false);
    assert!(
        message.contains("cannot read overlay directory"),
        "{message}"
    );
    let checkout = git2::Repository::open(repo.root_path()).unwrap();
    assert_eq!(
        checkout
            .config()
            .unwrap()
            .get_path("dotfiles.overlay")
            .unwrap(),
        overlay.path()
    );
    let global = git2::Config::open(&home.path().join(".gitconfig")).unwrap();
    assert_eq!(global.get_string("core.editor").unwrap(), "nano");
}

#[cfg(unix)]
#[test]
fn nonregular_log_entries_cannot_block_update() {
    let repo = common::IntegrationTestContext::new();
    let home = tempfile::tempdir().unwrap();
    let logs = home.path().join("logs");
    std::fs::create_dir(&logs).unwrap();
    let fifo = logs.join("20250101T010101Z-update-7.log");
    nix::unistd::mkfifo(&fifo, nix::sys::stat::Mode::S_IRUSR).unwrap();
    let mut update = common::cli_command(repo.root_path(), home.path(), None, "update", "git")
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if let Some(status) = update.try_wait().unwrap() {
            assert!(status.success(), "update failed with {status}");
            assert!(
                fifo.exists(),
                "retention must leave nonregular log entries alone"
            );
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    update.kill().unwrap();
    let status = update.wait().unwrap();
    assert!(
        status.success(),
        "update stalled while scanning nonregular logs"
    );
}

#[cfg(unix)]
#[test]
fn shell_path_migrates_the_legacy_export_before_an_early_return() {
    let repo = common::IntegrationTestContext::new();
    let home = tempfile::tempdir().unwrap();
    let profile = home.path().join(".profile");
    let original = "#!/bin/sh\nexport FIXTURE_SETTING=kept\nreturn 0\n";
    let export = "export PATH=\"$HOME/.local/bin:$PATH\"";
    let legacy = format!("{original}\n# Added by dotfiles\n{export}\n");
    std::fs::write(&profile, &legacy).unwrap();
    let update = || {
        common::cli_command(
            repo.root_path(),
            home.path(),
            None,
            "update",
            "launcher,path",
        )
    };
    output(update().arg("--dry-run"), true);
    assert_eq!(std::fs::read_to_string(&profile).unwrap(), legacy);
    output(&mut update(), true);
    let configured = std::fs::read_to_string(&profile).unwrap();
    assert_eq!(configured.matches(export).count(), 2);
    assert!(configured.starts_with("#!/bin/sh\n"));
    assert!(configured.contains("export FIXTURE_SETTING=kept\nreturn 0\n"));
    let shell = std::process::Command::new("/bin/sh")
        .env_clear()
        .env("HOME", home.path())
        .env("PATH", "/usr/bin:/bin")
        .args([
            "-c",
            ". \"$HOME/.profile\"; test \"$FIXTURE_SETTING\" = kept && command -v dotfiles",
        ])
        .output()
        .unwrap();
    assert!(shell.status.success(), "{shell:?}");
    assert_eq!(
        String::from_utf8(shell.stdout).unwrap().trim(),
        home.path().join(".local/bin/dotfiles").to_str().unwrap()
    );
    output(&mut update(), true);
    assert_eq!(std::fs::read_to_string(profile).unwrap(), configured);
}

#[cfg(target_os = "linux")]
#[test]
fn extension_deferral_requires_a_configured_enabled_consumer_and_records_the_profile() {
    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "vscode-extensions.toml",
            "[base]\nextensions=['fixture.extension']\n",
        )
        .build();
    let home = tempfile::tempdir().unwrap();
    let marker = home
        .path()
        .join(".local/state/dotfiles/deferred-vscode-extensions");
    let unit = "dotfiles-first-login.service";
    let directory = home.path().join(".config/systemd/user");
    let run = |success| {
        output(
            common::cli_command(
                repo.root_path(),
                home.path(),
                None,
                "update",
                "vscode-extensions",
            )
            .env("DOTFILES_PROVISIONING", "arch-chroot"),
            success,
        )
    };
    assert!(run(false).contains("no configured dotfiles-first-login"));
    assert!(!marker.exists());

    let config = repo.root_path().join("conf");
    std::fs::write(
        config.join("systemd-units.toml"),
        format!("[base]\nunits=['{unit}']\n"),
    )
    .unwrap();
    let source = format!("config/systemd/user/{unit}");
    std::fs::write(
        config.join("symlinks.toml"),
        format!("[base]\nsymlinks=['{source}']\n"),
    )
    .unwrap();
    let source_path = repo.root_path().join("symlinks").join(source);
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
        &source_path,
        "[Install]\nWantedBy=graphical-session.target\n",
    )
    .unwrap();
    let wants = directory.join("graphical-session.target.wants");
    std::fs::create_dir_all(&wants).unwrap();
    let installed = directory.join(unit);
    std::os::unix::fs::symlink(source_path, &installed).unwrap();
    assert!(run(false).contains("enable the dotfiles-first-login"));
    assert!(!marker.exists());
    std::os::unix::fs::symlink(&installed, wants.join(unit)).unwrap();
    assert!(run(true).contains("scheduled for first graphical login"));
    assert!(
        std::fs::read_to_string(marker)
            .unwrap()
            .contains("DOTFILES_PROFILE=\"base\"\n")
    );
}

#[cfg(target_os = "linux")]
#[test]
fn first_login_replays_the_overlay_and_extensions_recorded_in_its_marker() {
    use std::os::unix::fs::PermissionsExt as _;

    let unit = "dotfiles-first-login.service";
    let source = format!("config/systemd/user/{unit}");
    let repo = common::TestContextBuilder::new()
        .with_config_file("systemd-units.toml", &format!("[base]\nunits=['{unit}']\n"))
        .with_config_file("symlinks.toml", &format!("[base]\nsymlinks=['{source}']\n"))
        .with_symlink_source_content(&source, "[Install]\nWantedBy=graphical-session.target\n")
        .build();
    let home = tempfile::tempdir().unwrap();
    let overlay = tempfile::tempdir().unwrap();
    let overlay_config = overlay.path().join("conf/vscode-extensions.toml");
    std::fs::create_dir_all(overlay_config.parent().unwrap()).unwrap();
    std::fs::write(&overlay_config, "[base]\nextensions=['fixture.private']\n").unwrap();
    let unit_dir = home.path().join(".config/systemd/user");
    let wants = unit_dir.join("graphical-session.target.wants");
    std::fs::create_dir_all(&wants).unwrap();
    std::os::unix::fs::symlink(
        repo.root_path().join("symlinks").join(source),
        unit_dir.join(unit),
    )
    .unwrap();
    std::os::unix::fs::symlink(unit_dir.join(unit), wants.join(unit)).unwrap();

    let update = || {
        common::cli_command(
            repo.root_path(),
            home.path(),
            None,
            "update",
            "vscode-extensions",
        )
    };
    output(
        update()
            .env("DOTFILES_PROVISIONING", "arch-chroot")
            .env("DOTFILES_OVERLAY", overlay.path()),
        true,
    );
    let marker = home
        .path()
        .join(".local/state/dotfiles/deferred-vscode-extensions");
    let content = std::fs::read_to_string(&marker).unwrap();
    let variables: std::collections::BTreeMap<_, _> = content
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key, serde_json::from_str::<String>(value).unwrap()))
        .collect();
    assert_eq!(
        variables.get("DOTFILES_OVERLAY").map(String::as_str),
        overlay.path().to_str()
    );
    let installed = home.path().join("bin/code");
    std::fs::create_dir_all(installed.parent().unwrap()).unwrap();
    std::fs::write(
        &installed,
        "#!/bin/sh\ntest \"$1\" = --list-extensions || exit 9\nprintf 'fixture.private\\n'\n",
    )
    .unwrap();
    std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::remove_file(overlay_config).unwrap();
    let observed = output(
        update()
            .envs(&variables)
            .env("DOTFILES_RESUME_VSCODE_EXTENSIONS", "1")
            .env("PATH", installed.parent().unwrap())
            .args(["--fail-on-skip", "--verbose"]),
        true,
    );
    assert!(observed.contains("ok fixture.private"), "{observed}");
    let missing = output(
        update()
            .env("DOTFILES_RESUME_VSCODE_EXTENSIONS", "1")
            .arg("--fail-on-skip"),
        false,
    );
    assert!(missing.contains("list is empty"), "{missing}");
    assert_eq!(std::fs::read_to_string(marker).unwrap(), content);
}

#[cfg(target_os = "linux")]
#[test]
fn rewriting_global_git_config_preserves_linux_access_metadata() {
    use rustix::fs::{XattrFlags, getxattr, setxattr};
    use std::os::unix::fs::MetadataExt as _;

    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "git-config.toml",
            "[base]\nsettings = [{key='core.editor',value='vim'}]\n",
        )
        .build();
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".gitconfig");
    std::fs::write(&config, "[core]\n editor = nano\n").unwrap();
    let mut acl = 2_u32.to_le_bytes().to_vec();
    let uid = config.metadata().unwrap().uid();
    for (tag, permissions, id) in [
        (1_u16, 6_u16, u32::MAX),
        (2, 4, uid),
        (4, 0, u32::MAX),
        (16, 4, u32::MAX),
        (32, 0, u32::MAX),
    ] {
        acl.extend(tag.to_le_bytes());
        acl.extend(permissions.to_le_bytes());
        acl.extend(id.to_le_bytes());
    }
    let attributes = [
        ("system.posix_acl_access", acl.as_slice()),
        ("user.dotfiles_fixture", b"keep annotation".as_slice()),
    ];
    for (name, value) in attributes {
        setxattr(&config, name, value, XattrFlags::empty()).unwrap();
    }
    output(
        &mut common::cli_command(repo.root_path(), home.path(), None, "update", "git"),
        true,
    );
    assert!(std::fs::read_to_string(&config).unwrap().contains("vim"));
    for (name, value) in attributes {
        let mut observed = vec![0; value.len()];
        assert_eq!(
            getxattr(&config, name, observed.as_mut_slice()).unwrap(),
            value.len(),
            "{name}"
        );
        assert_eq!(observed, value, "{name}");
    }
}

#[cfg(unix)]
#[test]
fn global_git_update_honors_the_selected_file_and_later_conditional_includes() {
    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "git-config.toml",
            "[base]\nsettings = [{key='core.editor',value='vim'}]\n",
        )
        .build();
    let home = tempfile::tempdir().unwrap();
    let default = home.path().join(".gitconfig");
    let selected = home.path().join("selected.gitconfig");
    let included = home.path().join("included.gitconfig");
    let untouched_default = "[core]\n editor = existing-default\n";
    let untouched_include = "[core]\n editor = existing-override\n";
    std::fs::write(&default, untouched_default).unwrap();
    std::fs::write(&included, untouched_include).unwrap();
    std::fs::write(
        &selected,
        format!(
            "[core]\n editor = vim\n[includeIf \"gitdir:{}/.git\"]\n path = included.gitconfig\n",
            repo.root_path().display()
        ),
    )
    .unwrap();
    let git_update = || {
        let mut command = common::cli_command(repo.root_path(), home.path(), None, "update", "git");
        command.env("GIT_CONFIG_GLOBAL", &selected).arg("--verbose");
        command
    };
    let mut command = git_update();
    let native_value = || {
        let result = std::process::Command::new("git")
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env("HOME", home.path())
            .env("GIT_CONFIG_GLOBAL", &selected)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .current_dir(repo.root_path())
            .args(["config", "--get", "core.editor"])
            .output()
            .unwrap();
        assert!(result.status.success(), "{result:?}");
        String::from_utf8(result.stdout).unwrap().trim().to_owned()
    };
    assert_eq!(native_value(), "existing-override");
    let before = std::fs::read(&selected).unwrap();
    let preview = output(command.arg("--dry-run"), true);
    assert!(
        preview.contains("configure  core.editor = vim"),
        "{preview}"
    );
    assert_eq!(std::fs::read(&selected).unwrap(), before);

    let mut apply = git_update();
    output(&mut apply, true);
    assert_eq!(native_value(), "vim");
    assert_eq!(
        std::fs::read_to_string(&default).unwrap(),
        untouched_default
    );
    assert_eq!(
        std::fs::read_to_string(&included).unwrap(),
        untouched_include
    );
    let after = std::fs::read(&selected).unwrap();
    let text = output(&mut apply, true);
    assert!(text.contains("ok core.editor = vim"), "{text}");
    assert_eq!(std::fs::read(&selected).unwrap(), after);
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
    assert!(text.contains("Tasks · 1 unchanged"), "{text}");
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
    assert!(text.contains("Tasks · 1 unchanged"), "{text}");
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

#[cfg(target_os = "linux")]
#[test]
fn default_linux_modes_preserve_local_zsh_files_and_restrict_ssh_config() {
    use std::os::unix::fs::PermissionsExt as _;

    let repo = common::TestContextBuilder::new()
        .with_config_file("chmod.toml", include_str!("../../conf/chmod.toml"))
        .build();
    let home = tempfile::tempdir().unwrap();
    let files = [
        (".config/zsh/private-plugin.zsh", 0o600, 0o600),
        (".config/zsh/local-helper", 0o700, 0o700),
        (".ssh/config", 0o644, 0o600),
    ];
    for (path, mode, _) in files {
        let target = home.path().join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "fixture").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode)).unwrap();
    }
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
    for (path, _, expected) in files {
        let mode = home
            .path()
            .join(path)
            .metadata()
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, expected, "{path}");
    }
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
    assert!(text.contains("Tasks · 1 unchanged"), "{text}");
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
        assert!(text.contains("Tasks · 2 unchanged"), "{text}");
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
