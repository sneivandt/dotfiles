use super::*;

#[test]
fn current_and_legacy_skill_ownership_preserve_configured_names() {
    let dir = tempfile::tempdir_in(".").unwrap();
    std::fs::write(
        dir.path().join(OWNERSHIP_FILE),
        "[\"Example\", \"example\"]",
    )
    .unwrap();
    let expected = BTreeSet::from(["Example".to_string(), "example".to_string()]);
    assert_eq!(read_owned_skills(dir.path()).unwrap(), expected);

    let content = "dependencies:\n- deployed_files: \
            [.agents/skills/Example/SKILL.md, .agents/skills/example/SKILL.md]\n";
    std::fs::create_dir(dir.path().join(".apm")).unwrap();
    std::fs::write(dir.path().join(".apm").join("apm.lock.yaml"), content).unwrap();
    assert_eq!(desired_cowork_skill_names(dir.path()).unwrap(), expected);

    let mut lock: Value =
        crate::infra::yaml::from_str(&content.replace(".agents/skills/", "cowork://skills/"))
            .unwrap();
    let mut legacy = BTreeSet::new();
    assert!(strip_legacy_cowork_deployments(&mut lock, &mut legacy));
    assert_eq!(legacy, expected);
}

#[test]
fn legacy_ownership_comes_only_from_explicit_cowork_deployment_records() {
    for record in [
        "dependencies:\n- deployed_files: [cowork://skills/example/SKILL.md]\n",
        "dependencies:\n- deployed_file_hashes:\n    cowork://skills/example/SKILL.md: old\n",
        "deployments:\n- target: copilot-cowork\n  value: cowork://skills/example/SKILL.md\n",
        "deployments:\n  owner:\n  - target: copilot-cowork\n    value: cowork://skills/example/SKILL.md\n",
    ] {
        let mut lock: Value = crate::infra::yaml::from_str(record).unwrap();
        let mut names = BTreeSet::new();
        assert!(strip_legacy_cowork_deployments(&mut lock, &mut names));
        assert_eq!(names, BTreeSet::from(["example".to_string()]));
    }
    let mut lock: Value = crate::infra::yaml::from_str(
        "dependencies:\n- deployed_files: [.agents/skills/personal/SKILL.md]\n\
             deployments:\n- value: cowork://skills/../SKILL.md\n",
    )
    .unwrap();
    let mut names = BTreeSet::new();
    assert!(strip_legacy_cowork_deployments(&mut lock, &mut names));
    assert!(names.is_empty());
}

#[test]
fn legacy_hash_cleanup_preserves_other_targets_and_is_idempotent() {
    let retained = "dependencies:\n- deployed_file_hashes:\n    \
            .agents/skills/personal/SKILL.md: shared\n    \
            42: [unrelated, values]\n    \
            copilot-app-db://workflows/example: workflow\n";
    let mut lock: Value = crate::infra::yaml::from_str(&format!(
        "{retained}    cowork://skills/example/SKILL.md: old\n    \
             cowork://skills/../SKILL.md: unsafe\n    \
             cowork://other/record: legacy\n"
    ))
    .unwrap();
    let mut names = BTreeSet::new();

    assert!(strip_legacy_cowork_deployments(&mut lock, &mut names));
    assert_eq!(
        lock,
        crate::infra::yaml::from_str::<Value>(retained).unwrap()
    );
    assert_eq!(names, BTreeSet::from(["example".to_string()]));
    assert!(!strip_legacy_cowork_deployments(&mut lock, &mut names));
}

#[test]
fn legacy_deployment_classification_does_not_depend_on_a_cowork_uri() {
    for (record, expected) in [
        ("target: copilot-cowork", true),
        (
            "target: copilot\nvalue: .github/skills/example/SKILL.md",
            false,
        ),
        ("value: .agents/skills/example/SKILL.md", false),
        ("{}", false),
    ] {
        let value = crate::infra::yaml::from_str::<Value>(record).unwrap();
        let mut names = BTreeSet::new();
        assert_eq!(
            record_legacy_cowork_deployment(&value, &mut names),
            expected,
            "{record}"
        );
        assert!(
            names.is_empty(),
            "only a Cowork URI establishes named ownership"
        );
    }
}

