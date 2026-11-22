use super::*;

#[test]
fn deleted_source_files_are_pruned_but_unowned_files_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let (source, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
    std::fs::create_dir(source.join("scripts")).unwrap();
    std::fs::write(source.join("scripts/old.py"), "old managed script").unwrap();
    assert!(reconcile_cowork_skills(&ctx).unwrap());
    std::fs::write(target.join("scripts/personal.py"), "unmanaged").unwrap();
    std::fs::remove_file(source.join("scripts/old.py")).unwrap();
    assert!(reconcile_cowork_skills(&ctx).unwrap());
    assert!(!target.join("scripts/old.py").exists());
    assert_eq!(
        std::fs::read_to_string(target.join("scripts/personal.py")).unwrap(),
        "unmanaged"
    );
    assert!(!reconcile_cowork_skills(&ctx).unwrap());
}

#[test]
fn differing_unowned_and_modified_active_files_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let (source, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
    let skill = target.join("SKILL.md");
    for owned in [false, true] {
        if owned {
            std::fs::remove_file(&skill).unwrap();
            assert!(reconcile_cowork_skills(&ctx).unwrap());
            std::fs::write(source.join("SKILL.md"), "new upstream content").unwrap();
        }
        std::fs::write(&skill, "local content").unwrap();
        let error = reconcile_cowork_skills(&ctx).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("preserving unowned or modified Cowork file")
        );
        assert_eq!(std::fs::read_to_string(&skill).unwrap(), "local content");
        assert_eq!(
            target.parent().unwrap().join(FILE_OWNERSHIP_FILE).exists(),
            owned
        );
    }
}

#[test]
fn modified_stale_files_are_reported_and_retained_until_resolved() {
    for whole_skill in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (source, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
        std::fs::write(source.join("old.txt"), "managed").unwrap();
        reconcile_cowork_skills(&ctx).unwrap();
        std::fs::write(target.join("old.txt"), "my changes").unwrap();
        if whole_skill {
            std::fs::write(dir.path().join(".apm/apm.lock.yaml"), "dependencies: []\n").unwrap();
        } else {
            std::fs::remove_file(source.join("old.txt")).unwrap();
        }
        let error = reconcile_cowork_skills(&ctx).unwrap_err();
        assert!(
            error.to_string().contains("modified stale Cowork file"),
            "{error:#}"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("old.txt")).unwrap(),
            "my changes"
        );
        std::fs::remove_file(target.join("old.txt")).unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        assert!(!reconcile_cowork_skills(&ctx).unwrap());
    }
}

#[test]
fn unsafe_file_ownership_is_rejected_before_deployment() {
    let dir = tempfile::tempdir().unwrap();
    let (_, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
    std::fs::write(target.join("SKILL.md"), "preserve").unwrap();
    std::fs::write(
        target.parent().unwrap().join(FILE_OWNERSHIP_FILE),
        r#"{"example":{"../outside":{"hash":"x","link":false}}}"#,
    )
    .unwrap();
    assert!(reconcile_cowork_skills(&ctx).is_err());
    assert_eq!(
        std::fs::read_to_string(target.join("SKILL.md")).unwrap(),
        "preserve"
    );
}

#[cfg(windows)]
#[test]
fn ownership_key_distinguishes_missing_paths_from_inspection_errors() {
    let fixture = tempfile::tempdir().unwrap();
    assert_eq!(ownership_key(fixture.path(), "missing").unwrap(), "missing");
    assert!(ownership_key(fixture.path(), "invalid\0name").is_err());
}

#[cfg(windows)]
#[test]
fn skill_spelling_and_content_change_migrates_ownership_through_deselection() {
    for (before, after) in [("example", "Example"), ("İExample", "İEXAMPLE")] {
        let dir = tempfile::tempdir_in(".").unwrap();
        let (source, target, ctx) = setup_skill(dir.path(), "[agent-skills, copilot-cowork]");
        if before != "example" {
            std::fs::rename(&source, source.with_file_name(before)).unwrap();
        }
        let target = target.with_file_name(before);
        let lock_path = dir.path().join(".apm").join("apm.lock.yaml");
        let lock = std::fs::read_to_string(&lock_path).unwrap();
        std::fs::write(
            &lock_path,
            lock.replace("skills/example/", &format!("skills/{before}/")),
        )
        .unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        std::fs::write(target.join("cowork-owned.txt"), "preserve").unwrap();
        std::fs::write(source.with_file_name(before).join("SKILL.md"), "updated").unwrap();
        std::fs::write(
            &lock_path,
            lock.replace("skills/example/", &format!("skills/{after}/")),
        )
        .unwrap();

        assert!(reconcile_cowork_skills(&ctx).unwrap());

        assert_eq!(
            std::fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "updated",
            "the previous casing aliases the desired skill, not an orphan"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("cowork-owned.txt")).unwrap(),
            "preserve"
        );
        let root = target.parent().unwrap();
        assert_eq!(
            read_file_inventory(root)
                .unwrap()
                .into_keys()
                .collect::<Vec<_>>(),
            [after]
        );
        assert_eq!(
            read_owned_skills(root).unwrap(),
            BTreeSet::from([after.to_owned()])
        );
        assert!(!reconcile_cowork_skills(&ctx).unwrap());
        std::fs::write(lock_path, "dependencies: []\n").unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        assert!(!target.join("SKILL.md").exists());
        assert!(read_file_inventory(root).unwrap().is_empty());
        assert_eq!(
            std::fs::read(target.join("cowork-owned.txt")).unwrap(),
            b"preserve"
        );
        assert!(!reconcile_cowork_skills(&ctx).unwrap());
    }
}

