use super::*;

#[test]
fn readiness_requires_published_config_and_committed_current_inputs() {
    let f = SnapshotFixture::new();
    let _result = f.configure();
    assert!(unavailable_packages(&f.ctx).unwrap().is_empty());
    let config = f.ctx.home().join(".config/paru/paru.conf");
    let original = std::fs::read_to_string(&config).unwrap();
    std::fs::write(&config, "[options]\n").unwrap();
    assert!(
        unavailable_packages(&f.ctx)
            .unwrap()
            .contains_key("tool-bin")
    );
    std::fs::write(config, original).unwrap();
    std::fs::write(f.source.join("PKGBUILD"), "new inputs\n").unwrap();
    assert!(
        unavailable_packages(&f.ctx)
            .unwrap()
            .contains_key("tool-bin")
    );
    let _refreshed = f.configure();
    assert!(unavailable_packages(&f.ctx).unwrap().is_empty());
}

#[test]
fn split_package_ownership_is_explicit_and_ordinary_names_default_to_base() {
    assert_eq!(source().package_names(), ["tool-bin"]);
    let f = SnapshotFixture::new();
    let path = f.source.join("release.json");
    let mut descriptor: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    descriptor["packages"] = serde_json::json!(["tool-cli", "tool-gui"]);
    std::fs::write(&path, descriptor.to_string()).unwrap();
    let missing = unavailable_packages(&f.ctx).unwrap();
    assert!(missing.contains_key("tool-cli"));
    assert!(missing.contains_key("tool-gui"));
    assert!(!missing.contains_key("tool-bin"));
    for invalid in [
        serde_json::json!([]),
        serde_json::json!(["../unsafe"]),
        serde_json::json!(["same", "same"]),
    ] {
        descriptor["packages"] = invalid;
        std::fs::write(&path, descriptor.to_string()).unwrap();
        assert!(unavailable_packages(&f.ctx).is_err());
    }
    std::fs::remove_file(path).unwrap();
    assert!(
        unavailable_packages(&f.ctx).is_err(),
        "missing descriptor must not erase ownership and permit AUR fallback"
    );
}

#[test]
fn readiness_rejects_descriptor_changes_before_configuration_is_refreshed() {
    let f = SnapshotFixture::new();
    let _prepared = f.configure();
    assert!(unavailable_packages(&f.ctx).unwrap().is_empty());
    let path = f.source.join("release.json");
    let mut descriptor: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    descriptor["packages"] = serde_json::json!(["tool-cli", "tool-gui"]);
    std::fs::write(path, descriptor.to_string()).unwrap();

    let missing = unavailable_packages(&f.ctx).unwrap();

    assert_eq!(
        missing.keys().map(String::as_str).collect::<Vec<_>>(),
        ["tool-cli", "tool-gui"],
        "an old snapshot must not send newly declared custom packages to the AUR"
    );
}

#[test]
fn readiness_requires_successful_metadata_validation_for_current_inputs() {
    let f = SnapshotFixture::new();
    let _prepared = f.configure();
    let stamp = f.cache.join(".git/dotfiles-validated-metadata");
    for damaged in [None, Some("stale validation key")] {
        if let Some(value) = damaged {
            std::fs::write(&stamp, value).unwrap();
        } else {
            std::fs::remove_file(&stamp).unwrap();
        }

        assert!(
            unavailable_packages(&f.ctx)
                .unwrap()
                .contains_key("tool-bin"),
            "missing or stale validation is not proof of package readiness"
        );
        assert!(
            unavailable_packages(&f.ctx.with_dry_run(true))
                .unwrap()
                .is_empty(),
            "preview may plan prerequisite validation without executing recipes"
        );
        let _revalidated = f.configure();
        assert!(unavailable_packages(&f.ctx).unwrap().is_empty());
    }
}

#[test]
fn snapshot_apply_failure_does_not_publish_its_section_or_block_other_snapshots() {
    let f = SnapshotFixture::new();
    let broken = add_recipe(f.dir.path(), "broken-bin");
    std::fs::write(broken.join("version.sh"), "old pins\n").unwrap();
    let op = ParuConfig {
        base: &f.ctx.symlinks_dir().join("config/paru/paru.conf.base"),
    };
    let OperationState::NeedsRun { plan } = op.current_state(&f.ctx).unwrap() else {
        panic!("expected preparation plan")
    };
    let conflict = build_root(&f.ctx).unwrap().join("broken-bin");
    std::fs::create_dir_all(&conflict).unwrap();
    std::fs::write(conflict.join("sentinel"), "preserve").unwrap();
    let result = op.apply(&f.ctx, &plan).unwrap();
    assert_eq!(task_batch(&result).failed_count(), 1);
    assert_eq!(task_batch(&result).changed_count(), 2);
    assert_eq!(
        std::fs::read_to_string(conflict.join("sentinel")).unwrap(),
        "preserve"
    );
    let config = std::fs::read_to_string(f.ctx.home().join(".config/paru/paru.conf")).unwrap();
    assert!(!config.contains("[dotfiles-broken-bin]"));
    assert!(config.contains("[dotfiles-tool-bin]"));
}
