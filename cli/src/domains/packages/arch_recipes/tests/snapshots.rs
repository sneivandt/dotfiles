use super::*;

fn reset_to_head(repo: &git2::Repository) {
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    repo.reset(head.as_object(), git2::ResetType::Hard, None)
        .unwrap();
}

#[test]
fn snapshot_is_disposable_and_commits_inputs_for_git_cleanup() {
    let mut f = SnapshotFixture::new();
    let renamed = f.dir.path().join("packages/tool");
    std::fs::rename(&f.source, &renamed).unwrap();
    f.source = renamed;
    let ctx = &f.ctx;
    let cache = &f.cache;
    let preview_log = Arc::new(crate::test_helpers::CapturingOutput::default());
    let preview_ctx =
        ctx.with_dry_run(true)
            .with_log(Arc::<crate::test_helpers::CapturingOutput>::clone(
                &preview_log,
            ));
    let preview = configure_paru(&preview_ctx).unwrap();
    assert_eq!(task_batch(&preview).changed_count(), 2);
    assert_eq!(
        preview_log.messages_of(crate::infra::logging::MsgKind::DryRun),
        ["refresh tool", "update paru configuration"],
        "preview identifies the recipe and configuration without full paths"
    );
    assert_eq!(
        preview_log.messages_of(crate::infra::logging::MsgKind::Debug),
        [
            format!("snapshot {}", cache.display()),
            format!("write {}", paru_config_path(ctx).unwrap().display()),
        ]
    );
    assert!(!cache.exists());
    let applied_log = Arc::new(crate::test_helpers::CapturingOutput::default());
    let applied = configure_paru(&ctx.with_log(
        Arc::<crate::test_helpers::CapturingOutput>::clone(&applied_log),
    ))
    .unwrap();
    assert_eq!(task_batch(&applied).changed_count(), 2);
    assert_eq!(
        applied_log.messages_of(crate::infra::logging::MsgKind::Info),
        ["refresh tool", "update paru configuration"]
    );
    assert_eq!(
        applied_log.messages_of(crate::infra::logging::MsgKind::Debug),
        preview_log.messages_of(crate::infra::logging::MsgKind::Debug)
    );
    let repo = f.repo();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    let tree = head.tree().unwrap();
    let blob = repo
        .find_blob(tree.get_name("version.sh").unwrap().id())
        .unwrap();
    assert_eq!(blob.content(), b"original pins\n");
    let config = std::fs::read_to_string(ctx.home().join(".config/paru/paru.conf")).unwrap();
    assert!(config.contains("\nCleanAfter\n"));
    assert!(config.contains(&format!(
        "[dotfiles-tool-bin]\nPath = {}\nGenerateSrcinfo",
        cache.display()
    )));
    assert!(matches!(f.configure(), TaskResult::Ok));

    let unchanged_log = Arc::new(crate::test_helpers::CapturingOutput::default());
    let unchanged_ctx =
        ctx.with_dry_run(true)
            .with_log(Arc::<crate::test_helpers::CapturingOutput>::clone(
                &unchanged_log,
            ));
    let unchanged = configure_paru(&unchanged_ctx).unwrap();
    assert!(matches!(unchanged, TaskResult::Ok));
    assert!(
        unchanged_log
            .messages_of(crate::infra::logging::MsgKind::DryRun)
            .is_empty(),
        "preview must not announce work for a prepared snapshot"
    );

    let source = f.source.join("PKGBUILD");
    std::fs::write(&source, "modified tracked source\n").unwrap();
    // Resetting the standalone cache cannot reset the source checkout.
    std::fs::write(cache.join("PKGBUILD"), "modified cache\n").unwrap();
    repo.reset(head.as_object(), git2::ResetType::Hard, None)
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&source).unwrap(),
        "modified tracked source\n"
    );
    std::fs::write(cache.join("PKGBUILD"), "modified tracked source\n").unwrap();
    let refreshed = f.configure();
    assert_eq!(task_batch(&refreshed).changed_count(), 1);
    assert_ne!(repo.head().unwrap().target().unwrap(), head.id());
    assert!(matches!(f.configure(), TaskResult::Ok));
}

