use super::*;

#[test]
fn overlapping_cowork_destination_never_claims_or_prunes_shared_skills() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let home = fixture.path();
    let (source, target, ctx) = setup_skill(home, "[agent-skills, copilot-cowork]");
    let shared = source.parent().unwrap();
    let lock = home.join(".apm/apm.lock.yaml");
    let before = std::fs::read(&lock).unwrap();
    let nested = source.join("missing/new");
    #[cfg(unix)]
    std::os::unix::fs::symlink(shared, home.join("alias")).unwrap();
    for destination in [
        shared.to_path_buf(),
        home.join(".agents"),
        nested.clone(),
        #[cfg(unix)]
        home.join("alias/example/nested"),
    ] {
        let bad = make_context_with_home(home, ctx.platform(), MockExecutor::new()).with_env(
            MapEnv::new()
                .with("APM_COPILOT_COWORK_SKILLS_DIR", destination)
                .into_handle(),
        );
        assert!(super::super::super::targets::ApmTargets::detect(&bad).is_err());
        assert!(reconcile_cowork_skills(&bad).is_err());
        assert!(!shared.join(OWNERSHIP_FILE).exists());
        assert!(!shared.join(FILE_OWNERSHIP_FILE).exists());
        assert!(!nested.exists());
        assert_eq!(std::fs::read(&lock).unwrap(), before);
        assert_eq!(std::fs::read(source.join("SKILL.md")).unwrap(), b"current");
    }
    assert!(reconcile_cowork_skills(&ctx).unwrap());
    std::fs::write(&lock, "dependencies: []\n").unwrap();
    assert!(reconcile_cowork_skills(&ctx).unwrap());
    assert!(!target.join("SKILL.md").exists());
    assert_eq!(std::fs::read(source.join("SKILL.md")).unwrap(), b"current");
}

#[test]
fn reconcile_updates_files_without_replacing_cowork_directory() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let (_, target_skill, ctx) = setup_skill(dir.path(), "[agent-skills, copilot-cowork]");
    std::fs::write(target_skill.join("placeholder.txt"), "preserved").expect("write placeholder");

    reconcile_cowork_skills(&ctx).expect("reconcile skills");

    assert_eq!(
        std::fs::read_to_string(target_skill.join("SKILL.md")).expect("read target"),
        "current"
    );
    assert!(target_skill.join("placeholder.txt").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn updating_an_owned_script_preserves_its_destination_acl_owner_and_execute_mode() {
    use rustix::fs::{XattrFlags, fgetxattr, fsetxattr};
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    let dir = tempfile::tempdir_in(".").unwrap();
    let (source, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
    let script = source.join("run");
    let installed = target.join("run");
    std::fs::write(&script, "first").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(reconcile_cowork_skills(&ctx).unwrap());
    assert_eq!(installed.metadata().unwrap().mode() & 0o777, 0o755);

    // A named ACL on this synthetic target cannot be retained just by
    // copying Unix mode bits. No account lookup or ownership change.
    let mut acl = 2_u32.to_le_bytes().to_vec();
    for (tag, rights, id) in [
        (1_u16, 7_u16, u32::MAX),
        (2, 4, 12_345),
        (4, 0, u32::MAX),
        (16, 4, u32::MAX),
        (32, 0, u32::MAX),
    ] {
        acl.extend(tag.to_le_bytes());
        acl.extend(rights.to_le_bytes());
        acl.extend(id.to_le_bytes());
    }
    let attr = "system.posix_acl_access";
    fsetxattr(
        std::fs::File::open(&installed).unwrap(),
        attr,
        &acl,
        XattrFlags::empty(),
    )
    .unwrap();
    let before = installed.metadata().unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o750)).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::write(&script, "updated").unwrap();

    assert!(reconcile_cowork_skills(&ctx).unwrap());
    assert_eq!(std::fs::read(&installed).unwrap(), b"updated");
    let after = installed.metadata().unwrap();
    assert_eq!((after.uid(), after.gid()), (before.uid(), before.gid()));
    assert_eq!(after.mode() & 0o777, 0o740);
    let mut retained = vec![0; acl.len()];
    assert_eq!(
        fgetxattr(
            std::fs::File::open(&installed).unwrap(),
            attr,
            retained.as_mut_slice()
        )
        .unwrap(),
        acl.len()
    );
    assert_eq!(retained, acl);
    assert_eq!(target.metadata().unwrap().mode() & 0o777, 0o750);
    assert!(!reconcile_cowork_skills(&ctx).unwrap());
    std::fs::write(dir.path().join(".apm/apm.lock.yaml"), "dependencies: []\n").unwrap();
    assert!(reconcile_cowork_skills(&ctx).unwrap());
    assert!(!installed.exists());
}

