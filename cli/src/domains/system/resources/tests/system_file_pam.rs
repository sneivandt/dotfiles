use super::*;

#[cfg(unix)]
#[test]
fn privileged_assessment_refuses_missing_pam_and_nonregular_targets_without_staging_contents() {
    let fixture = PrivateStage::new("target");
    assert!(
        fixture
            .prepare("auth optional fixture.so\n", "pam")
            .is_err()
    );
    assert!(!fixture.target.exists());
    assert!(fixture.stage_is_empty());

    let unmanaged = fixture.root.path().join("unmanaged");
    fs::write(&unmanaged, "private = 'fixture secret'\n").unwrap();
    std::os::unix::fs::symlink(&unmanaged, &fixture.target).unwrap();
    assert!(fixture.prepare("managed = true\n", "toml").is_err());
    assert!(fs::symlink_metadata(&fixture.target).unwrap().is_symlink());
    assert_eq!(
        fs::read_to_string(&unmanaged).unwrap(),
        "private = 'fixture secret'\n"
    );
    assert!(fixture.stage_is_empty());
}

#[test]
fn pam_conflicts_fail_validation_and_merge_without_silently_dropping_rules() {
    let original = "auth required pam_unix.so\n";
    for facility in ["auth", "AUTH", "-auth"] {
        let fragment = format!(
            "auth optional pam_example.so first\n{facility} required pam_example.so second\n"
        );
        assert!(
            validate_fragment(MergeStrategy::Pam, &fragment)
                .unwrap_err()
                .to_string()
                .contains("conflicting rules")
        );
        assert!(merge_pam(original, &fragment).is_err());
    }
    let rule = "auth optional pam_example.so first\n";
    let duplicate = format!("{rule}{rule}");
    validate_fragment(MergeStrategy::Pam, &duplicate).unwrap();
    let merged = merge_pam(original, &duplicate).unwrap();
    assert!(merged.contains(original));
    assert_eq!(merged.matches("pam_example.so").count(), 1);
    assert_eq!(merge_pam(&merged, &duplicate).unwrap(), merged);
}

#[test]
fn pam_refuses_conflicting_module_paths_in_the_same_facility() {
    let installed = "auth required /usr/lib/security/pam_gnome_keyring.so\n";
    let desired = "auth optional pam_gnome_keyring.so\n";
    let error = merge_pam(installed, desired).unwrap_err().to_string();
    assert!(error.contains("ambiguous PAM auth module paths"), "{error}");
    assert!(
        error.contains("/usr/lib/security/pam_gnome_keyring.so"),
        "{error}"
    );
    assert!(
        merge_pam(
            "session required /usr/lib/security/pam_gnome_keyring.so\nauth include system-auth\n",
            desired
        )
        .is_ok()
    );
}

#[test]
fn missing_pam_target_cannot_be_created_by_direct_apply() {
    for fragment in ["", "# Managed login rules\n", "auth optional keyring.so\n"] {
        let root = tempfile::tempdir_in(".").unwrap();
        fs::create_dir(root.path().join("system")).unwrap();
        fs::write(root.path().join("system/login"), fragment).unwrap();
        let target = root.path().join("missing-login");
        let resource = fixture_resource(
            entry(root.path(), &target, "login", MergeStrategy::Pam),
            Arc::new(MockExecutor::new()),
        );

        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Invalid { reason } if reason.contains("is missing")
        ));
        let error = resource.apply().unwrap_err();

        assert!(error.to_string().contains("is missing"), "{error}");
        assert!(!target.exists(), "{fragment:?}");
        assert!(!root.path().join("staging-must-not-be-created").exists());
    }
}

#[test]
fn pam_fragment_comments_do_not_become_rules_or_target_content() {
    let current = "# Preserve this comment\nauth include system-auth\n";
    let fragment = "# Managed rules\n\n  # Ignore indented comments\n\
            auth optional keyring.so\n";
    let merged = merge_pam(current, fragment).unwrap();

    assert_eq!(merged, format!("{current}auth optional keyring.so\n"));
    assert_eq!(merge_pam(&merged, fragment).unwrap(), merged);
    assert_eq!(merge_pam(current, "# No rules\n").unwrap(), current);
}

#[test]
fn pam_merge_preserves_a_stack_containing_only_the_managed_module() {
    let fragment = "auth optional keyring.so\n";
    for current in [
        fragment,
        "# retained\nauth required keyring.so old_option\n",
    ] {
        let merged = merge_pam(current, fragment).unwrap();
        assert!(merged.ends_with(fragment), "{merged}");
        assert_eq!(merged.matches("keyring.so").count(), 1, "{merged}");
        assert_eq!(merge_pam(&merged, fragment).unwrap(), merged);
        if current.starts_with('#') {
            assert!(merged.starts_with("# retained\n"));
        }
    }
    assert!(
        merge_pam("session optional other.so\n", fragment).is_err(),
        "a genuinely absent facility must still be rejected"
    );
}