#[cfg(windows)]
#[test]
fn asset_spelling_and_content_change_preserves_local_edits_until_retry() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let (source, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
    std::fs::create_dir(source.join("Docs")).unwrap();
    std::fs::write(source.join("Docs/Guide"), "prior").unwrap();
    assert!(reconcile_cowork_skills(&ctx).unwrap());
    let inventory = target.parent().unwrap().join(FILE_OWNERSHIP_FILE);
    let before = std::fs::read(&inventory).unwrap();
    // Recreate rather than rely on a case-only rename changing the stored name.
    std::fs::remove_dir_all(source.join("Docs")).unwrap();
    std::fs::create_dir(source.join("DOCS")).unwrap();
    std::fs::write(source.join("DOCS/GUIDE"), "updated").unwrap();
    let guide = target.join("Docs/Guide");
    std::fs::write(&guide, "edited").unwrap();
    let error = reconcile_cowork_skills(&ctx).unwrap_err();
    assert!(
        error.to_string().contains("modified Cowork file"),
        "{error:#}"
    );
    assert_eq!(std::fs::read(&guide).unwrap(), b"edited");
    assert_eq!(std::fs::read(&inventory).unwrap(), before);

    std::fs::write(&guide, "prior").unwrap();
    assert!(reconcile_cowork_skills(&ctx).unwrap());
    assert_eq!(std::fs::read(&guide).unwrap(), b"updated");
    assert!(!reconcile_cowork_skills(&ctx).unwrap());
    std::fs::write(dir.path().join(".apm/apm.lock.yaml"), "dependencies: []\n").unwrap();
    assert!(reconcile_cowork_skills(&ctx).unwrap());
    assert!(!guide.exists());
    assert!(!reconcile_cowork_skills(&ctx).unwrap());
}

#[cfg(windows)]
#[test]
fn competing_selected_spellings_do_not_claim_a_skill() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let (_, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
    std::fs::remove_dir(&target).unwrap();
    std::fs::write(dir.path().join(".apm/apm.lock.yaml"), "dependencies:\n- deployed_files: [.agents/skills/example/SKILL.md, .agents/skills/EXAMPLE/SKILL.md]\n").unwrap();
    let error = reconcile_cowork_skills(&ctx).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("ambiguous selected Cowork skill"),
        "{error:#}"
    );
    assert!(!target.exists());
    let root = target.parent().unwrap();
    assert!(!root.join(OWNERSHIP_FILE).exists());
    assert!(!root.join(FILE_OWNERSHIP_FILE).exists());
}

#[test]
fn conflicting_alias_fingerprints_block_updates_and_agreeing_aliases_retire() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let (source, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
    std::fs::create_dir(source.join("docs")).unwrap();
    std::fs::write(source.join("docs/guide"), "old").unwrap();
    assert!(reconcile_cowork_skills(&ctx).unwrap());
    let root = target.parent().unwrap();
    let mut inventory = read_file_inventory(root).unwrap();
    // Older runs could leave a second skill key only in the file ledger.
    #[cfg(windows)]
    let alias = "EXAMPLE";
    #[cfg(not(windows))]
    let alias = "example";
    let aliases = inventory.entry(alias.to_owned()).or_default();
    let guide = target.join("docs/guide");
    let old = file_fingerprint(&guide).unwrap().unwrap();
    aliases.insert(
        "docs/./guide".to_owned(),
        file_fingerprint(&target.join("SKILL.md")).unwrap().unwrap(),
    );
    std::fs::write(target.join("retired"), "old asset").unwrap();
    aliases.insert(
        "retired".to_owned(),
        file_fingerprint(&target.join("retired")).unwrap().unwrap(),
    );
    write_file_inventory(root, &inventory).unwrap();
    std::fs::write(source.join("docs/guide"), "new").unwrap();

    let error = reconcile_cowork_skills(&ctx).unwrap_err();
    assert!(error.to_string().contains("ambiguous Cowork"), "{error:#}");
    assert_eq!(std::fs::read(&guide).unwrap(), b"old");
    assert_eq!(read_file_inventory(root).unwrap(), inventory);
    assert!(target.join("retired").exists());

    inventory
        .get_mut(alias)
        .unwrap()
        .insert("docs/./guide".to_owned(), old);
    write_file_inventory(root, &inventory).unwrap();
    assert!(reconcile_cowork_skills(&ctx).unwrap());
    assert_eq!(std::fs::read(&guide).unwrap(), b"new");
    assert!(!target.join("retired").exists());
    assert_eq!(
        read_file_inventory(root).unwrap(),
        SkillInventory::from([(
            "example".to_owned(),
            selected_skill_inventory(&source).unwrap()
        )])
    );
    assert!(!reconcile_cowork_skills(&ctx).unwrap());
}

