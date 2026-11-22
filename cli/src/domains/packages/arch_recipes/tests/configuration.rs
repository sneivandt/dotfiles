use super::*;

#[test]
fn missing_selected_paru_base_is_an_explicit_error() {
    let fixture = tempfile::tempdir().unwrap();
    let ctx = ContextBuilder::new(fixture.path().into()).build();
    let task = ConfigureParu::new(crate::infra::ConfigHandle::new(None));
    assert!(
        task.run(&ctx)
            .unwrap_err()
            .to_string()
            .contains("no managed source selected")
    );
}

#[cfg(unix)]
#[test]
fn selected_paru_base_cannot_escape_its_source_tree() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("symlinks");
    let external = fixture.path().join("outside");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(&external, "must not be published").unwrap();
    let link = root.join("base");
    std::os::unix::fs::symlink(&external, &link).unwrap();
    let ctx = ContextBuilder::new(fixture.path().into())
        .build()
        .with_home(fixture.path().join("home"))
        .with_env(crate::infra::env::MapEnv::new().into_handle());
    let task = ConfigureParu::new(crate::infra::ConfigHandle::new(Some(ParuBase {
        path: link.clone(),
        root,
    })));
    for dry_run in [true, false] {
        assert!(
            task.run(&ctx.with_dry_run(dry_run))
                .unwrap_err()
                .to_string()
                .contains("outside its managed source tree")
        );
        assert!(!ctx.home().exists());
    }
    std::fs::remove_file(&link).unwrap();
    std::fs::write(&link, "[options]\nBottomUp\n").unwrap();
    assert_eq!(task_batch(&task.run(&ctx).unwrap()).changed_count(), 1);
    assert!(
        std::fs::read_to_string(ctx.home().join(".config/paru/paru.conf"))
            .unwrap()
            .contains("BottomUp")
    );
}

#[test]
fn config_preview_apply_and_repeat() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ContextBuilder::new(dir.path().join("repo with spaces"))
        .arch(true)
        .build()
        .with_home(dir.path().join("home"))
        .with_env(crate::infra::env::MapEnv::new().into_handle());
    let base = ctx.symlinks_dir().join("config/paru/paru.conf.base");
    std::fs::create_dir_all(base.parent().unwrap()).unwrap();
    std::fs::write(&base, "[options]\nDevel\n").unwrap();
    let target = ctx.home().join(".config/paru/paru.conf");
    let preview = configure_paru(&ctx.with_dry_run(true)).unwrap();
    assert_eq!(task_batch(&preview).changed_count(), 1);
    assert!(!target.exists());
    assert_eq!(
        task_batch(&configure_paru(&ctx).unwrap()).changed_count(),
        1
    );
    let config = std::fs::read_to_string(&target).unwrap();
    assert!(config.contains(&format!(
        "CloneDir = {}\n",
        build_root(&ctx).unwrap().display()
    )));
    assert!(config.contains("[options]\nDevel\n"));
    assert!(matches!(configure_paru(&ctx).unwrap(), TaskResult::Ok));
    std::fs::write(base, "[options]\nBottomUp\n").unwrap();
    assert_eq!(
        task_batch(&configure_paru(&ctx).unwrap()).changed_count(),
        1
    );
}

#[test]
fn config_paths_must_survive_paru_whitespace_and_comment_parsing() {
    for path in [
        "cache with spaces",
        "cache/internal space/clone",
        "cache\twith\ttabs",
    ] {
        assert_eq!(config_path(Path::new(path)).unwrap(), path);
    }
    for path in [
        " cache",
        "cache ",
        "\tcache",
        "cache\t",
        "cache\nother",
        "cache\rother",
        "cache#comment",
    ] {
        assert!(
            config_path(Path::new(path)).is_err(),
            "Paru would interpret {path:?} as a different path"
        );
    }
}

#[cfg(unix)]
#[test]
fn paru_preparation_preserves_config_changes_made_after_discovery() {
    for config_write_planned in [false, true] {
        let f = SnapshotFixture::new();
        let _installed = f.configure();
        let config = paru_config_path(&f.ctx).unwrap();
        let original = std::fs::read_to_string(&config).unwrap();
        let cache = f.cache.join("PKGBUILD");
        let before = std::fs::read(&cache).unwrap();
        let recipe = "pkgname=tool-bin\n# updated\n";
        std::fs::write(f.source.join("PKGBUILD"), recipe).unwrap();
        if config_write_planned {
            std::fs::write(&config, "[options]\nDevel\n").unwrap();
        }
        let base = f.ctx.symlinks_dir().join("config/paru/paru.conf.base");
        let op = ParuConfig { base: &base };
        let OperationState::NeedsRun { plan } = op.current_state(&f.ctx).unwrap() else {
            panic!("new recipe must require preparation")
        };
        let saved = f.dir.path().join("saved.conf");
        let preferences = "[options]\nBottomUp\n";
        if config_write_planned {
            // A user saves additional preferences after the old file was read.
            std::fs::write(&config, preferences).unwrap();
        } else {
            // A user restores a managed link to a byte-identical backup.
            std::fs::write(&saved, &original).unwrap();
            std::fs::remove_file(&config).unwrap();
            std::os::unix::fs::symlink(&saved, &config).unwrap();
        }
        let error = op.apply(&f.ctx, &plan).unwrap_err();
        assert!(
            error.to_string().contains("changed during preparation"),
            "{error:#}"
        );
        assert_eq!(std::fs::read(&cache).unwrap(), before);
        assert_eq!(config.is_symlink(), !config_write_planned);
        assert_eq!(
            std::fs::read_to_string(&config).unwrap(),
            if config_write_planned {
                preferences
            } else {
                &original
            }
        );

        std::fs::write(&base, preferences).unwrap();
        let retry = f.configure();
        assert_eq!(task_batch(&retry).changed_count(), 2);
        assert!(!config.is_symlink());
        assert!(
            std::fs::read_to_string(&config)
                .unwrap()
                .contains(preferences)
        );
        if !config_write_planned {
            assert_eq!(std::fs::read_to_string(saved).unwrap(), original);
        }
        assert_eq!(std::fs::read_to_string(cache).unwrap(), recipe);
        assert!(matches!(f.configure(), TaskResult::Ok));
    }
}

