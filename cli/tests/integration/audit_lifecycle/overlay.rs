use super::*;

#[cfg(unix)]
#[test]
fn broken_hook_directory_is_not_an_empty_or_retired_source() {
    let repo = common::TestContextBuilder::new()
        .with_hook_source("pre-commit", "first version\n")
        .build();
    let cli = LifecycleCli::new(repo.root_path());
    let hooks = repo.root_path().join("hooks");
    let saved = repo.root_path().join("saved-hooks");
    let installed = repo.root_path().join(".git/hooks/pre-commit");
    let run = |verb, dry_run, success| {
        let mut command = cli.command(None, verb, "git-hooks");
        if dry_run {
            command.arg("--dry-run");
        }
        output(&mut command, success)
    };
    run("update", false, true);
    let owner = installed.with_extension("dotfiles-owned");
    let record = std::fs::read(&owner).unwrap();
    std::fs::rename(&hooks, &saved).unwrap();
    for destination in ["missing", "hooks"] {
        std::os::unix::fs::symlink(destination, &hooks).unwrap();
        for verb in ["update", "remove"] {
            for preview in [true, false] {
                let message = run(verb, preview, false);
                assert!(message.contains("reading hooks directory"), "{message}");
                assert_eq!(std::fs::read(&installed).unwrap(), b"first version\n");
                assert_eq!(std::fs::read(&owner).unwrap(), record);
            }
        }
        std::fs::remove_file(&hooks).unwrap();
    }
    std::fs::rename(saved, &hooks).unwrap();
    std::fs::write(hooks.join("pre-commit"), "updated version\n").unwrap();
    run("update", false, true);
    assert_eq!(std::fs::read(&installed).unwrap(), b"updated version\n");
    run("remove", false, true);
    assert!(!installed.exists() && !owner.exists());
}

#[test]
fn missing_overlay_cannot_replace_the_saved_selection_or_apply_base_settings() {
    let repo = common::TestContextBuilder::new()
        .with_config_file("git-config.toml", "[base]\ncore.editor = 'vim'\n")
        .build();
    let cli = LifecycleCli::new(repo.root_path());
    let overlay = tempfile::tempdir().unwrap();
    std::fs::create_dir(overlay.path().join("conf")).unwrap();
    std::fs::write(
        overlay.path().join("conf/git-config.toml"),
        "[base]\ncore.editor = 'nano'\n",
    )
    .unwrap();
    let run = |selected_overlay, success| cli.run(selected_overlay, "update", "git", success);
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
    let global = git2::Config::open(&cli.home.path().join(".gitconfig")).unwrap();
    assert_eq!(global.get_string("core.editor").unwrap(), "nano");
}

#[cfg(unix)]
#[test]
fn dangling_optional_overlay_config_cannot_revert_an_installed_preference() {
    let repo = common::TestContextBuilder::new()
        .with_config_file("agent-settings.toml", "[base.copilot]\nmodel = 'base'\n")
        .build();
    let cli = LifecycleCli::new(repo.root_path());
    let overlay = tempfile::tempdir().unwrap();
    let conf = overlay.path().join("conf");
    std::fs::create_dir(&conf).unwrap();
    let active = overlay.path().join("active.toml");
    std::fs::write(&active, "[base.copilot]\nmodel = 'overlay'\n").unwrap();
    std::os::unix::fs::symlink("../active.toml", conf.join("agent-settings.toml")).unwrap();
    let update = |selection| cli.command(selection, "update", "agent-settings");
    output(&mut update(Some(overlay.path())), true);
    let document = cli.home.path().join(".copilot/settings.json");
    let installed = std::fs::read(&document).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&installed)
            .unwrap()
            .get("model")
            .and_then(serde_json::Value::as_str),
        Some("overlay")
    );
    std::fs::remove_file(active).unwrap();
    for preview in [true, false] {
        let mut command = update(None);
        if preview {
            command.arg("--dry-run");
        }
        let message = output(&mut command, false);
        assert!(message.contains("conf/agent-settings.toml"), "{message}");
        assert_eq!(std::fs::read(&document).unwrap(), installed);
    }
}
