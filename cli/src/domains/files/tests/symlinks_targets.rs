use super::*;

#[test]
fn target_for_bashrc() {
    let home = PathBuf::from("/home/user");
    let target = compute_target(&home, "bashrc");
    assert_eq!(target, PathBuf::from("/home/user/.bashrc"));

    let root = PathBuf::from("/repo");
    for file in ["keybindings.json", "settings.json"] {
        let win_target = format!("AppData/Roaming/Code - Insiders/User/{file}");
        let entry = Symlink {
            source: format!("config/Code/User/{file}"),
            target: Some(win_target.clone()),
            allow_external_target: false,
            origin: Some(root.clone()),
        };
        assert_eq!(
            legacy_editor_source(&entry, &root),
            Some(win_target.as_str())
        );
    }
}

#[test]
fn target_for_config_subpath() {
    let home = PathBuf::from("/home/user");
    let target = compute_target(&home, "config/git/config");
    assert_eq!(target, PathBuf::from("/home/user/.config/git/config"));
}

#[test]
fn default_target_normalizes_current_directory_components() {
    let home = PathBuf::from("/home/user");
    for (source, expected) in [
        ("./bashrc", ".bashrc"),
        ("././bashrc", ".bashrc"),
        ("./config/./git//config", ".config/git/config"),
        ("config//git/config", ".config/git/config"),
        (".hidden", "..hidden"),
    ] {
        let target = compute_target(&home, source);
        assert_eq!(target, home.join(expected), "{source}");
        assert!(
            !target
                .components()
                .any(|component| component == std::path::Component::ParentDir),
            "{source}: {target:?}"
        );
    }
}

#[test]
fn default_target_uses_native_separator_semantics() {
    let home = PathBuf::from("/home/user");
    let expected = if cfg!(windows) {
        ".config/git/config"
    } else {
        r"..\config\.\git\config"
    };
    assert_eq!(
        compute_target(&home, r".\config\.\git\config"),
        home.join(expected)
    );
}

#[test]
fn resource_with_dot_prefixed_source_stays_inside_home() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("repo");
    let home = fixture.path().join("home");
    std::fs::create_dir_all(root.join("symlinks")).unwrap();
    std::fs::create_dir(&home).unwrap();
    std::fs::write(root.join("symlinks/bashrc"), "source").unwrap();
    let outside = fixture.path().join("bashrc");
    std::fs::write(&outside, "must survive").unwrap();
    let ctx = make_linux_context(root.clone(), None).with_home(home.clone());
    let resource = build_resource(
        &symlink("./bashrc", None),
        &root,
        &home,
        &ctx.executor_arc(),
        Some(&mut GitSymlinkPlaceholders::default()),
    );

    assert_eq!(resource.target, home.join(".bashrc"));
    assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    assert_eq!(
        resource.with_restore_missing(true).remove().unwrap(),
        ResourceChange::Applied
    );
    assert_eq!(
        std::fs::read_to_string(home.join(".bashrc")).unwrap(),
        "source"
    );
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "must survive");
}

#[test]
fn overlay_resource_description_uses_overlay_relative_source() {
    let root = PathBuf::from("/repo");
    let overlay = PathBuf::from("/private/overlay");
    let home = PathBuf::from("/home/user");
    let symlink = Symlink {
        source: "config/example".to_string(),
        target: None,
        allow_external_target: false,
        origin: Some(overlay),
    };
    let ctx = make_linux_context(root.clone(), None).with_home(home);
    let resource = build_resource(
        &symlink,
        &root,
        ctx.home(),
        &ctx.executor_arc(),
        Some(&mut GitSymlinkPlaceholders::default()),
    );

    assert_eq!(
        resource.description(),
        "~/.config/example \u{2192} symlinks/config/example"
    );
}

