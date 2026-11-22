use super::*;

#[cfg(any(unix, windows))]
fn create_directory_link(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        use crate::infra::exec::{ProcessExecutor, windows::CmdCommand};

        let result = CmdCommand::new("mklink")
            .arg("/J")
            .arg(std::path::absolute(link).unwrap().to_str().unwrap())
            .arg(std::path::absolute(target).unwrap().to_str().unwrap())
            .run_unchecked(&ProcessExecutor::system())
            .unwrap();
        assert!(result.success, "fixture junction creation: {result:?}");
    }
}

#[cfg(any(unix, windows))]
fn remove_directory_link(link: &Path) {
    if is_dir_like(&link.symlink_metadata().unwrap()) {
        std::fs::remove_dir(link).unwrap();
    } else {
        std::fs::remove_file(link).unwrap();
    }
}

#[test]
#[cfg(any(unix, windows))]
fn matching_and_copying_reject_root_and_nested_directory_links() {
    for nested in [false, true] {
        let dir = tempfile::tempdir_in(".").unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let source = root.join("source");
        let target = root.join("target");
        let unrelated = root.join("unrelated");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&unrelated).unwrap();
        std::fs::write(unrelated.join("SKILL.md"), "preserve").unwrap();
        let permissions = unrelated.metadata().unwrap().permissions();
        let link = if nested {
            std::fs::create_dir(source.join("nested")).unwrap();
            std::fs::write(source.join("nested/SKILL.md"), "desired").unwrap();
            std::fs::create_dir(&target).unwrap();
            target.join("nested")
        } else {
            std::fs::write(source.join("SKILL.md"), "desired").unwrap();
            target.clone()
        };
        create_directory_link(&unrelated, &link);

        assert!(
            skill_files_match(&source, &target).is_err(),
            "nested={nested}"
        );
        assert!(
            copy_skill_files(&source, &target).is_err(),
            "nested={nested}"
        );
        assert_eq!(
            std::fs::read_to_string(unrelated.join("SKILL.md")).unwrap(),
            "preserve"
        );
        assert_eq!(unrelated.metadata().unwrap().permissions(), permissions);
        assert!(std::fs::read_link(&link).is_ok());
        remove_directory_link(&link);
    }
}

#[test]
#[cfg(any(unix, windows))]
fn reconciliation_rejects_a_linked_cowork_root_before_reading_ownership() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let (_, target, ctx) = setup_skill(&root, "[agent-skills, copilot-cowork]");
    let cowork_root = target.parent().unwrap();
    let unrelated = root.join("unrelated");
    std::fs::rename(cowork_root, &unrelated).unwrap();
    std::fs::write(unrelated.join(OWNERSHIP_FILE), "[\"other\"]").unwrap();
    create_directory_link(&unrelated, cowork_root);

    assert!(reconcile_cowork_skills(&ctx).is_err());
    assert_eq!(
        std::fs::read_to_string(unrelated.join(OWNERSHIP_FILE)).unwrap(),
        "[\"other\"]"
    );
    assert!(!unrelated.join("example/SKILL.md").exists());
    remove_directory_link(cowork_root);
}

#[test]
#[cfg(windows)]
fn matching_and_copying_reject_a_junction_at_a_file_destination() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let source = dir.path().join("SKILL.md");
    let target = dir.path().join("target");
    let unrelated = dir.path().join("unrelated");
    std::fs::write(&source, "desired").unwrap();
    std::fs::create_dir(&unrelated).unwrap();
    std::fs::write(unrelated.join("preserved"), "unmanaged").unwrap();
    create_directory_link(&unrelated, &target);

    assert!(skill_files_match(&source, &target).is_err());
    assert!(copy_skill_files(&source, &target).is_err());
    assert_eq!(
        std::fs::read_to_string(unrelated.join("preserved")).unwrap(),
        "unmanaged"
    );
    assert!(std::fs::read_link(&target).is_ok());
    remove_directory_link(&target);
}

#[test]
#[cfg(any(unix, windows))]
fn stale_cleanup_refuses_a_linked_grandparent() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let (source, target, ctx) = setup_skill(&root, "[copilot-cowork]");
    let stale = source.join("nested/scripts/old.py");
    std::fs::create_dir_all(stale.parent().unwrap()).unwrap();
    std::fs::write(&stale, "managed").unwrap();
    assert!(reconcile_cowork_skills(&ctx).unwrap());

    let outside = root.join("outside");
    let linked = target.join("nested");
    std::fs::rename(&linked, &outside).unwrap();
    create_directory_link(&outside, &linked);
    std::fs::remove_file(stale).unwrap();

    let error = reconcile_cowork_skills(&ctx).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("refusing to copy through destination link")
    );
    assert_eq!(
        std::fs::read_to_string(outside.join("scripts/old.py")).unwrap(),
        "managed"
    );
    remove_directory_link(&linked);
}