#[cfg(unix)]
#[test]
fn config_migrates_old_link_without_overwriting_tracked_source() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ContextBuilder::new(dir.path().into())
        .arch(true)
        .build()
        .with_home(dir.path().join("home"))
        .with_env(crate::infra::env::MapEnv::new().into_handle());
    let base = ctx.symlinks_dir().join("config/paru/paru.conf.base");
    std::fs::create_dir_all(base.parent().unwrap()).unwrap();
    std::fs::write(&base, "[options]\nDevel\n").unwrap();
    let target = ctx.home().join(".config/paru/paru.conf");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&base, &target).unwrap();
    assert_eq!(
        task_batch(&configure_paru(&ctx).unwrap()).changed_count(),
        1
    );
    assert!(!std::fs::symlink_metadata(target).unwrap().is_symlink());
    assert_eq!(std::fs::read_to_string(base).unwrap(), "[options]\nDevel\n");
}

#[cfg(target_os = "linux")]
#[test]
fn powershell_package_hooks_preserve_existing_login_shell_registrations() {
    let dir = tempfile::tempdir().unwrap();
    let etc = dir.path().join("etc");
    std::fs::create_dir(&etc).unwrap();
    let shells = etc.join("shells");
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../packages/powershell/package.install");
    let hook = |name: &str| {
        std::process::Command::new("/bin/sh")
            .args(["-c", ". \"$1\" && \"$2\"", "fixture"])
            .arg(&script)
            .arg(name)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .current_dir(dir.path())
            .output()
            .unwrap()
            .status
            .success()
    };
    assert!(
        !hook("post_install"),
        "an unreadable/missing registry is not an empty list"
    );
    assert!(!shells.exists());
    let existing = "/bin/sh\n/usr/bin/pwsh";
    std::fs::write(&shells, existing).unwrap();
    for action in ["post_install", "post_upgrade", "pre_remove"] {
        assert!(hook(action));
        assert_eq!(std::fs::read_to_string(&shells).unwrap(), existing);
    }

    let target = etc.join("login-shells");
    std::fs::rename(&shells, &target).unwrap();
    std::os::unix::fs::symlink("login-shells", &shells).unwrap();
    std::fs::write(&target, "/bin/sh").unwrap();
    assert!(hook("post_install"));
    let registered = std::fs::read_to_string(&target).unwrap();
    assert_eq!(
        registered
            .lines()
            .filter(|s| s.starts_with('/'))
            .collect::<Vec<_>>(),
        ["/bin/sh", "/usr/bin/pwsh"]
    );
    assert!(hook("post_upgrade"));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), registered);
    std::fs::write(&target, format!("{registered}# local\n/usr/bin/pwsh\n")).unwrap();
    assert!(hook("pre_remove"));
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "/bin/sh\n# local\n/usr/bin/pwsh\n"
    );
    assert!(shells.is_symlink());
    assert!(hook("pre_remove"));
}

#[test]
fn config_overrides_control_publication_and_readiness() {
    for explicit in [false, true] {
        let f = SnapshotFixture::new();
        let xdg = f.dir.path().join("xdg config");
        let explicit_path = f.dir.path().join("custom/paru.conf");
        let mut env = crate::infra::env::MapEnv::new().with("XDG_CONFIG_HOME", &xdg);
        let target = if explicit {
            env = env.with("PARU_CONF", &explicit_path);
            explicit_path
        } else {
            xdg.join("paru/paru.conf")
        };
        let ctx = f.ctx.with_env(env.into_handle());
        let default = ctx.home().join(".config/paru/paru.conf");
        std::fs::create_dir_all(default.parent().unwrap()).unwrap();
        std::fs::write(&default, "sentinel\n").unwrap();
        let _preview = configure_paru(&ctx.with_dry_run(true)).unwrap();
        assert!(!target.exists());
        assert!(!build_root(&ctx).unwrap().exists());
        assert!(
            unavailable_packages(&ctx, |_| true)
                .unwrap()
                .contains_key("tool-bin")
        );
        let _applied = configure_paru(&ctx).unwrap();
        assert!(target.is_file());
        assert!(unavailable_packages(&ctx, |_| true).unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(&default).unwrap(), "sentinel\n");
        // A valid default config must not mask an invalid active override.
        std::fs::copy(&target, &default).unwrap();
        std::fs::write(&target, "[options]\n").unwrap();
        assert!(
            unavailable_packages(&ctx, |_| true)
                .unwrap()
                .contains_key("tool-bin")
        );
        let _repaired = configure_paru(&ctx).unwrap();
        assert!(unavailable_packages(&ctx, |_| true).unwrap().is_empty());
        assert!(matches!(configure_paru(&ctx).unwrap(), TaskResult::Ok));
    }
    for key in ["PARU_CONF", "XDG_CONFIG_HOME"] {
        for value in ["", "relative/path"] {
            let f = SnapshotFixture::new();
            let ctx = f.ctx.with_env(
                crate::infra::env::MapEnv::new()
                    .with(key, value)
                    .into_handle(),
            );
            assert!(configure_paru(&ctx).is_err(), "{key}={value:?}");
            assert!(unavailable_packages(&ctx, |_| true).is_err());
            assert!(!build_root(&ctx).unwrap().exists());
        }
    }
}