#[test]
fn pam_merge_preserves_every_invocation_when_one_rule_matches() {
    let fragment =
        "auth sufficient pam_listfile.so item=user sense=allow onerr=fail file=/etc/allow-a\n";
    let other =
        "auth sufficient pam_listfile.so item=user sense=allow onerr=fail file=/etc/allow-b\n";
    let fallback = "auth required pam_deny.so\n";
    for current in [
        format!("{fragment}{other}{fallback}"),
        format!("{other}{fragment}{fallback}"),
        format!("{fragment}{fragment}{fallback}"),
        format!("auth [success=1 default=ignore] pam_permit.so\n{other}{fragment}{fallback}"),
        format!("auth include system-auth\n{other}{fragment}{fallback}"),
    ] {
        assert_eq!(merge_pam(&current, fragment).unwrap(), current);
    }
    let current = format!("{other}{fragment}{fallback}");
    let changed = fragment.replace("allow-a", "allow-c");
    let error = merge_pam(&current, &changed).unwrap_err().to_string();
    assert!(error.contains("none matches the fragment"), "{error}");
}

#[cfg(unix)]
#[test]
fn pam_retry_preserves_equivalent_site_layout_and_adds_only_missing_session_rule() {
    const LOGIN: &str = include_str!("../../../../../../system/pam.d/login");
    let fixture = PrivateStage::new("login");
    let legacy = "auth\toptional\tpam_gnome_keyring.so # local tty policy\n";
    let site = "auth optional pam_gnome_keyring.so only_if=gdm # graphical login\n\
                    session include system-login\n";
    let ambiguous = format!("auth optional pam_gnome_keyring.so only_if=login\n{site}");
    fixture.write_target(&ambiguous);
    let prepare = || fixture.prepare(LOGIN, "pam");
    assert!(prepare().is_err());
    assert_eq!(fixture.target_contents(), ambiguous);
    assert!(fixture.stage_is_empty());

    let reviewed = format!("{legacy}{site}");
    fixture.write_target(&reviewed);
    assert_eq!(prepare().unwrap(), "prepared");
    let session = LOGIN
        .lines()
        .find(|line| line.starts_with("session"))
        .unwrap();
    let expected = format!("{reviewed}{session}\n");
    assert_eq!(fixture.staged_contents("input"), expected);
    assert_eq!(fixture.staged_contents("original"), reviewed);
    fs::rename(fixture.stage.join("input"), &fixture.target).unwrap();
    fs::remove_file(fixture.stage.join("original")).unwrap();
    assert_eq!(prepare().unwrap(), "already-correct");
    assert!(fixture.stage_is_empty());
    assert_eq!(fixture.target_contents(), expected);

    let argument = "auth optional site.so [config=/etc/kiosk  site]";
    assert!(!same_pam_rule(
        argument,
        &argument.replace("kiosk  site", "kiosk site")
    ));
    assert_eq!(
        merge_pam(legacy, LOGIN.lines().next().unwrap()).unwrap(),
        legacy
    );
}

#[test]
fn pam_merge_preserves_the_rule_a_numeric_jump_skips() {
    let before = "# retain layout and comments\n\
            auth [success=1 default=die] pam_permit.so\n\
            auth optional pam_gnome_keyring.so # existing rule\n\
            auth sufficient pam_permit.so\n\
            auth requisite pam_deny.so\n";
    let fragment = "auth optional pam_gnome_keyring.so\n";
    assert_eq!(merge_pam(before, fragment).unwrap(), before);

    let updated = "auth optional pam_gnome_keyring.so debug\n";
    let expected = before.replace(
        "auth optional pam_gnome_keyring.so # existing rule\n",
        updated,
    );
    assert_eq!(merge_pam(before, updated).unwrap(), expected);
    assert_eq!(merge_pam(&expected, updated).unwrap(), expected);
}

#[test]
fn pam_merge_refuses_to_change_stack_length_around_jumps() {
    let fragment = "auth optional keyring.so\n";
    let jump = "-AUTH [success = \\\n    1 default=die] pam_permit.so\n";
    let error = merge_pam(jump, fragment).unwrap_err().to_string();
    assert!(error.contains("could be affected"), "{error}");

    let other_facility = format!("{jump}session required pam_permit.so\n");
    let session = "session optional keyring.so\n";
    assert_eq!(
        merge_pam(&other_facility, session).unwrap(),
        format!("{other_facility}{session}")
    );
}