#[test]
fn snapshot_publishes_current_metadata_and_repairs_legacy_or_damaged_srcinfo() {
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .withf(|spec| spec.program() == "makepkg" && spec.arguments() == ["--printsrcinfo"] && spec.is_checked())
        .returning(|spec| {
            let pin_contents = std::fs::read_to_string(spec.working_dir().unwrap().join("version.sh")).unwrap();
            let version = pin_contents.trim().strip_prefix("pkgver=").unwrap();
            Ok(ExecResult::success(format!(
                "pkgbase = tool-bin\n\tpkgver = {version}\n\tpkgrel = 1\n\tarch = x86_64\npkgname = tool-bin\n"
            )))
        });
    let f = SnapshotFixture::with_executor(mock);
    let pins = f.source.join("version.sh");
    std::fs::write(&pins, "pkgver=1.0.103\n").unwrap();
    let _first = f.configure();
    let metadata = f.cache.join(".SRCINFO");
    let old_metadata = std::fs::read_to_string(&metadata).unwrap();
    assert!(old_metadata.contains("pkgver = 1.0.103\n"));

    // Reproduce the pre-fix snapshot: metadata exists but is not committed,
    // and a successful validation stamp exists for the recipe inputs.
    f.commit_index("legacy snapshot", |index| {
        index.remove_path(Path::new(".SRCINFO")).unwrap();
    });
    assert_eq!(task_batch(&f.configure()).changed_count(), 1);
    assert!(matches!(f.configure(), TaskResult::Ok));

    std::fs::write(&pins, "pkgver=1.0.104\n").unwrap();
    let repo = f.repo();
    let previous_head = repo.head().unwrap().target();
    let preview = f.preview();
    assert_eq!(task_batch(&preview).changed_count(), 1);
    assert_eq!(std::fs::read_to_string(&metadata).unwrap(), old_metadata);
    assert_eq!(repo.head().unwrap().target(), previous_head);
    assert_eq!(task_batch(&f.configure()).changed_count(), 1);
    let current = std::fs::read_to_string(&metadata).unwrap();
    assert!(current.contains("pkgver = 1.0.104\n"));

    for damaged in [Some(old_metadata.as_str()), None] {
        if let Some(contents) = damaged {
            std::fs::write(&metadata, contents).unwrap();
        } else {
            std::fs::remove_file(&metadata).unwrap();
        }
        assert_eq!(task_batch(&f.configure()).changed_count(), 1);
        assert_eq!(std::fs::read_to_string(&metadata).unwrap(), current);
        assert!(matches!(f.configure(), TaskResult::Ok));
    }
    // Paru's reset/clean must preserve the current package metadata too.
    std::fs::write(&metadata, old_metadata).unwrap();
    reset_to_head(&repo);
    assert_eq!(std::fs::read_to_string(metadata).unwrap(), current);
    assert!(unavailable_packages(&f.ctx, |_| true).unwrap().is_empty());
}

#[test]
fn changed_committed_metadata_invalidates_validation_cache() {
    let f = SnapshotFixture::new();
    let _prepared = f.configure();
    let metadata = f.cache.join(".SRCINFO");
    let current = std::fs::read_to_string(&metadata).unwrap();
    std::fs::write(&metadata, "stale committed metadata\n").unwrap();
    f.commit_index("stale committed metadata", |index| {
        index.add_path(Path::new(".SRCINFO")).unwrap();
    });
    assert_eq!(task_batch(&f.configure()).changed_count(), 1);
    assert_eq!(std::fs::read_to_string(&metadata).unwrap(), current);
    assert!(matches!(f.configure(), TaskResult::Ok));
}

