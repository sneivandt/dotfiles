use super::*;

#[cfg(unix)]
#[test]
fn offline_lookup_preserves_override_precedence_and_masks() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    let user_dir = home.join(".config/systemd/user");
    let admin_dir = fixture.path().join("etc/systemd/user");
    let package_dir = fixture.path().join("usr/lib/systemd/user");
    for dir in [&user_dir, &admin_dir, &package_dir] {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("example.service"),
            "[Install]\nWantedBy=default.target\n",
        )
        .unwrap();
    }
    let mut resource = offline_unit(&home, "example.service", true);
    resource.system_user_unit_dirs = vec![admin_dir.clone(), package_dir.clone()];
    for expected in [&user_dir, &admin_dir, &package_dir] {
        assert_eq!(
            resource.offline_unit_path().unwrap(),
            Some(expected.join(&resource.name))
        );
        std::fs::remove_file(expected.join(&resource.name)).unwrap();
    }
    std::fs::write(
        package_dir.join(&resource.name),
        "[Install]\nWantedBy=default.target\n",
    )
    .unwrap();
    for target in [Path::new("/dev/null"), &fixture.path().join("missing")] {
        let mask = user_dir.join(&resource.name);
        std::os::unix::fs::symlink(target, &mask).unwrap();
        assert_eq!(resource.offline_unit_path().unwrap(), Some(mask.clone()));
        assert!(
            resource.apply().is_err(),
            "must not fall back past a masked or broken override"
        );
        assert!(!user_dir.join("default.target.wants").exists());
        std::fs::remove_file(mask).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn offline_enablement_repairs_checkout_links_and_accepts_relative_installed_links() {
    let home = tempfile::tempdir().unwrap();
    let source = home.path().join("checkout/example.service");
    let unit_dir = home.path().join(".config/systemd/user");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::create_dir_all(unit_dir.join("default.target.wants")).unwrap();
    std::fs::write(&source, "[Install]\nWantedBy=default.target\n").unwrap();
    let installed = unit_dir.join("example.service");
    std::os::unix::fs::symlink(&source, &installed).unwrap();
    let link = unit_dir.join("default.target.wants/example.service");
    std::os::unix::fs::symlink(&source, &link).unwrap();
    let resource = offline_unit(home.path(), "example.service", true);
    assert!(matches!(
        resource.current_state().unwrap(),
        ResourceState::Incorrect { .. }
    ));
    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    assert_eq!(std::fs::read_link(&link).unwrap(), installed);
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink("../example.service", &link).unwrap();
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
}

#[cfg(unix)]
#[test]
fn offline_user_units_converge_without_contacting_the_bus() {
    for (name, definition, relationship) in [
        (
            "clean-home-tmp.timer",
            "[Unit]\nDescription=Clean tmp\n[Install]\nWantedBy=timers.target\n",
            "timers.target.wants",
        ),
        (
            "session.service",
            "[Install]\nWantedBy=graphical-session.target\n",
            "graphical-session.target.wants",
        ),
        (
            "session.service",
            "[Install]\nRequiredBy=graphical-session.target\n",
            "graphical-session.target.requires",
        ),
    ] {
        let home = tempfile::tempdir().unwrap();
        let user = home.path().join(".config/systemd/user");
        std::fs::create_dir_all(&user).unwrap();
        let source = user.join(name);
        std::fs::write(&source, definition).unwrap();
        let link = user.join(relationship).join(name);

        let enabled = offline_unit(home.path(), name, true);
        assert_eq!(enabled.current_state().unwrap(), ResourceState::Missing);
        assert_eq!(enabled.apply().unwrap(), ResourceChange::Applied);
        assert_eq!(std::fs::read_link(&link).unwrap(), source, "{relationship}");
        assert_eq!(enabled.current_state().unwrap(), ResourceState::Correct);

        let disabled = offline_unit(home.path(), name, false);
        assert!(matches!(
            disabled.current_state().unwrap(),
            ResourceState::Incorrect { .. }
        ));
        assert_eq!(disabled.apply().unwrap(), ResourceChange::Applied);
        assert!(!link.is_symlink(), "{relationship}");
        assert_eq!(disabled.current_state().unwrap(), ResourceState::Correct);
    }
}

#[cfg(unix)]
#[test]
fn offline_user_unit_missing_definition_is_plannable_for_dry_run() {
    let home = tempfile::tempdir().unwrap();
    let resource = offline_unit(home.path(), "not-linked-yet.timer", true);

    assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
}

#[cfg(unix)]
#[test]
fn disable_reports_global_enablement_and_preserves_the_admin_link_online_and_offline() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    let admin = fixture.path().join("etc/systemd/user");
    let wants = admin.join("default.target.wants");
    std::fs::create_dir_all(&wants).unwrap();
    let name = "example.service";
    let source = admin.join(name);
    let link = wants.join(name);
    std::fs::write(&source, "[Install]\nWantedBy=default.target\n").unwrap();
    std::os::unix::fs::symlink("../example.service", &link).unwrap();
    let mut resource = offline_unit(&home, name, false);
    resource.system_user_unit_dirs = vec![admin];
    for live in [false, true] {
        resource.manager_available = live;
        if live {
            resource.executor = Arc::new(ScriptedExecutor::new().command(
                systemctl(&["--user", "is-enabled", name]),
                Ok(ExecResult::success("enabled")),
            ));
        }
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Invalid { reason } if reason.contains("ask an administrator")
        ));
        assert_eq!(
            resource.apply().unwrap(),
            ResourceChange::unusable(format!(
                "{name} is enabled for all users by {}; ask an administrator to disable it",
                link.display()
            ))
        );
    }
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        Path::new("../example.service")
    );
    assert!(!home.exists());
}