#[test]
#[cfg(any(unix, windows))]
fn file_to_link_requires_move_aside_before_any_reconciliation_writes() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let (first, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
    let last = first.with_file_name("z-last");
    let guide = last.join("docs/guide");
    std::fs::create_dir_all(guide.parent().unwrap()).unwrap();
    std::fs::write(last.join("SKILL.md"), "new reference").unwrap();
    std::fs::write(&guide, "installed guide").unwrap();
    std::fs::write(first.join("stale"), "keep until retry").unwrap();
    let lock = dir.path().join(".apm/apm.lock.yaml");
    let selection = "dependencies:\n- deployed_files:\n  - .agents/skills/example/SKILL.md\n  - .agents/skills/z-last/SKILL.md\n";
    std::fs::write(&lock, selection).unwrap();
    assert!(reconcile_cowork_skills(&ctx).unwrap());
    let root = target.parent().unwrap();
    let installed = root.join("z-last").join("docs").join("guide");
    let names = std::fs::read(root.join(OWNERSHIP_FILE)).unwrap();
    let files = std::fs::read(root.join(FILE_OWNERSHIP_FILE)).unwrap();
    let legacy = format!("{selection}  - cowork://skills/retired/SKILL.md\n");
    std::fs::write(&lock, &legacy).unwrap();
    std::fs::remove_file(first.join("stale")).unwrap();
    std::fs::write(first.join("fresh"), "pending").unwrap();
    std::fs::remove_file(&guide).unwrap();
    create_native_symlink(&Path::new("..").join("SKILL.md"), &guide, false).unwrap();

    let error = reconcile_cowork_skills(&ctx).unwrap_err().to_string();
    assert!(error.contains(&installed.display().to_string()), "{error}");
    assert!(error.contains("move it aside and retry"), "{error}");
    assert!(installed.symlink_metadata().unwrap().is_file());
    assert_eq!(std::fs::read(&installed).unwrap(), b"installed guide");
    assert_eq!(
        std::fs::read(target.join("stale")).unwrap(),
        b"keep until retry"
    );
    assert!(!target.join("fresh").exists());
    assert_eq!(std::fs::read(root.join(OWNERSHIP_FILE)).unwrap(), names);
    assert_eq!(
        std::fs::read(root.join(FILE_OWNERSHIP_FILE)).unwrap(),
        files
    );
    assert_eq!(std::fs::read_to_string(&lock).unwrap(), legacy);

    let saved = dir.path().join("saved-guide");
    std::fs::rename(&installed, &saved).unwrap();
    assert!(reconcile_cowork_skills(&ctx).unwrap());
    assert!(installed.symlink_metadata().unwrap().is_symlink());
    assert_eq!(std::fs::read(&installed).unwrap(), b"new reference");
    assert_eq!(std::fs::read(&saved).unwrap(), b"installed guide");
    assert!(!target.join("stale").exists());
    assert_eq!(std::fs::read(target.join("fresh")).unwrap(), b"pending");
    assert!(!reconcile_cowork_skills(&ctx).unwrap());
}

#[test]
#[cfg(any(unix, windows))]
fn relocated_skill_entry_and_asset_links_read_from_the_cowork_copy() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let (source, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
    std::fs::create_dir(source.join("docs")).unwrap();
    std::fs::create_dir(source.join("assets")).unwrap();
    std::fs::rename(source.join("SKILL.md"), source.join("docs/entry.md")).unwrap();
    std::fs::write(source.join("assets/guide"), "copied guide").unwrap();
    create_native_symlink(
        &Path::new("docs").join("entry.md"),
        &source.join("SKILL.md"),
        false,
    )
    .unwrap();
    create_native_symlink(Path::new("assets"), &source.join("library"), true).unwrap();
    let reference = Path::new("..").join("library").join("guide");
    create_native_symlink(&reference, &source.join("docs/reference"), false).unwrap();

    assert!(reconcile_cowork_skills(&ctx).unwrap());
    assert!(!reconcile_cowork_skills(&ctx).unwrap());
    std::fs::rename(&source, dir.path().join("source-offline")).unwrap();
    assert_eq!(std::fs::read(target.join("SKILL.md")).unwrap(), b"current");
    assert_eq!(
        std::fs::read(target.join("docs/reference")).unwrap(),
        b"copied guide"
    );
    assert_eq!(
        std::fs::read_link(target.join("docs/reference")).unwrap(),
        reference
    );
}