#[test]
fn snapshot_commits_install_script_and_refreshes_changes_for_cleanup() {
    let f = SnapshotFixture::new();
    let script = f.source.join("package.install");
    std::fs::write(&script, "post_install() { :; }\n").unwrap();
    for name in ["LICENSE", "NOTICE"] {
        std::fs::write(f.source.join(name), format!("upstream {name}\n")).unwrap();
    }
    let snapshot = f.plan(true);
    assert!(snapshot.needs_write().unwrap());
    assert!(!snapshot.path.exists(), "planning must not publish inputs");
    snapshot.apply("fixture metadata\n").unwrap();
    assert_eq!(
        std::fs::read_to_string(snapshot.path.join(".git/dotfiles-local-build")).unwrap(),
        "dotfiles local PKGBUILD snapshot v1\n",
        "terminology changes must preserve the existing snapshot ownership protocol"
    );
    assert!(!f.plan(false).needs_write().unwrap());
    let repo = f.repo();
    std::fs::write(snapshot.path.join("package.install"), "cache edit\n").unwrap();
    reset_to_head(&repo);
    assert_eq!(
        std::fs::read_to_string(snapshot.path.join("package.install")).unwrap(),
        "post_install() { :; }\n"
    );
    std::fs::write(&script, "post_install() { echo updated; }\n").unwrap();
    let changed = f.plan(false);
    assert!(changed.needs_write().unwrap());
    changed.apply("fixture metadata\n").unwrap();
    assert!(!f.plan(false).needs_write().unwrap());
    let tree = repo.head().unwrap().peel_to_tree().unwrap();
    for name in ["LICENSE", "NOTICE"] {
        let blob = repo.find_blob(tree.get_name(name).unwrap().id()).unwrap();
        assert_eq!(blob.content(), format!("upstream {name}\n").as_bytes());
    }
    let blob = repo
        .find_blob(tree.get_name("package.install").unwrap().id())
        .unwrap();
    assert_eq!(blob.content(), b"post_install() { echo updated; }\n");
}

#[test]
fn retiring_optional_snapshot_inputs_cleans_only_the_managed_slots() {
    let f = SnapshotFixture::new();
    let cache = &f.cache;
    std::fs::write(
        f.source.join("package.install"),
        "obsolete lifecycle script\n",
    )
    .unwrap();
    assert_eq!(task_batch(&f.configure()).changed_count(), 2);
    std::fs::remove_file(f.source.join("package.install")).unwrap();
    std::fs::write(cache.join("artifact"), "keep\n").unwrap();
    let preview = f.preview();
    assert_eq!(task_batch(&preview).changed_count(), 1);
    assert!(cache.join("package.install").exists());
    assert!(
        unavailable_packages(&f.ctx, |_| true)
            .unwrap()
            .contains_key("tool-bin")
    );
    assert_eq!(task_batch(&f.configure()).changed_count(), 1);
    let repo = f.repo();
    let tree = repo.head().unwrap().peel_to_tree().unwrap();
    assert!(tree.get_name("package.install").is_none());
    assert!(!cache.join("package.install").exists());

    // Older versions could already have dropped the file from HEAD and the
    // index without unlinking it. A directory is not safe to delete recursively.
    std::fs::create_dir(cache.join("NOTICE")).unwrap();
    std::fs::write(cache.join("NOTICE/keep"), "user data\n").unwrap();
    assert_eq!(task_batch(&f.configure()).failed_count(), 1);
    assert!(
        unavailable_packages(&f.ctx, |_| true)
            .unwrap()
            .contains_key("tool-bin")
    );
    std::fs::rename(cache.join("NOTICE"), cache.join("saved")).unwrap();
    std::fs::write(cache.join("NOTICE"), "retired notice\n").unwrap();
    assert_eq!(task_batch(&f.configure()).changed_count(), 2);
    assert!(!cache.join("NOTICE").exists());
    assert_eq!(
        std::fs::read(cache.join("saved/keep")).unwrap(),
        b"user data\n"
    );
    assert_eq!(std::fs::read(cache.join("artifact")).unwrap(), b"keep\n");
    assert!(matches!(f.configure(), TaskResult::Ok));
}