#[test]
fn pam_merge_replaces_a_continued_rule_as_one_entry() {
    let current = "auth optional keyring.so \\\n    old_option\n\
            auth required pam_permit.so\n";
    let fragment = "auth optional keyring.so new_option\n";
    assert_eq!(
        merge_pam(current, fragment).unwrap(),
        format!("{fragment}auth required pam_permit.so\n")
    );
    assert!(
        validate_fragment(MergeStrategy::Pam, "auth optional keyring.so \\\n").is_err(),
        "a fragment must not continue into the next installed rule"
    );
}

#[cfg(unix)]
#[test]
fn unsafe_pam_stack_edit_does_not_stage_any_files() {
    let fragment = "auth optional keyring.so\n";
    for (installed, expected) in [
        (
            "auth [success=1 default=die] pam_permit.so\nauth required pam_permit.so\n",
            None,
        ),
        (
            "auth optional keyring.so first\nauth optional keyring.so second\n",
            None,
        ),
        (
            "auth optional keyring.so first\nauth optional keyring.so\n",
            Some("already-correct"),
        ),
    ] {
        let fixture = PrivateStage::new("login");
        fixture.write_target(installed);
        let result = fixture.prepare(fragment, "pam");
        assert_eq!(result.ok(), expected, "{installed}");
        assert_eq!(fixture.target_contents(), installed);
        assert!(fixture.stage_is_empty());
    }
}

#[test]
fn pam_merge_does_not_claim_module_arguments_or_stack_names() {
    let unmanaged = "auth include system-auth\n\
            auth required pam_exec.so keyring.so\n\
            auth [success=1 default=ignore] pam_debug.so keyring.so\n\
            -AUTH INCLUDE keyring.so\n\
            AUTH SUBSTACK keyring.so\n\
            auth include keyring.so\n";
    let current =
        format!("{unmanaged}auth optional keyring.so old_option\nsession include system-login\n");
    let fragment = "auth optional keyring.so\n";
    let expected = format!("{unmanaged}{fragment}session include system-login\n");

    let merged = merge_pam(&current, fragment).unwrap();

    assert_eq!(merged, expected, "unmanaged PAM rules must be preserved");
    assert_eq!(merge_pam(&merged, fragment).unwrap(), merged);
}

#[test]
fn pam_merge_recognizes_prefixed_and_case_insensitive_facilities() {
    for (current, fragment, expected) in [
        (
            "-auth include system-auth\nsession include system-login\n",
            "auth optional keyring.so\n",
            "-auth include system-auth\nauth optional keyring.so\nsession include system-login\n",
        ),
        (
            "auth include system-auth\n-auth optional keyring.so old_option\n",
            "auth optional keyring.so\n",
            "auth include system-auth\nauth optional keyring.so\n",
        ),
        (
            "session include system-login\nSESSION optional keyring.so old_option\n",
            "-session optional keyring.so\n",
            "session include system-login\n-session optional keyring.so\n",
        ),
        (
            "-session optional keyring.so\n",
            "SESSION optional keyring.so\n",
            "SESSION optional keyring.so\n",
        ),
    ] {
        let merged = merge_pam(current, fragment).unwrap();
        assert_eq!(merged, expected, "{current:?} with {fragment:?}");
        assert_eq!(merge_pam(&merged, fragment).unwrap(), merged);
    }
}

#[test]
fn pam_rule_identity_uses_the_module_field_after_the_control_field() {
    for rule in [
        "auth optional keyring.so argument",
        "auth [success=1 default=ignore] keyring.so argument",
        "auth [success=done] keyring.so argument",
    ] {
        assert_eq!(pam_rule_identity(rule).unwrap(), ("auth", "keyring.so"));
    }

    for rule in [
        "auth optional script keyring.so",
        "auth include keyring.so",
        "auth substack keyring.so",
        "-AUTH INCLUDE keyring.so",
        "AUTH SUBSTACK keyring.so",
        "auth [success=1 keyring.so",
    ] {
        assert!(
            pam_rule_identity(rule).is_err(),
            "a stack name or argument is not a module field: {rule}"
        );
    }
}

#[cfg(unix)]
#[test]
fn pam_resource_accepts_an_already_converged_single_module_stack() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = tempfile::tempdir_in(".").unwrap();
    fs::create_dir(root.path().join("system")).unwrap();
    let content = "password optional keyring.so\n";
    fs::write(root.path().join("system/passwd"), content).unwrap();
    let target = root.path().join("passwd");
    fs::write(&target, content).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    let resource = fixture_resource(
        entry(root.path(), &target, "passwd", MergeStrategy::Pam),
        Arc::new(MockExecutor::new()),
    );

    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    assert_eq!(resource.apply().unwrap(), ResourceChange::AlreadyCorrect);
    assert_eq!(resource.apply().unwrap(), ResourceChange::AlreadyCorrect);
    assert_eq!(fs::read_to_string(target).unwrap(), content);
}