#[cfg(unix)]
#[test]
fn temporary_mask_cannot_confirm_persistent_disable_until_it_expires() {
    use crate::engine::{ProcessOpts, process_resources};
    use crate::test_helpers::{ContextBuilder, task_batch};

    let fixture = tempfile::tempdir_in(".").unwrap();
    let home = fixture.path().join("home");
    let admin = fixture.path().join("etc/systemd/user");
    let runtime = fixture.path().join("run/user/1000/systemd/user");
    let name = "fixture.service";
    let wants = admin.join("default.target.wants");
    std::fs::create_dir_all(&wants).unwrap();
    std::fs::create_dir_all(&runtime).unwrap();
    let mask = runtime.join(name);
    let link = wants.join(name);
    std::fs::write(admin.join(name), "[Install]\nWantedBy=default.target\n").unwrap();
    std::os::unix::fs::symlink("../fixture.service", &link).unwrap();
    std::os::unix::fs::symlink("/dev/null", &mask).unwrap();
    let resource = || {
        let (enablement, active) = if mask.is_symlink() {
            (ExecResult::failure("masked-runtime\n", "", Some(1)), None)
        } else if link.is_symlink() {
            (ExecResult::success("enabled\n"), None)
        } else {
            (
                ExecResult::failure("disabled\n", "", Some(1)),
                Some("ActiveState=inactive\n"),
            )
        };
        let mut unit = queried_unit(name, UnitScope::User, false, enablement, active);
        unit.home = Some(home.clone());
        unit.system_user_unit_dirs = vec![admin.clone()];
        unit
    };

    let ctx = ContextBuilder::new(home.clone()).overlay(None).build();
    for dry_run in [true, false] {
        let result = process_resources(
            &ctx.with_dry_run(dry_run),
            [resource()],
            &ProcessOpts::lenient("configure"),
        )
        .unwrap();
        assert_eq!(task_batch(&result).failed_count(), 1);
        assert_eq!(std::fs::read_link(&mask).unwrap(), Path::new("/dev/null"));
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            Path::new("../fixture.service")
        );
    }
    // The mask expires at logout/boot; the admin's persistent link remains.
    std::fs::remove_file(mask.as_path()).unwrap();
    assert!(
        matches!(resource().current_state().unwrap(), ResourceState::Invalid { reason } if reason.contains("enabled for all users"))
    );
    std::fs::remove_file(link.as_path()).unwrap();
    assert_eq!(resource().current_state().unwrap(), ResourceState::Correct);
}