#[test]
fn snapshot_repairs_attributes_and_preserves_exact_bytes_through_git_reset() {
    let f = SnapshotFixture::new();
    let script = "post_install() { :; }\r\n";
    let metadata = "fixture metadata\n";
    std::fs::write(f.source.join("package.install"), script).unwrap();
    let snapshot = f.plan(false);
    snapshot.apply(metadata).unwrap();
    let repo = f.repo();
    let attributes = repo.path().join("info/attributes");
    std::fs::write(snapshot.path.join(".gitattributes"), "* text eol=crlf\n").unwrap();

    for contents in [None, Some("* text eol=crlf\n")] {
        if let Some(contents) = contents {
            std::fs::write(&attributes, contents).unwrap();
        } else {
            std::fs::remove_file(&attributes).unwrap();
        }
        let planned = f.plan(false);
        assert!(planned.needs_write().unwrap());
        assert_eq!(read_optional(&attributes).unwrap().as_deref(), contents);
        planned.apply(metadata).unwrap();

        for name in ["package.install", ".SRCINFO"] {
            std::fs::write(snapshot.path.join(name), "changed\n").unwrap();
        }
        reset_to_head(&repo);

        for (name, expected) in [("package.install", script), (".SRCINFO", metadata)] {
            assert_eq!(
                std::fs::read(snapshot.path.join(name)).unwrap(),
                expected.as_bytes(),
                "{name}: snapshot-local attributes must preserve both CRLF and LF inputs"
            );
        }
        assert!(!f.plan(false).needs_write().unwrap());
    }
}

#[test]
fn snapshot_repairs_committed_symlink_inputs_even_when_blob_contents_match() {
    for name in ["PKGBUILD", "version.sh", ".SRCINFO"] {
        let f = SnapshotFixture::new();
        f.plan(false).apply("fixture metadata\n").unwrap();
        let repo = f.repo();
        let original = repo.head().unwrap().peel_to_tree().unwrap();
        let mut builder = repo.treebuilder(Some(&original)).unwrap();
        builder
            .insert(name, original.get_name(name).unwrap().id(), 0o120_000)
            .unwrap();
        let tree = repo.find_tree(builder.write().unwrap()).unwrap();
        commit_tree(&repo, "replace input with committed symlink", &tree);

        let damaged = f.plan(false);
        assert!(
            damaged.needs_write().unwrap(),
            "{name}: Git cleanup would restore a symlink rather than a usable input"
        );
        damaged.apply("fixture metadata\n").unwrap();
        assert_eq!(
            repo.head()
                .unwrap()
                .peel_to_tree()
                .unwrap()
                .get_name(name)
                .unwrap()
                .filemode(),
            0o100_644,
            "{name}",
        );
        assert!(
            !f.plan(false).needs_write().unwrap(),
            "{name}: repaired snapshot must converge",
        );
    }
}

#[cfg(unix)]
#[test]
fn snapshot_repairs_working_symlinks_without_modifying_their_targets() {
    for name in ["PKGBUILD", "version.sh", ".SRCINFO"] {
        let f = SnapshotFixture::new();
        f.plan(false).apply("fixture metadata\n").unwrap();
        let input = f.cache.join(name);
        let original = std::fs::read_to_string(&input).unwrap();
        let target = f.dir.path().join("outside-snapshot");
        std::fs::write(&target, &original).unwrap();
        std::fs::remove_file(&input).unwrap();
        std::os::unix::fs::symlink(&target, &input).unwrap();

        let damaged = f.plan(false);
        assert!(
            damaged.needs_write().unwrap(),
            "{name}: matching symlink contents must not bypass snapshot repair"
        );
        damaged.apply("fixture metadata\n").unwrap();

        assert!(std::fs::symlink_metadata(&input).unwrap().is_file());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), original);
        assert_eq!(std::fs::read_to_string(&input).unwrap(), original);
        assert!(
            !f.plan(false).needs_write().unwrap(),
            "{name}: repaired snapshot must converge",
        );
    }
}

