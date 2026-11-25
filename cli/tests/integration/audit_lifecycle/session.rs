use super::*;

#[cfg(unix)]
#[test]
fn shell_path_migrates_the_legacy_export_before_an_early_return() {
    let repo = common::IntegrationTestContext::new();
    let cli = LifecycleCli::new(repo.root_path());
    let profile = cli.home.path().join(".profile");
    let original = "#!/bin/sh\nexport FIXTURE_SETTING=kept\nreturn 0\n";
    let export = "export PATH=\"$HOME/.local/bin:$PATH\"";
    let legacy = format!("{original}\n# Added by dotfiles\n{export}\n");
    std::fs::write(&profile, &legacy).unwrap();
    let update = || cli.command(None, "update", "launcher,path");
    output(update().arg("--dry-run"), true);
    assert_eq!(std::fs::read_to_string(&profile).unwrap(), legacy);
    output(&mut update(), true);
    let configured = std::fs::read_to_string(&profile).unwrap();
    assert_eq!(configured.matches(export).count(), 2);
    assert!(configured.starts_with("#!/bin/sh\n"));
    assert!(configured.contains("export FIXTURE_SETTING=kept\nreturn 0\n"));
    let shell = Command::new("/bin/sh")
        .env_clear()
        .env("HOME", cli.home.path())
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
        cli.home
            .path()
            .join(".local/bin/dotfiles")
            .to_str()
            .unwrap()
    );
    output(&mut update(), true);
    assert_eq!(std::fs::read_to_string(profile).unwrap(), configured);
}