#[test]
fn removes_legacy_cowork_lock_entries() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let (_, target_skill, ctx) = setup_skill(dir.path(), "[agent-skills]");
    std::fs::write(target_skill.join("SKILL.md"), "legacy deployment").unwrap();
    std::fs::write(
        dir.path().join(".apm").join("apm.lock.yaml"),
        "dependencies:\n\
             - deployed_files:\n\
             \x20 - .agents/skills/example/SKILL.md\n\
             \x20 - cowork://skills/example/SKILL.md\n\
             \x20 deployed_file_hashes:\n\
             \x20   cowork://skills/example/SKILL.md: stale\n\
             deployments:\n\
             - target: copilot-cowork\n\
             \x20 value: cowork://skills/example/SKILL.md\n",
    )
    .expect("write lock");

    assert!(remove_legacy_cowork_lock_deployments(&ctx).expect("remove legacy records"));
    let lock =
        std::fs::read_to_string(dir.path().join(".apm").join("apm.lock.yaml")).expect("read lock");
    assert!(!lock.contains("cowork://"));
    assert!(lock.contains(".agents/skills/example/SKILL.md"));
    assert_eq!(
        read_owned_skills(target_skill.parent().unwrap()).unwrap(),
        BTreeSet::from(["example".to_string()])
    );
    assert!(!remove_legacy_cowork_lock_deployments(&ctx).expect("repeat migration"));

    std::fs::write(
        dir.path().join(".apm").join("apm.lock.yaml"),
        "dependencies: []\n",
    )
    .expect("simulate native removal of dependency");
    let error = reconcile_cowork_skills(&ctx).unwrap_err();
    assert!(error.to_string().contains("without a verified fingerprint"));
    assert_eq!(
        std::fs::read_to_string(target_skill.join("SKILL.md")).unwrap(),
        "legacy deployment"
    );
    std::fs::remove_file(target_skill.join("SKILL.md")).unwrap();
    assert!(reconcile_cowork_skills(&ctx).expect("retry after moving the local file"));
}

#[test]
fn legacy_migration_does_not_discard_lock_evidence_if_inventory_cannot_be_written() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let (_, target_skill, ctx) = setup_skill(dir.path(), "[agent-skills]");
    let lock = "dependencies:\n  - deployed_files:\n      - cowork://skills/example/SKILL.md\n";
    let lock_path = dir.path().join(".apm").join("apm.lock.yaml");
    std::fs::write(&lock_path, lock).unwrap();
    std::fs::create_dir(target_skill.parent().unwrap().join(OWNERSHIP_FILE)).unwrap();

    assert!(remove_legacy_cowork_lock_deployments(&ctx).is_err());
    assert_eq!(std::fs::read_to_string(lock_path).unwrap(), lock);
}

#[test]
fn legacy_fingerprints_require_valid_agreeing_hashes() {
    let first = "ab".repeat(32);
    let other = "cd".repeat(32);
    let signed = format!("+b{}", "ab".repeat(31));
    for (second, verified) in [
        (first.as_str(), true),
        (other.as_str(), false),
        ("ab", false),
        (signed.as_str(), false),
    ] {
        let lock: Value = crate::infra::yaml::from_str(&format!(
                "dependencies:\n- deployed_file_hashes:\n    cowork://skills/example/SKILL.md: sha256:{first}\n- deployed_file_hashes:\n    cowork://skills/example/SKILL.md: sha256:{second}\n"
            )).unwrap();
        let fingerprints = legacy_entry_point_fingerprints(&lock);
        assert_eq!(
            fingerprints.get("example").map(Option::is_some),
            Some(verified),
            "{second}"
        );
    }
    assert_eq!(decode_sha256_hex(&first), Some([0xab; 32]));
    assert_eq!(decode_sha256_hex("ab"), None);
    assert_eq!(decode_sha256_hex(&signed), None);
}

#[test]
fn legacy_cleanup_requires_a_matching_deployment_hash() {
    use sha2::{Digest as _, Sha256};

    for modified in [false, true] {
        let dir = tempfile::tempdir().expect("create temp dir");
        let (_, target_skill, ctx) = setup_skill(dir.path(), "[agent-skills]");
        let lock_path = dir.path().join(".apm").join("apm.lock.yaml");
        let hash = Sha256::digest(b"deployed")
            .iter()
            .fold(String::new(), |mut hex, byte| {
                use std::fmt::Write as _;
                write!(hex, "{byte:02x}").unwrap();
                hex
            });
        std::fs::write(
                &lock_path,
                format!("dependencies:\n  - deployed_file_hashes:\n      cowork://skills/example/SKILL.md: sha256:{hash}\n"),
            ).unwrap();
        let entry = target_skill.join("SKILL.md");
        std::fs::write(&entry, if modified { "edited" } else { "deployed" }).unwrap();

        let result = reconcile_cowork_skills(&ctx);
        if modified {
            assert!(result.unwrap_err().to_string().contains("modified stale"));
            assert_eq!(std::fs::read_to_string(entry).unwrap(), "edited");
        } else {
            assert!(result.unwrap());
            assert!(!entry.exists());
            assert!(!reconcile_cowork_skills(&ctx).unwrap());
        }
        assert!(
            !std::fs::read_to_string(lock_path)
                .unwrap()
                .contains("cowork://")
        );
    }
}