#[test]
fn snapshot_preview_handles_missing_pins_and_refuses_unmanaged_cache() {
    let f = SnapshotFixture::new();
    let pins = f.source.join("version.sh");
    std::fs::remove_file(&pins).unwrap();
    let preview = f.preview();
    assert_eq!(task_batch(&preview).changed_count(), 2);
    assert!(!build_root(&f.ctx).unwrap().exists());
    assert_eq!(task_batch(&f.configure()).failed_count(), 1);
    std::fs::write(pins, "pins\n").unwrap();
    let cache = &f.cache;
    std::fs::create_dir_all(cache).unwrap();
    std::fs::write(cache.join("sentinel"), "unmanaged\n").unwrap();
    assert_eq!(task_batch(&f.configure()).failed_count(), 1);
    assert_eq!(
        std::fs::read_to_string(cache.join("sentinel")).unwrap(),
        "unmanaged\n"
    );
    assert!(!cache.join(".git").exists());
}

#[test]
fn build_cache_respects_injected_xdg_and_aurdest() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = snapshot_context(dir.path(), metadata_executor());
    let cache = dir.path().join("custom-cache");
    let env = crate::infra::env::MapEnv::new().with("XDG_CACHE_HOME", &cache);
    assert_eq!(
        build_root(&ctx.with_env(env.clone().into_handle())).unwrap(),
        cache.join("paru/clone")
    );
    let dest = dir.path().join("aurdest");
    assert_eq!(
        build_root(&ctx.with_env(env.with("AURDEST", &dest).into_handle())).unwrap(),
        dest
    );
}

#[test]
fn interrupted_snapshot_initialization_recovers_an_unborn_head() {
    let f = SnapshotFixture::new();
    assert_eq!(task_batch(&f.configure()).changed_count(), 2);
    let repo = f.repo();
    let branch = repo.head().unwrap().name().unwrap().to_owned();
    std::fs::remove_file(repo.path().join(branch)).unwrap();
    assert!(repo.head().is_err());
    let recovered = f.configure();
    assert_eq!(task_batch(&recovered).changed_count(), 1);
    assert!(repo.head().unwrap().peel_to_commit().is_ok());
    assert!(matches!(f.configure(), TaskResult::Ok));
}

#[test]
fn corrupt_snapshot_head_fails_before_preview_or_cached_input_changes() {
    let f = SnapshotFixture::new();
    assert_eq!(task_batch(&f.configure()).changed_count(), 2);
    let cache = &f.cache;
    let original = std::fs::read_to_string(cache.join("PKGBUILD")).unwrap();
    std::fs::write(f.source.join("PKGBUILD"), "new recipe inputs\n").unwrap();
    let plan = f.plan(false);
    let repo = f.repo();
    let branch = repo.head().unwrap().name().unwrap().to_owned();
    std::fs::write(repo.path().join(branch), "corrupted reference\n").unwrap();
    assert!(repo.head().is_err());
    assert_eq!(task_batch(&f.preview()).failed_count(), 1);
    assert!(plan.apply("fixture metadata\n").is_err());
    assert_eq!(
        std::fs::read_to_string(cache.join("PKGBUILD")).unwrap(),
        original
    );
    assert_eq!(
        std::fs::read_to_string(cache.join("version.sh")).unwrap(),
        "original pins\n"
    );
}

#[test]
fn snapshot_conflict_removes_only_its_repository_and_preserves_other_recipes() {
    let f = SnapshotFixture::new();
    let broken = add_recipe(f.dir.path(), "broken-bin");
    std::fs::write(broken.join("version.sh"), "old pins\n").unwrap();
    assert_eq!(task_batch(&f.configure()).failed_count(), 0);
    let path = build_root(&f.ctx).unwrap().join("broken-bin");
    std::fs::remove_file(path.join(".git/dotfiles-local-build")).unwrap();
    let result = f.configure();
    assert_eq!(task_batch(&result).failed_count(), 1);
    let config = std::fs::read_to_string(f.ctx.home().join(".config/paru/paru.conf")).unwrap();
    assert!(config.contains("[dotfiles-tool-bin]"));
    assert!(!config.contains("[dotfiles-broken-bin]"));
    assert!(
        path.join("version.sh").exists(),
        "conflicting snapshot must be preserved"
    );
    assert_eq!(
        unavailable_packages(&f.ctx, |_| true)
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        [&"broken-bin".to_string()]
    );
}