#[cfg(windows)]
#[test]
fn update_refuses_a_read_only_cowork_target_without_replacing_or_disowning_it() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let (source, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
    assert!(reconcile_cowork_skills(&ctx).unwrap());
    let installed = target.join("SKILL.md");
    let original_policy = installed.metadata().unwrap().permissions();
    let mut restricted = original_policy.clone();
    restricted.set_readonly(true);
    std::fs::set_permissions(&installed, restricted).unwrap();
    let root = target.parent().unwrap();
    let ledger = std::fs::read(root.join(FILE_OWNERSHIP_FILE)).unwrap();
    std::fs::write(source.join("SKILL.md"), "updated").unwrap();

    let error = reconcile_cowork_skills(&ctx).unwrap_err();
    assert!(
        format!("{error:#}").contains("read-only target"),
        "{error:#}"
    );
    assert_eq!(std::fs::read(&installed).unwrap(), b"current");
    assert!(installed.metadata().unwrap().permissions().readonly());
    assert_eq!(
        std::fs::read(root.join(FILE_OWNERSHIP_FILE)).unwrap(),
        ledger
    );
    assert_eq!(std::fs::read_dir(&target).unwrap().count(), 1);
    std::fs::set_permissions(&installed, original_policy).unwrap();
    assert!(reconcile_cowork_skills(&ctx).unwrap());
    assert_eq!(std::fs::read(&installed).unwrap(), b"updated");
    assert!(!reconcile_cowork_skills(&ctx).unwrap());
}

#[test]
fn interrupted_shared_skill_preserves_the_installed_entry_point_until_retry() {
    let dir = tempfile::tempdir().unwrap();
    let (source, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
    std::fs::write(source.join("guide"), "previous").unwrap();
    assert!(reconcile_cowork_skills(&ctx).unwrap());
    std::fs::write(target.join("notes"), "my customization").unwrap();
    let names = target.parent().unwrap().join(OWNERSHIP_FILE);
    let files = target.parent().unwrap().join(FILE_OWNERSHIP_FILE);
    let before_names = std::fs::read(&names).unwrap();
    let before_files = std::fs::read(&files).unwrap();
    std::fs::remove_file(source.join("SKILL.md")).unwrap();
    std::fs::remove_file(source.join("guide")).unwrap();

    assert!(reconcile_cowork_skills(&ctx).is_err());
    assert_eq!(std::fs::read(target.join("SKILL.md")).unwrap(), b"current");
    assert_eq!(std::fs::read(target.join("guide")).unwrap(), b"previous");
    assert_eq!(std::fs::read(&names).unwrap(), before_names);
    assert_eq!(std::fs::read(&files).unwrap(), before_files);
    std::fs::write(source.join("SKILL.md"), "restored").unwrap();
    assert!(reconcile_cowork_skills(&ctx).unwrap());
    assert_eq!(std::fs::read(target.join("SKILL.md")).unwrap(), b"restored");
    assert!(!target.join("guide").exists());
    assert_eq!(
        std::fs::read(target.join("notes")).unwrap(),
        b"my customization"
    );
    assert!(!reconcile_cowork_skills(&ctx).unwrap());
}

#[cfg(unix)]
#[test]
fn disconnected_apm_config_does_not_redirect_skills_to_onedrive() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let (_, fallback, ctx) = setup_skill(home, "[copilot-cowork]");
    let selected = home.join("selected");
    let settings = home.join("settings.json");
    let link = home.join(".apm/config.json");
    std::os::unix::fs::symlink(&settings, &link).unwrap();
    assert!(super::super::super::targets::ApmTargets::detect(&ctx).is_err());
    assert!(reconcile_cowork_skills(&ctx).is_err());
    assert!(!fallback.join("SKILL.md").exists());
    assert!(!fallback.parent().unwrap().join(OWNERSHIP_FILE).exists());
    let explicit = make_context_with_home(home, ctx.platform(), MockExecutor::new()).with_env(
        MapEnv::new()
            .with("APM_COPILOT_COWORK_SKILLS_DIR", &selected)
            .into_handle(),
    );
    assert_eq!(
        copilot_cowork_skills_path(&explicit).unwrap(),
        Some(selected.clone())
    );

    std::fs::write(
        &settings,
        serde_json::json!({ "copilot_cowork_skills_dir": selected }).to_string(),
    )
    .unwrap();
    assert!(reconcile_cowork_skills(&ctx).unwrap());
    assert_eq!(
        std::fs::read(selected.join("example/SKILL.md")).unwrap(),
        b"current"
    );
    assert_eq!(std::fs::read_link(&link).unwrap(), settings);
    assert!(!fallback.join("SKILL.md").exists());
    assert!(!reconcile_cowork_skills(&ctx).unwrap());
}