#[cfg(unix)]
#[test]
fn offline_disable_removes_stale_links_without_requiring_install_metadata() {
    use crate::engine::{ProcessOpts, TaskResult, process_resources};
    use crate::test_helpers::ContextBuilder;

    for (label, definition) in [
        ("changed targets", Some("[Install]\nWantedBy=new.target\n")),
        ("static unit", Some("[Service]\nExecStart=/usr/bin/true\n")),
        ("missing definition", None),
    ] {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let home = fixture.path().canonicalize().unwrap();
        let unit_dir = home.join(".config/systemd/user");
        let wants = unit_dir.join("old.target.wants");
        let requires = unit_dir.join("old.target.requires");
        std::fs::create_dir_all(&wants).unwrap();
        std::fs::create_dir_all(&requires).unwrap();
        let unit_path = unit_dir.join("example.service");
        if let Some(definition) = definition {
            std::fs::write(&unit_path, definition).unwrap();
        }
        let wanted = wants.join("example.service");
        let dependency = requires.join("example.service");
        std::os::unix::fs::symlink("../example.service", &wanted).unwrap();
        std::os::unix::fs::symlink(&unit_path, &dependency).unwrap();
        let unrelated_dir = unit_dir.join("unrelated.target.wants");
        std::fs::create_dir(&unrelated_dir).unwrap();
        let unrelated = unrelated_dir.join("example.service");
        std::os::unix::fs::symlink("../other.service", &unrelated).unwrap();
        let resource = || {
            let mut resource = offline_unit(&home, "example.service", false);
            resource.system_user_unit_dirs.clear();
            resource
        };
        assert!(
            matches!(
                resource().current_state().unwrap(),
                ResourceState::Incorrect { .. }
            ),
            "{label}"
        );
        let ctx = ContextBuilder::new(home.clone())
            .overlay(None)
            .build()
            .with_dry_run(true);
        let result =
            process_resources(&ctx, [resource()], &ProcessOpts::strict("configure")).unwrap();
        assert!(
            matches!(result, TaskResult::Batch(stats) if stats.changed_count() == 1),
            "{label}: dry-run must plan the stale-link removal"
        );
        assert!(wanted.is_symlink() && dependency.is_symlink(), "{label}");

        assert_eq!(
            resource().apply().unwrap(),
            ResourceChange::Applied,
            "{label}"
        );
        assert!(!wanted.is_symlink() && !dependency.is_symlink(), "{label}");
        assert!(
            unrelated.is_symlink(),
            "{label}: preserve user-replaced links"
        );
        assert_eq!(resource().current_state().unwrap(), ResourceState::Correct);
        assert_eq!(resource().apply().unwrap(), ResourceChange::AlreadyCorrect);
    }
}

#[cfg(unix)]
#[test]
fn offline_disable_refuses_to_traverse_replaced_enablement_directories() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let home = fixture.path().canonicalize().unwrap();
    let unit_dir = home.join(".config/systemd/user");
    let external = home.join("external");
    std::fs::create_dir_all(&unit_dir).unwrap();
    std::fs::create_dir(&external).unwrap();
    let link = external.join("example.service");
    std::os::unix::fs::symlink(unit_dir.join("example.service"), &link).unwrap();
    std::os::unix::fs::symlink(&external, unit_dir.join("old.target.wants")).unwrap();
    let resource = offline_unit(&home, "example.service", false);

    assert!(resource.current_state().is_err());
    assert!(resource.apply().is_err());
    assert!(
        link.is_symlink(),
        "unmanaged directory contents must survive"
    );
}

#[test]
fn unsafe_names_cannot_probe_or_mutate_even_when_resources_are_constructed_directly() {
    for name in [
        "../../audit.service",
        "/audit.service",
        "--root.service",
        r"a\b.service",
    ] {
        let resource =
            SystemdUnitResource::new(name, UnitScope::User, Arc::new(MockExecutor::new()));
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Invalid { .. }
        ));
        assert!(resource.apply().is_err());
    }
}

#[cfg(unix)]
#[test]
fn offline_discovery_preview_and_apply_reject_redirected_ancestors() {
    for ancestor in [
        ".config",
        ".config/systemd",
        ".config/systemd/user",
        ".config/systemd/user/default.target.wants",
    ] {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let packaged = tempfile::tempdir().unwrap();
        std::fs::write(
            packaged.path().join("audit.service"),
            "[Install]\nWantedBy=default.target\n",
        )
        .unwrap();
        let link = home.path().join(ancestor);
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        let mut resource = offline_unit(home.path(), "audit.service", true);
        resource.system_user_unit_dirs = vec![packaged.path().into()];
        assert!(resource.current_state().is_err(), "{ancestor}");
        assert!(resource.apply().is_err(), "{ancestor}");
        let ctx =
            crate::test_helpers::make_linux_context(home.path().into(), None).with_dry_run(true);
        let outcome = crate::engine::process_resources(
            &ctx,
            [resource],
            &crate::engine::ProcessOpts::strict("configure"),
        );
        assert!(outcome.is_err(), "unsafe preview must fail: {ancestor}");
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        assert!(link.is_symlink());
    }
}