#[test]
fn reconcile_retains_partial_progress_and_stale_ownership_until_retry_converges() {
    let dir = tempfile::tempdir_in(".").expect("create fixture");
    let (source_skill, target_skill, ctx) =
        setup_skill(dir.path(), "[agent-skills, copilot-cowork]");
    let target = target_skill.parent().unwrap();
    let stale_skill = target.join("old");
    std::fs::create_dir_all(&stale_skill).unwrap();
    std::fs::write(stale_skill.join("SKILL.md"), "stale managed entry").unwrap();
    std::fs::write(stale_skill.join("notes.txt"), "Cowork-owned notes").unwrap();
    write_owned_skills(target, &BTreeSet::from(["old".to_string()])).unwrap();
    let old_files = FileInventory::from([(
        "SKILL.md".to_owned(),
        file_fingerprint(&stale_skill.join("SKILL.md"))
            .unwrap()
            .unwrap(),
    )]);
    write_atomic(
        &target.join(FILE_OWNERSHIP_FILE),
        serde_json::to_vec(&SkillInventory::from([("old".to_owned(), old_files)])).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".apm").join("apm.lock.yaml"),
        "dependencies:\n  - deployed_files:\n      - .agents/skills/example/SKILL.md\n\
             \x20     - .agents/skills/z-missing/SKILL.md\n",
    )
    .expect("write missing source dependency");

    let error = reconcile_cowork_skills(&ctx).expect_err("missing source must fail");
    assert!(error.to_string().contains("z-missing"));
    assert_eq!(
        read_owned_skills(target).unwrap(),
        BTreeSet::from(["example".to_string(), "old".to_string()])
    );
    let completed = std::fs::read(target_skill.join("SKILL.md")).unwrap();
    assert_eq!(
        completed,
        std::fs::read(source_skill.join("SKILL.md")).unwrap()
    );
    assert!(stale_skill.join("SKILL.md").is_file());
    assert!(!target.join("z-missing").exists());
    let ownership = std::fs::read(target.join(OWNERSHIP_FILE)).unwrap();

    assert!(
        reconcile_cowork_skills(&ctx)
            .unwrap_err()
            .to_string()
            .contains("z-missing")
    );
    assert_eq!(
        std::fs::read(target.join(OWNERSHIP_FILE)).unwrap(),
        ownership
    );
    assert_eq!(
        std::fs::read(target_skill.join("SKILL.md")).unwrap(),
        completed
    );

    let recovered_source = source_skill.parent().unwrap().join("z-missing");
    std::fs::create_dir_all(&recovered_source).unwrap();
    std::fs::write(recovered_source.join("SKILL.md"), "recovered source").unwrap();
    assert!(reconcile_cowork_skills(&ctx).expect("retry after repairing source"));
    assert_eq!(
        std::fs::read_to_string(target.join("z-missing/SKILL.md")).unwrap(),
        "recovered source"
    );
    assert_eq!(
        read_owned_skills(target).unwrap(),
        BTreeSet::from(["example".to_string(), "z-missing".to_string()])
    );
    assert!(!stale_skill.join("SKILL.md").exists());
    assert_eq!(
        std::fs::read_to_string(stale_skill.join("notes.txt")).unwrap(),
        "Cowork-owned notes"
    );
    assert!(!reconcile_cowork_skills(&ctx).expect("repeat converged reconciliation"));
}

#[test]
fn reconcile_rejects_corrupt_or_unsafe_ownership_without_mutating_skills() {
    for inventory in ["not json", "[\"..\"]", "[\"../personal\"]"] {
        let dir = tempfile::tempdir().expect("create temp dir");
        let (_, target_skill, ctx) = setup_skill(dir.path(), "[agent-skills]");
        std::fs::write(target_skill.join("SKILL.md"), "preserve").unwrap();
        std::fs::write(
            target_skill.parent().unwrap().join(OWNERSHIP_FILE),
            inventory,
        )
        .unwrap();

        assert!(reconcile_cowork_skills(&ctx).is_err());
        assert_eq!(
            std::fs::read_to_string(target_skill.join("SKILL.md")).unwrap(),
            "preserve"
        );
    }
}

#[test]
fn ownership_read_failures_are_distinct_from_absent_records() {
    use super::super::ownership::{read_file_inventory, read_owned_skills};

    let fixture = tempfile::tempdir().unwrap();
    assert!(read_file_inventory(fixture.path()).unwrap().is_empty());
    assert!(read_owned_skills(fixture.path()).unwrap().is_empty());
    for name in [FILE_OWNERSHIP_FILE, OWNERSHIP_FILE] {
        std::fs::create_dir(fixture.path().join(name)).unwrap();
    }
    let inventory_error = read_file_inventory(fixture.path()).unwrap_err();
    assert!(format!("{inventory_error:#}").contains("reading Cowork file ownership"));
    let owners_error = read_owned_skills(fixture.path()).unwrap_err();
    assert!(format!("{owners_error:#}").contains(OWNERSHIP_FILE));
}