#[cfg(target_os = "linux")]
#[test]
fn shell_path_recovers_from_a_fresh_shell_without_doubling_the_guard_or_entry() {
    let repo = common::IntegrationTestContext::new();
    let cli = LifecycleCli::new(repo.root_path());
    let root = repo.root_path();
    let profile = cli.home.path().join(".profile");
    let update = |only| cli.command(None, "update", only);
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../dotfiles.sh"),
        root.join("dotfiles.sh"),
    )
    .unwrap();
    std::fs::create_dir(root.join("bin")).unwrap();
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_dotfiles"), root.join("bin/dotfiles")).unwrap();
    let fresh_shell = |script: &str| {
        let mut shell = Command::new("/bin/sh");
        shell
            .env_clear()
            .env("HOME", cli.home.path())
            .env("PATH", "/no-tools")
            .env("DOTFILES_PROFILE", "base")
            .env("DOTFILES_SKIP_SELF_UPDATE", "1")
            .env("DOTFILES_LOG_DIR", cli.home.path().join("logs"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", cli.home.path().join(".gitconfig"))
            .current_dir(cli.home.path())
            .args(["-ec", script]);
        output(&mut shell, true)
    };

    output(&mut update("launcher,path"), true);
    let guard = std::fs::read_to_string(&profile).unwrap();
    assert_eq!(guard.matches("# Added by dotfiles").count(), 1);
    let edited = format!("# personal comment\r\n{guard}export PATH=/no-tools\n");
    std::fs::write(&profile, &edited).unwrap();
    output(update("path").arg("--dry-run"), true);
    assert_eq!(std::fs::read_to_string(&profile).unwrap(), edited);
    output(&mut update("path"), true);
    let installed = std::fs::read_to_string(&profile).unwrap();
    assert!(installed.starts_with(&edited));
    assert_eq!(installed.matches(&guard).count(), 2);

    let conflict = format!("export FIRST=yes\n{installed}export PATH=/no-tools\n");
    std::fs::write(&profile, &conflict).unwrap();
    let error = output(&mut update("path"), false);
    assert_eq!(std::fs::read_to_string(&profile).unwrap(), conflict);
    let retry =
        r#"PATH=/usr/bin:/bin "$HOME/.local/bin/dotfiles" update --only path --no-repo-update"#;
    assert!(error.contains(retry), "{error}");
    // Both the inherited PATH and the freshly sourced profile hide dotfiles.
    // The conflict still produces the right diagnostic via the printed route.
    let still_blocked = fresh_shell(&format!(
        r#". "$HOME/.profile"; if command -v dotfiles || {retry}; then exit 1; fi"#
    ));
    assert!(
        still_blocked.contains("between the leading and trailing"),
        "{still_blocked}"
    );
    let repaired = installed.replacen(
        "export PATH=/no-tools",
        "export FIRST=yes\nexport PATH=/no-tools",
        1,
    );
    std::fs::write(&profile, &repaired).unwrap();
    let result = fresh_shell(&format!(
        r#"{retry}; . "$HOME/.profile"; . "$HOME/.profile"; test "$FIRST" = yes; test "$PATH" = "$HOME/.local/bin:/no-tools"; command -v dotfiles"#
    ));
    assert!(result.contains("unchanged"), "{result}");
    assert_eq!(std::fs::read_to_string(&profile).unwrap(), repaired);
    let run_id = error
        .split_once("dotfiles log --id ")
        .unwrap()
        .1
        .split_whitespace()
        .next()
        .unwrap();
    let log = fresh_shell(&format!(
        r#"PATH=/usr/bin:/bin "$HOME/.local/bin/dotfiles" log --id {run_id} -v"#
    ));
    assert!(log.contains("between the leading and trailing"), "{log}");
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
    let cli = LifecycleCli::new(repo.root_path());
    let marker = cli
        .home
        .path()
        .join(".local/state/dotfiles/deferred-vscode-extensions");
    let unit = "dotfiles-first-login.service";
    let directory = cli.home.path().join(".config/systemd/user");
    let run = |success| {
        output(
            cli.command(None, "update", "vscode-extensions")
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
    let cli = LifecycleCli::new(repo.root_path());
    let overlay = tempfile::tempdir().unwrap();
    let overlay_config = overlay.path().join("conf/vscode-extensions.toml");
    std::fs::create_dir_all(overlay_config.parent().unwrap()).unwrap();
    std::fs::write(&overlay_config, "[base]\nextensions=['fixture.private']\n").unwrap();
    let unit_dir = cli.home.path().join(".config/systemd/user");
    let wants = unit_dir.join("graphical-session.target.wants");
    std::fs::create_dir_all(&wants).unwrap();
    std::os::unix::fs::symlink(
        repo.root_path().join("symlinks").join(source),
        unit_dir.join(unit),
    )
    .unwrap();
    std::os::unix::fs::symlink(unit_dir.join(unit), wants.join(unit)).unwrap();

    let update = || cli.command(None, "update", "vscode-extensions");
    output(
        update()
            .env("DOTFILES_PROVISIONING", "arch-chroot")
            .env("DOTFILES_OVERLAY", overlay.path()),
        true,
    );
    let marker = cli
        .home
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
    let installed = cli.home.path().join("bin/code");
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

#[cfg(unix)]
#[test]
fn non_interactive_login_shell_never_calls_an_authentication_prompt() {
    use std::os::unix::fs::PermissionsExt as _;
    if nix::unistd::Uid::effective().is_root() {
        return;
    }
    let repo = common::IntegrationTestContext::new();
    let cli = LifecycleCli::new(repo.root_path());
    let bin = cli.home.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let prompt_marker = cli.home.path().join("unexpected-prompt");
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
    let mut command = cli.command(None, "update", "login-shell");
    command
        .env("PATH", &bin)
        .env("USER", "fixture")
        .arg("--fail-on-skip");
    let text = output(&mut command, false);
    // Runners without the system package must decline the PATH-only fixture.
    assert!(
        text.contains("authentication")
            || text.contains("system zsh not found at /usr/bin/zsh")
            || text.contains("is not registered in /etc/shells"),
        "{text}"
    );
    assert!(
        !prompt_marker.exists(),
        "non-interactive execution must not invoke chsh"
    );
}

#[test]
fn launcher_preserves_user_files_and_modified_installations() {
    let repo = common::IntegrationTestContext::new();
    let cli = LifecycleCli::new(repo.root_path());
    let name = if cfg!(windows) {
        "dotfiles.cmd"
    } else {
        "dotfiles"
    };
    let target = cli.home.path().join(".local/bin").join(name);
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "user launcher").unwrap();
    for preview in [true, false] {
        let mut command = cli.command(None, "update", "launcher");
        command.arg("--fail-on-skip");
        if preview {
            command.arg("--dry-run");
        }
        output(&mut command, false);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "user launcher");
    }
    std::fs::remove_file(&target).unwrap();
    output(
        cli.command(None, "update", "launcher").arg("--dry-run"),
        true,
    );
    assert!(!target.exists(), "preview must not install a launcher");
    output(
        cli.command(None, "update", "launcher").env(
            "DOTFILES_WRAPPER",
            if cfg!(windows) { "sh" } else { "pwsh" },
        ),
        true,
    );
    let installed = std::fs::read(&target).unwrap();
    let text = cli.run(None, "update", "launcher", true);
    assert!(text.contains("Tasks · 1 unchanged"), "{text}");
    assert_eq!(std::fs::read(&target).unwrap(), installed);
    std::fs::write(&target, "user edit").unwrap();
    cli.run(None, "remove", "launcher", true);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "user edit");
    std::fs::write(&target, &installed).unwrap();
    cli.run(None, "remove", "launcher", true);
    assert!(!target.exists());
}