#[cfg(unix)]
#[test]
fn offline_rechecks_redirected_ancestors_and_preserves_foreign_links_before_apply() {
    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let user = home.path().join(".config/systemd/user");
    std::fs::create_dir_all(&user).unwrap();
    std::fs::write(
        user.join("audit.service"),
        "[Install]\nWantedBy=default.target\n",
    )
    .unwrap();
    let resource = offline_unit(home.path(), "audit.service", true);
    resource.current_state().unwrap();
    let wants = user.join("default.target.wants");
    std::os::unix::fs::symlink(outside.path(), &wants).unwrap();
    assert!(resource.apply().is_err());
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    std::fs::remove_file(&wants).unwrap();
    std::fs::create_dir(&wants).unwrap();
    let target = wants.join("audit.service");
    std::os::unix::fs::symlink(outside.path().join("foreign"), &target).unwrap();
    assert!(resource.apply().is_err());
    assert_eq!(
        std::fs::read_link(target).unwrap(),
        outside.path().join("foreign")
    );
}

#[cfg(unix)]
#[test]
fn offline_partial_enablement_preserves_correct_relative_links() {
    let home = tempfile::tempdir().unwrap();
    let user = home.path().join(".config/systemd/user");
    let wants = user.join("default.target.wants");
    std::fs::create_dir_all(&wants).unwrap();
    std::fs::write(
        user.join("audit.service"),
        "[Install]\nWantedBy=default.target other.target\n",
    )
    .unwrap();
    let link = wants.join("audit.service");
    std::os::unix::fs::symlink("../audit.service", &link).unwrap();
    let resource = offline_unit(home.path(), "audit.service", true);
    assert!(matches!(
        resource.current_state().unwrap(),
        ResourceState::Incorrect { .. }
    ));
    resource.apply().unwrap();
    assert_eq!(
        std::fs::read_link(link).unwrap(),
        PathBuf::from("../audit.service")
    );
    assert!(user.join("other.target.wants/audit.service").is_symlink());
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
}

#[cfg(unix)]
#[test]
fn offline_unsupported_install_semantics_fail_before_creating_links() {
    for directive in [
        "Alias=other.service",
        "Also=other.service",
        "DefaultInstance=one",
        "UpheldBy=other.target",
        "WantedBy=%i.target",
        "WantedBy=\"other.target\"",
    ] {
        let home = tempfile::tempdir().unwrap();
        let user = home.path().join(".config/systemd/user");
        std::fs::create_dir_all(&user).unwrap();
        std::fs::write(
            user.join("audit.service"),
            format!("[Install]\nWantedBy=default.target\n{directive}\n"),
        )
        .unwrap();
        let resource = offline_unit(home.path(), "audit.service", true);
        assert!(resource.current_state().is_err(), "{directive}");
        assert!(resource.apply().is_err(), "{directive}");
        assert_eq!(std::fs::read_dir(&user).unwrap().count(), 1);
    }
    for directory in ["service.d", "audit-.service.d"] {
        let home = tempfile::tempdir().unwrap();
        let user = home.path().join(".config/systemd/user");
        std::fs::create_dir_all(user.join(directory)).unwrap();
        std::fs::write(
            user.join("audit-example.service"),
            "[Install]\nWantedBy=default.target\n",
        )
        .unwrap();
        std::fs::write(
            user.join(directory).join("override.conf"),
            "[Install]\nWantedBy=other.target\n",
        )
        .unwrap();
        let resource = offline_unit(home.path(), "audit-example.service", true);
        assert!(
            format!("{:#}", resource.apply().unwrap_err()).contains("live user systemd manager")
        );
        assert!(!user.join("default.target.wants").exists());
    }
    let home = tempfile::tempdir().unwrap();
    let resource = offline_unit(home.path(), &format!("audit{}one.service", '@'), true);
    assert!(resource.apply().is_err());
}

#[cfg(unix)]
#[test]
fn offline_enablement_propagates_non_missing_link_errors() {
    let home = tempfile::tempdir().unwrap();
    let user = home.path().join(".config/systemd/user");
    std::fs::create_dir_all(&user).unwrap();
    let unit = user.join("audit.service");
    std::fs::write(&unit, "[Install]\nWantedBy=default.target\n").unwrap();
    // A regular file as the parent of a candidate link makes symlink_metadata(link)
    // return ENOTDIR rather than NotFound, without triggering offline_obsolete_links.
    let not_a_dir = user.join("not-a-dir");
    std::fs::write(&not_a_dir, "not a directory").unwrap();
    let resource = offline_unit(home.path(), "audit.service", true);
    assert!(
        resource
            .enablement_link_state(&[(not_a_dir.join("audit.service"), unit)], false)
            .is_err(),
        "ENOTDIR on enablement link lookup must fail rather than report Missing"
    );
}