#[test]
fn reconcile_never_replaces_a_cowork_directory_with_a_file() {
    let dir = tempfile::tempdir_in(".").expect("create fixture");
    let (_, target, ctx) = setup_skill(dir.path(), "[agent-skills, copilot-cowork]");
    let directory = target.join("SKILL.md");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("preserved"), "unmanaged").unwrap();

    let error = reconcile_cowork_skills(&ctx).expect_err("directory must be preserved");
    assert!(format!("{error:#}").contains("cannot replace Cowork-owned directory"));
    assert_eq!(
        std::fs::read_to_string(directory.join("preserved")).unwrap(),
        "unmanaged"
    );
}

#[test]
fn reconcile_removes_entry_point_when_package_excludes_cowork() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let (_, target_skill, ctx) = setup_skill(dir.path(), "[agent-skills, copilot-cowork]");
    reconcile_cowork_skills(&ctx).expect("establish ownership");
    std::fs::write(target_skill.join("placeholder.txt"), "preserved").expect("write placeholder");
    std::fs::write(
        dir.path().join(".apm").join("apm.lock.yaml"),
        "dependencies:\n  - deployed_files:\n      - .agents/skills/example/SKILL.md\n\
             \x20   target_subset: [agent-skills]\n",
    )
    .expect("exclude Cowork");

    assert!(reconcile_cowork_skills(&ctx).expect("reconcile skills"));

    assert!(!target_skill.join("SKILL.md").exists());
    assert!(target_skill.join("placeholder.txt").exists());
    assert!(!reconcile_cowork_skills(&ctx).expect("repeat reconciliation"));
}

#[test]
fn reconcile_preserves_unmanaged_skills_including_excluded_shared_names() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let (_, target_skill, ctx) = setup_skill(dir.path(), "[agent-skills]");
    std::fs::write(target_skill.join("SKILL.md"), "independent example")
        .expect("write independent skill");
    let personal = target_skill.parent().unwrap().join("personal");
    std::fs::create_dir_all(&personal).expect("create personal skill");
    std::fs::write(personal.join("SKILL.md"), "independent personal")
        .expect("write personal skill");

    assert!(!reconcile_cowork_skills(&ctx).expect("reconcile skills"));
    assert_eq!(
        std::fs::read_to_string(target_skill.join("SKILL.md")).unwrap(),
        "independent example"
    );
    assert_eq!(
        std::fs::read_to_string(personal.join("SKILL.md")).unwrap(),
        "independent personal"
    );
    assert!(!target_skill.parent().unwrap().join(OWNERSHIP_FILE).exists());
}

#[test]
fn reconcile_removes_owned_skills_after_shared_source_and_lock_entry_disappear() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let (source_skill, target_skill, ctx) =
        setup_skill(dir.path(), "[agent-skills, copilot-cowork]");
    assert!(reconcile_cowork_skills(&ctx).expect("install skill"));
    assert!(!reconcile_cowork_skills(&ctx).expect("repeat install"));
    std::fs::remove_dir_all(source_skill.parent().unwrap()).expect("remove shared skills");
    std::fs::write(
        dir.path().join(".apm").join("apm.lock.yaml"),
        "dependencies: []\n",
    )
    .expect("remove dependency");

    assert!(reconcile_cowork_skills(&ctx).expect("remove owned skill"));
    assert!(!target_skill.join("SKILL.md").exists());
    assert!(target_skill.is_dir());
    assert!(!reconcile_cowork_skills(&ctx).expect("repeat removal"));
}

#[test]
fn owned_file_and_directory_transitions_preserve_local_content_and_retry() {
    let dir = tempfile::tempdir().unwrap();
    let (source_skill, target_skill, ctx) =
        setup_skill(dir.path(), "[agent-skills, copilot-cowork]");
    let source = source_skill.join("asset");
    let target = target_skill.join("asset");
    std::fs::write(&source, "file").unwrap();
    assert!(reconcile_cowork_skills(&ctx).unwrap());

    std::fs::remove_file(&source).unwrap();
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("nested"), "nested").unwrap();
    std::fs::write(&target, "edited").unwrap();
    assert!(reconcile_cowork_skills(&ctx).is_err());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "edited");
    std::fs::write(&target, "file").unwrap();
    assert!(reconcile_cowork_skills(&ctx).unwrap());
    assert_eq!(
        std::fs::read_to_string(target.join("nested")).unwrap(),
        "nested"
    );

    std::fs::remove_dir_all(&source).unwrap();
    std::fs::write(&source, "new file").unwrap();
    std::fs::write(target.join("local"), "keep").unwrap();
    assert!(reconcile_cowork_skills(&ctx).is_err());
    assert_eq!(
        std::fs::read_to_string(target.join("local")).unwrap(),
        "keep"
    );
    assert_eq!(
        std::fs::read_to_string(target.join("nested")).unwrap(),
        "nested"
    );
    std::fs::remove_file(target.join("local")).unwrap();
    assert!(reconcile_cowork_skills(&ctx).unwrap());
    assert_eq!(std::fs::read_to_string(target).unwrap(), "new file");
    assert!(!reconcile_cowork_skills(&ctx).unwrap());
}
