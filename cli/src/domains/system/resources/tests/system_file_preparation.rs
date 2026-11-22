use super::*;

#[cfg(unix)]
#[test]
fn privileged_fragment_read_rejects_changed_content_without_exposing_it() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let path = fixture.path().join("fragment");
    let fragment = "managed = true\n";
    let fingerprint = crate::infra::fs::content_hash(fragment);
    fs::write(&path, fragment).unwrap();
    assert_eq!(verified_fragment(&path, &fingerprint).unwrap(), fragment);
    fs::write(&path, "private = 'substituted fixture secret'\n").unwrap();
    let error = verified_fragment(&path, &fingerprint).unwrap_err();
    assert!(error.to_string().contains("fragment changed"));
    assert!(!format!("{error:#}").contains("substituted fixture secret"));
}

#[cfg(unix)]
#[test]
fn fragment_reads_reject_pipes_and_accept_symlinks_to_regular_files() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let root = fixture.path().canonicalize().unwrap();
    let system = root.join("system");
    fs::create_dir(&system).unwrap();
    let source = system.join("fragment");
    let pipe = system.join("pipe");
    nix::unistd::mkfifo(&pipe, nix::sys::stat::Mode::S_IRUSR).unwrap();
    std::os::unix::fs::symlink("pipe", &source).unwrap();
    let content = "managed = true\n";
    let fingerprint = crate::infra::fs::content_hash(content);
    let target = root.join("target");
    fs::write(&target, content).unwrap();
    let resource = fixture_resource(
        entry(&root, &target, "fragment", MergeStrategy::Toml),
        Arc::new(MockExecutor::new()),
    );

    for error in [
        resource.current_state().unwrap_err(),
        resource.apply().unwrap_err(),
    ] {
        assert!(
            format!("{error:#}").contains("unsupported special file"),
            "{error:#}"
        );
    }
    let error = verified_fragment(&source, &fingerprint).unwrap_err();
    assert_eq!(error.to_string(), "could not read system file fragment");
    assert_eq!(fs::read_to_string(&target).unwrap(), content);

    fs::write(system.join("regular"), content).unwrap();
    fs::remove_file(&source).unwrap();
    std::os::unix::fs::symlink("regular", &source).unwrap();
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    assert_eq!(resource.apply().unwrap(), ResourceChange::AlreadyCorrect);
    assert_eq!(verified_fragment(&source, &fingerprint).unwrap(), content);
}

#[cfg(unix)]
#[test]
fn standalone_helper_rejects_unprivileged_calls_and_out_of_scope_targets_before_reading() {
    let error = privileged_prepare(
        Path::new("/outside/etc/target"),
        Path::new("source-must-not-be-read"),
        "fingerprint",
        "toml",
        Path::new("stage-must-not-be-created"),
    )
    .unwrap_err();
    let reason = if nix::unistd::Uid::effective().is_root() {
        "target must be below /etc"
    } else {
        "helper requires root"
    };
    assert!(error.to_string().contains(reason), "{error}");
}

#[cfg(unix)]
#[test]
fn private_preparation_merges_fragments_then_converges_without_new_staged_contents() {
    use std::os::unix::fs::PermissionsExt as _;

    for (strategy, original, fragment, expected) in [
        (
            "toml",
            "managed = 1.0\nkeep = 'private'\n",
            "managed = nan\n",
            "managed = nan\nkeep = 'private'\n",
        ),
        (
            "ini",
            "# administrator\r\n[boot]\r\nsystemd=false\r\n\n[network]\r\n# retained",
            "[boot]\nsystemd=true\n",
            "# administrator\r\n[boot]\r\nsystemd=true\r\n\n[network]\r\n# retained",
        ),
        (
            "ini",
            "# retained\n[options]\nkeep=private\nmanaged=no\n",
            "[options]\nmanaged=yes\n",
            "# retained\n[options]\nkeep=private\nmanaged=yes\n",
        ),
        (
            "pam",
            "# retained\nauth include system-auth\nsession include system-login\n",
            "auth optional fixture.so\n",
            "# retained\nauth include system-auth\nauth optional fixture.so\nsession include system-login\n",
        ),
    ] {
        let fixture = PrivateStage::new("target");
        fixture.write_target(original);
        fs::set_permissions(&fixture.target, fs::Permissions::from_mode(0o600)).unwrap();
        let prepare = || fixture.prepare(fragment, strategy).unwrap();

        assert_eq!(prepare(), "prepared", "{strategy}");
        assert_eq!(fixture.staged_contents("input"), expected, "{strategy}");
        assert_eq!(fixture.staged_contents("original"), original, "{strategy}");
        assert_eq!(fixture.target_contents(), original, "{strategy}");
        fixture.write_target(expected);
        fs::remove_file(fixture.stage.join("input")).unwrap();
        fs::remove_file(fixture.stage.join("original")).unwrap();
        assert_eq!(prepare(), "already-correct", "{strategy}");
        assert!(fixture.stage_is_empty(), "{strategy}");
    }
}

#[cfg(unix)]
#[test]
fn preparation_keeps_original_and_merged_contents_in_verified_private_stage() {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    let fixture = PrivateStage::new("target");
    let original = "# confidential\nprivate = 'fixture secret'\nmanaged = false\n";
    fixture.write_target(original);
    fs::set_permissions(&fixture.target, fs::Permissions::from_mode(0o600)).unwrap();
    let prepare = || fixture.prepare("managed = true\n", "toml").unwrap();
    assert_eq!(prepare(), "prepared");
    assert_eq!(fixture.staged_contents("original"), original);
    let merged = fixture.staged_contents("input");
    assert!(merged.contains("private = 'fixture secret'"));
    assert!(merged.contains("# confidential"));
    for name in ["input", "original"] {
        let metadata = fs::metadata(fixture.stage.join(name)).unwrap();
        assert_eq!(metadata.uid(), fixture.owner);
        assert_eq!(metadata.mode() & 0o777, 0o600);
        fs::remove_file(fixture.stage.join(name)).unwrap();
    }
    assert_eq!(fixture.target_contents(), original);
    fixture.write_target(&merged);
    assert_eq!(prepare(), "already-correct");
    assert!(fixture.stage_is_empty());
}

#[cfg(unix)]
#[test]
fn preparation_refuses_public_or_linked_staging_and_sanitizes_target_parse_errors() {
    use std::os::unix::fs::PermissionsExt as _;

    let fixture = PrivateStage::new("target");
    let original = "private = 'fixture secret'\nmanaged = false\n";
    fixture.write_target(original);
    let fragment = "managed = true\n";
    fs::set_permissions(&fixture.stage, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(fixture.prepare(fragment, "toml").is_err());
    assert!(fixture.stage_is_empty());
    assert_eq!(fixture.target_contents(), original);

    fs::set_permissions(&fixture.stage, fs::Permissions::from_mode(0o700)).unwrap();
    let link = fixture.root.path().join("link");
    std::os::unix::fs::symlink(&fixture.stage, &link).unwrap();
    assert!(
        prepare_in_private_stage(&fixture.target, fragment, "toml", &link, fixture.owner).is_err()
    );
    assert!(fixture.stage_is_empty());
    assert_eq!(fixture.target_contents(), original);

    let invalid = "private = [fixture secret\n";
    fixture.write_target(invalid);
    let error = fixture.prepare(fragment, "toml").unwrap_err();
    assert!(!format!("{error:#}").contains("fixture secret"));
    assert_eq!(fixture.target_contents(), invalid);
    assert!(fixture.stage_is_empty());
}