#[test]
fn git_symlink_placeholders_are_rejected_in_main_and_overlay() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let root = fixture.path().join("repo");
    let overlay = fixture.path().join("overlay");
    let home = fixture.path().join("home");
    std::fs::create_dir(&home).unwrap();
    for source_root in [&root, &overlay] {
        let repository = git2::Repository::init(source_root).unwrap();
        let sources = source_root.join("symlinks");
        std::fs::create_dir(&sources).unwrap();
        std::fs::write(sources.join("placeholder"), "real-config").unwrap();
        std::fs::write(sources.join("real-config"), "actual configuration").unwrap();
        let mut index = repository.index().unwrap();
        let path = Path::new("symlinks").join("placeholder");
        index.add_path(&path).unwrap();
        let mut entry = index.get_path(&path, 0).unwrap();
        entry.mode = 0o120_000;
        index.add(&entry).unwrap();
        index.write().unwrap();
    }
    let ctx = make_linux_context(root.clone(), None).with_home(home.clone());
    for origin in [None, Some(overlay)] {
        let symlink = Symlink {
            source: "placeholder".into(),
            target: Some(".configuration".into()),
            allow_external_target: false,
            origin,
        };
        let resource = build_resource(
            &symlink,
            &root,
            &home,
            &ctx.executor_arc(),
            Some(&mut GitSymlinkPlaceholders::default()),
        );
        assert!(
            matches!(
                resource.current_state().unwrap(),
                ResourceState::Invalid { reason } if reason.contains("Git records it as a symlink")
            ),
            "{symlink:?}"
        );
        assert!(!home.join(".configuration").exists());

        let ordinary = Symlink {
            source: "real-config".into(),
            ..symlink
        };
        assert_eq!(
            build_resource(
                &ordinary,
                &root,
                &home,
                &ctx.executor_arc(),
                Some(&mut GitSymlinkPlaceholders::default())
            )
            .current_state()
            .unwrap(),
            ResourceState::Missing,
            "ordinary source files must remain installable"
        );
    }
}

#[test]
fn placeholder_indexes_are_scoped_to_each_repository_and_build_pass() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("repo");
    let overlay = fixture.path().join("overlay");
    let initialize = |path: &Path, mode| {
        let repo = git2::Repository::init(path).unwrap();
        std::fs::create_dir(path.join("symlinks")).unwrap();
        std::fs::write(path.join("symlinks/source"), "target").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("symlinks/source")).unwrap();
        let mut entry = index.get_path(Path::new("symlinks/source"), 0).unwrap();
        entry.mode = mode;
        index.add(&entry).unwrap();
        index.write().unwrap();
    };
    initialize(&root, 0o120_000);
    initialize(&overlay, 0o100_644);
    let source = root.join("symlinks/source");
    let ctx = make_linux_context(root.clone(), None).with_home(fixture.path().join("home"));
    let entries = [symlink("source", None)];
    assert_eq!(
        build_resources(&ctx, &entries)
            .pop()
            .unwrap()
            .current_state()
            .unwrap(),
        ResourceState::Missing
    );
    let windows_ctx = crate::test_helpers::make_windows_context(root.clone(), None)
        .with_home(ctx.home().to_path_buf());
    assert!(
        matches!(build_resources(&windows_ctx, &entries).pop().unwrap().current_state().unwrap(), ResourceState::Invalid { reason } if reason.contains("Git records it as a symlink"))
    );
    let mut cache = GitSymlinkPlaceholders::default();
    assert!(
        cache
            .reason(&source, &root)
            .unwrap()
            .contains("Git records it as a symlink")
    );
    assert!(
        cache
            .reason(&overlay.join("symlinks/source"), &overlay)
            .is_none()
    );

    // A pass uses its original index, but a new pass must see index changes.
    let repo = git2::Repository::open(&root).unwrap();
    let mut index = repo.index().unwrap();
    let mut entry = index.get_path(Path::new("symlinks/source"), 0).unwrap();
    entry.mode = 0o100_644;
    index.add(&entry).unwrap();
    index.write().unwrap();
    assert!(cache.reason(&source, &root).is_some());
    assert!(
        GitSymlinkPlaceholders::default()
            .reason(&source, &root)
            .is_none()
    );

    std::fs::remove_file(&source).unwrap();
    std::fs::create_dir(&source).unwrap();
    assert!(
        cache.reason(&source, &root).is_none(),
        "a directory is not a placeholder file"
    );
    std::fs::remove_dir(&source).unwrap();
    assert!(
        cache.reason(&source, &root).is_none(),
        "an absent source is not a placeholder file"
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("missing", &source).unwrap();
        assert!(
            cache.reason(&source, &root).is_none(),
            "a real symlink is not a placeholder file"
        );
    }
}

#[test]
fn unavailable_placeholder_index_is_best_effort_and_cached_for_the_pass() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    let source = root.join("source");
    std::fs::write(&source, "target").unwrap();
    let mut cache = GitSymlinkPlaceholders::default();
    assert!(cache.reason(&source, root).is_none());
    let repo = git2::Repository::init(root).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("source")).unwrap();
    let mut entry = index.get_path(Path::new("source"), 0).unwrap();
    entry.mode = 0o120_000;
    index.add(&entry).unwrap();
    index.write().unwrap();
    assert!(cache.reason(&source, root).is_none());
    assert!(
        GitSymlinkPlaceholders::default()
            .reason(&source, root)
            .is_some()
    );
}

#[test]
fn target_for_ssh() {
    let home = PathBuf::from("/home/user");
    let target = compute_target(&home, "ssh/config");
    assert_eq!(target, PathBuf::from("/home/user/.ssh/config"));
}
