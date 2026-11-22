use super::*;

#[cfg(unix)]
#[test]
fn nonregular_log_entries_cannot_block_update() {
    let repo = common::IntegrationTestContext::new();
    let cli = LifecycleCli::new(repo.root_path());
    let logs = cli.home.path().join("logs");
    std::fs::create_dir(&logs).unwrap();
    let fifo = logs.join("20250101T010101Z-update-7.log");
    nix::unistd::mkfifo(&fifo, nix::sys::stat::Mode::S_IRUSR).unwrap();
    let mut update = cli
        .command(None, "update", "git")
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

#[test]
fn checks_reject_hard_linked_locks_without_overwriting_user_data() {
    let repo = common::IntegrationTestContext::new();
    let cli = LifecycleCli::new(repo.root_path());
    let unrelated = cli.home.path().join("unrelated");
    std::fs::write(&unrelated, "valuable user data").unwrap();
    // Initialize the fixture repository before installing the conflicting lock.
    let _command = cli.command(None, "check", "config-diagnostics");
    let lock = repo.root_path().join(".git/dotfiles-run.lock");
    std::fs::hard_link(&unrelated, &lock).unwrap();
    let text = cli.run(None, "check", "config-diagnostics", false);
    assert!(text.contains("multiply linked"), "{text}");
    assert_eq!(
        std::fs::read_to_string(&unrelated).unwrap(),
        "valuable user data"
    );
    std::fs::remove_file(lock).unwrap();
    cli.run(None, "check", "config-diagnostics", true);
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
    let cli = LifecycleCli::new(repo.root_path());
    let source = repo.root_path().join("system/pam.d/test");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(
        source,
        "auth optional pam_example.so first\n-auth required pam_example.so second\n",
    )
    .unwrap();
    let text = cli.run(None, "check", "config-diagnostics", false);
    assert!(text.contains("conflicting rules"), "{text}");
}

#[cfg(target_os = "linux")]
#[test]
fn default_linux_modes_preserve_local_zsh_files_and_restrict_ssh_config() {
    use std::os::unix::fs::PermissionsExt as _;

    let repo = common::TestContextBuilder::new()
        .with_config_file("chmod.toml", include_str!("../../../conf/chmod.toml"))
        .build();
    let cli = LifecycleCli::new(repo.root_path());
    let files = [
        (".config/zsh/private-plugin.zsh", 0o600, 0o600),
        (".config/zsh/local-helper", 0o700, 0o700),
        (".ssh/config", 0o644, 0o600),
    ];
    for (path, mode, _) in files {
        let target = cli.home.path().join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "fixture").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode)).unwrap();
    }
    cli.run(None, "update", "file-permissions", true);
    for (path, _, expected) in files {
        let mode = cli
            .home
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
    let cli = LifecycleCli::new(repo.root_path());
    let outside = tempfile::tempdir().unwrap();
    let private = outside.path().join("private");
    std::fs::write(&private, "private").unwrap();
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o600)).unwrap();
    symlink(outside.path(), cli.home.path().join(".redirect")).unwrap();
    let config = repo.root_path().join("conf/chmod.toml");
    std::fs::write(
        &config,
        "[base]\npermissions = [{ mode = '755', path = 'redirect' }]\n",
    )
    .unwrap();
    for preview in [true, false] {
        let mut command = cli.command(None, "update", "file-permissions");
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
        cli.command(None, "update", "file-permissions")
            .arg("--dry-run"),
        true,
    );
    assert_eq!(
        private.metadata().unwrap().permissions().mode() & 0o777,
        0o600
    );
    cli.run(None, "update", "file-permissions", true);
    assert_eq!(
        private.metadata().unwrap().permissions().mode() & 0o777,
        0o644
    );
    let text = cli.run(None, "update", "file-permissions", true);
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
        let cli = LifecycleCli::new(repo.root_path());
        let overlay_path = use_overlay.then_some(overlay.path());
        output(
            cli.command(overlay_path, "update", "symlinks,file-permissions")
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
            cli.command(overlay_path, "update", "symlinks,file-permissions")
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
        let cli = LifecycleCli::new(repo.root_path());
        cli.run(Some(overlay.path()), "check", "symlink-sources", true);
        std::fs::remove_file(&source).unwrap();
        let text = cli.run(Some(overlay.path()), "check", "symlink-sources", false);
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
        let cli = LifecycleCli::new(repo.root_path());
        let overlay = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(overlay.path().join("symlinks")).unwrap();
        let source = match location {
            "home" => cli.home.path().join(".custom"),
            "public" => repo.root_path().join("symlinks/custom"),
            _ => overlay.path().join("symlinks/custom"),
        };
        std::fs::write(&source, "fixture").unwrap();
        cli.run(Some(overlay.path()), "check", "symlink-sources", true);
        std::fs::remove_file(source).unwrap();
        cli.run(Some(overlay.path()), "check", "symlink-sources", false);
    }
}