#[test]
#[cfg(any(unix, windows))]
fn nonportable_skill_links_reject_the_batch_before_any_cowork_writes() {
    for (entry, payload) in [
        ("SKILL.md", "../common/SKILL.md"),
        ("asset", "../common/SKILL.md"),
        ("asset", "../z-last/SKILL.md"),
        ("asset", "absolute"),
        ("asset", "missing"),
        ("asset", "asset"),
    ] {
        let dir = tempfile::tempdir_in(".").unwrap();
        let (first, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
        let last = first.with_file_name("z-last");
        let common = first.with_file_name("common");
        std::fs::create_dir(&last).unwrap();
        std::fs::create_dir(&common).unwrap();
        std::fs::write(common.join("SKILL.md"), "external").unwrap();
        std::fs::write(last.join("SKILL.md"), "installed").unwrap();
        std::fs::write(first.join("old"), "retain until safe").unwrap();
        let lock = dir.path().join(".apm/apm.lock.yaml");
        let selection = "dependencies:\n- deployed_files:\n  - .agents/skills/example/SKILL.md\n  - .agents/skills/z-last/SKILL.md\n";
        std::fs::write(&lock, selection).unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        let root = target.parent().unwrap();
        let names = std::fs::read(root.join(OWNERSHIP_FILE)).unwrap();
        let files = std::fs::read(root.join(FILE_OWNERSHIP_FILE)).unwrap();
        let legacy = format!("{selection}  - cowork://skills/old/SKILL.md\n");
        std::fs::write(&lock, &legacy).unwrap();
        std::fs::write(first.join("SKILL.md"), "pending update").unwrap();
        std::fs::remove_file(first.join("old")).unwrap();
        if entry == "SKILL.md" {
            std::fs::remove_file(last.join(entry)).unwrap();
        }
        let destination = if payload == "absolute" {
            std::fs::canonicalize(last.join("SKILL.md")).unwrap()
        } else {
            payload.split('/').collect()
        };
        create_native_symlink(&destination, &last.join(entry), false).unwrap();
        if payload == "../common/SKILL.md" {
            assert_eq!(std::fs::read(last.join(entry)).unwrap(), b"external");
        }

        let error = reconcile_cowork_skills(&ctx).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("cannot relocate Cowork skill link"),
            "{entry} -> {payload}: {error:#}"
        );
        assert_eq!(std::fs::read(target.join("SKILL.md")).unwrap(), b"current");
        assert_eq!(
            std::fs::read(target.join("old")).unwrap(),
            b"retain until safe"
        );
        assert_eq!(
            std::fs::read(root.join("z-last/SKILL.md")).unwrap(),
            b"installed"
        );
        assert!(
            root.join("z-last/asset")
                .symlink_metadata()
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        );
        assert_eq!(std::fs::read(root.join(OWNERSHIP_FILE)).unwrap(), names);
        assert_eq!(
            std::fs::read(root.join(FILE_OWNERSHIP_FILE)).unwrap(),
            files
        );
        assert_eq!(std::fs::read_to_string(&lock).unwrap(), legacy);
    }
}

#[test]
#[cfg(unix)]
fn reconcile_updates_skill_files_with_existing_symlinks() {
    let dir = tempfile::tempdir_in(".").expect("create fixture");
    let (source, target, ctx) = setup_skill(dir.path(), "[agent-skills, copilot-cowork]");
    std::os::unix::fs::symlink("SKILL.md", source.join("reference")).unwrap();
    std::fs::write(target.join("cowork-owned.txt"), "preserve").unwrap();
    assert!(reconcile_cowork_skills(&ctx).expect("first deployment"));

    std::fs::write(source.join("SKILL.md"), "updated").unwrap();
    assert!(reconcile_cowork_skills(&ctx).expect("update with retained symlink"));
    assert_eq!(
        std::fs::read_to_string(target.join("SKILL.md")).unwrap(),
        "updated"
    );
    assert_eq!(
        std::fs::read_link(target.join("reference")).unwrap(),
        Path::new("SKILL.md")
    );
    assert_eq!(
        std::fs::read_to_string(target.join("cowork-owned.txt")).unwrap(),
        "preserve"
    );
    assert!(!reconcile_cowork_skills(&ctx).expect("repeat deployment"));
}

#[test]
#[cfg(unix)]
fn reconcile_retargets_owned_links_and_preserves_modified_destination_links() {
    let dir = tempfile::tempdir_in(".").expect("create fixture");
    let (source, target, ctx) = setup_skill(dir.path(), "[agent-skills, copilot-cowork]");
    std::os::unix::fs::symlink("SKILL.md", source.join("reference")).unwrap();
    assert!(reconcile_cowork_skills(&ctx).expect("first deployment"));
    let external = std::fs::canonicalize(dir.path())
        .unwrap()
        .join("external.md");
    std::fs::write(&external, "unmanaged").unwrap();
    std::fs::remove_file(target.join("SKILL.md")).unwrap();
    std::os::unix::fs::symlink(&external, target.join("SKILL.md")).unwrap();
    std::fs::remove_file(source.join("reference")).unwrap();
    std::os::unix::fs::symlink("new.md", source.join("reference")).unwrap();
    std::fs::write(source.join("new.md"), "new reference").unwrap();

    let error = reconcile_cowork_skills(&ctx).expect_err("keep the modified link");
    assert!(error.to_string().contains("modified Cowork file"));
    assert_eq!(
        std::fs::read_link(target.join("SKILL.md")).unwrap(),
        external
    );
    std::fs::remove_file(target.join("SKILL.md")).unwrap();
    assert!(reconcile_cowork_skills(&ctx).expect("restore missing file and retarget owned link"));
    assert_eq!(std::fs::read_to_string(&external).unwrap(), "unmanaged");
    assert!(
        !target
            .join("SKILL.md")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read_link(target.join("reference")).unwrap(),
        Path::new("new.md")
    );
    assert!(!reconcile_cowork_skills(&ctx).expect("repeat repair"));
}