#[cfg(unix)]
const HISTORICAL_EDITOR_LINKS: &[(&str, &str, &str, Option<&str>)] = &[
    ("vim", ".config/nvim", "config/nvim", Some("init.vim")),
    (
        "config/Code/User/settings.json",
        ".config/Code - Insiders/User/settings.json",
        "config/Code - Insiders/User/settings.json",
        None,
    ),
    (
        "config/Code/User/keybindings.json",
        ".config/Code - Insiders/User/keybindings.json",
        "config/Code - Insiders/User/keybindings.json",
        None,
    ),
];
#[cfg(windows)]
const HISTORICAL_EDITOR_LINKS: &[(&str, &str, &str, Option<&str>)] = &[
    (
        "config/Code/User/settings.json",
        "AppData/Roaming/Code - Insiders/User/settings.json",
        "AppData/Roaming/Code - Insiders/User/settings.json",
        None,
    ),
    (
        "config/Code/User/keybindings.json",
        "AppData/Roaming/Code - Insiders/User/keybindings.json",
        "AppData/Roaming/Code - Insiders/User/keybindings.json",
        None,
    ),
];

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "cover the complete lifecycle and ownership of historical links"
)]
fn historical_editor_links_migrate_or_materialize_without_claiming_unrelated_entries() {
    fn link(source: &Path, target: &Path, directory: bool) {
        #[cfg(unix)]
        {
            let _ = directory;
            std::os::unix::fs::symlink(source, target).unwrap();
        }
        #[cfg(windows)]
        if directory {
            std::os::windows::fs::symlink_dir(source, target).unwrap();
        } else {
            std::os::windows::fs::symlink_file(source, target).unwrap();
        }
    }

    for &(source, target, old, config_file) in HISTORICAL_EDITOR_LINKS {
        let content = "canonical configuration\n";
        let source_file =
            config_file.map_or_else(|| source.to_string(), |file| format!("{source}/{file}"));
        let repo = common::TestContextBuilder::new()
            .with_config_file(
                "symlinks.toml",
                &format!("[base]\nsymlinks=[{{ source='{source}', target='{target}' }}]\n"),
            )
            .with_symlink_source_content(&source_file, content)
            .build();
        let cli = LifecycleCli::new(repo.root_path());
        let target = cli.home.path().join(target);
        // The CLI loads the canonical checkout root. Keep the historical
        // referent in its original spelling so the test exercises both.
        let canonical_root = dunce::canonicalize(repo.root_path()).unwrap();
        let source = canonical_root.join("symlinks").join(source);
        #[cfg(windows)]
        let old = repo.root_path().join("symlinks").join(old);
        #[cfg(unix)]
        let old = {
            // Older direct invocations used DOTFILES_ROOT without normalizing it.
            let existing = repo.root_path().join("legacy-root-spelling");
            std::fs::create_dir(&existing).unwrap();
            existing.join("../symlinks").join(old)
        };
        let directory = config_file.is_some();
        let target_file = config_file.map_or_else(|| target.clone(), |file| target.join(file));
        let run = |verb| cli.command(None, verb, "symlinks");
        let assert_link = |referent: &Path| {
            let actual = std::fs::read_link(&target).unwrap();
            assert_eq!(dunce::simplified(&actual), dunce::simplified(referent));
        };
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();

        // A currently existing legacy path might contain user data.
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        if directory {
            std::fs::create_dir(&old).unwrap();
        } else {
            std::fs::write(&old, "user data").unwrap();
        }
        link(&old, &target, directory);
        output(&mut run("update"), false);
        output(&mut run("remove"), true);
        assert_link(&old);
        if directory {
            std::fs::remove_dir(&old).unwrap();
        } else {
            std::fs::remove_file(&old).unwrap();
        }
        std::fs::remove_dir(old.parent().unwrap()).unwrap();
        assert!(!target.exists());

        output(run("update").arg("--dry-run"), true);
        assert_link(&old);
        output(&mut run("update"), true);
        assert_link(&source);
        assert_eq!(std::fs::read_to_string(&target_file).unwrap(), content);
        output(&mut run("update"), true);
        assert_link(&source);
        output(&mut run("remove"), true);
        assert!(!target.is_symlink());
        assert_eq!(std::fs::read_to_string(&target_file).unwrap(), content);

        if directory {
            std::fs::remove_dir_all(&target).unwrap();
        } else {
            std::fs::remove_file(&target).unwrap();
        }
        link(&old, &target, directory);
        output(run("remove").arg("--dry-run"), true);
        assert_link(&old);
        output(&mut run("remove"), true);
        assert!(!target.is_symlink());
        assert_eq!(std::fs::read_to_string(&target_file).unwrap(), content);
        output(&mut run("remove"), true);
        assert_eq!(std::fs::read_to_string(&target_file).unwrap(), content);
        output(&mut run("update"), false);
        assert_eq!(std::fs::read_to_string(&target_file).unwrap(), content);

        if directory {
            std::fs::remove_dir_all(&target).unwrap();
        } else {
            std::fs::remove_file(&target).unwrap();
        }
        let unrelated = old.with_extension("user");
        link(&unrelated, &target, directory);
        output(&mut run("update"), false);
        output(&mut run("remove"), true);
        assert_link(&unrelated);

        #[cfg(unix)]
        {
            // A symlink invented outside the checkout does not establish ownership.
            let alias = cli.home.path().join("checkout-alias");
            std::os::unix::fs::symlink(repo.root_path(), &alias).unwrap();
            let aliased_referent = alias.join(old.strip_prefix(repo.root_path()).unwrap());
            std::fs::remove_file(&target).unwrap();
            link(&aliased_referent, &target, directory);
            output(&mut run("update"), false);
            output(&mut run("remove"), true);
            assert_link(&aliased_referent);
        }
    }
}
