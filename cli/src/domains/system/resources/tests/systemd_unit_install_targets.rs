use super::*;

#[cfg(unix)]
#[test]
fn a_mask_with_install_dropins_preserves_links_until_unmasked() {
    for dev_null in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().join("home");
        let name = "example.service";
        let mut resource = offline_unit(&home, name, true);
        let user_dir = home.join(".config/systemd/user");
        let vendor = fixture.path().join("usr/lib/systemd/user");
        resource.system_user_unit_dirs = vec![vendor.clone()];
        let dropin = user_dir.join(format!("{name}.d"));
        let old_target = user_dir.join("default.target.wants");
        for dir in [&vendor, &dropin, &old_target] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let source = vendor.join(name);
        let installed = user_dir.join(name);
        let old = old_target.join(name);
        let desired = user_dir
            .join("graphical-session.target.requires")
            .join(name);
        std::fs::write(&source, "[Service]\nExecStart=/bin/true\n").unwrap();
        std::fs::write(
            dropin.join("install.conf"),
            "[Install]\nRequiredBy=graphical-session.target\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(&source, &old).unwrap();
        if dev_null {
            std::os::unix::fs::symlink("/dev/null", &installed).unwrap();
        } else {
            std::fs::write(&installed, "").unwrap();
        }

        assert!(matches!(
            resource.current_state(),
            Err(ResourceError::ConflictingState { .. })
        ));
        assert!(matches!(
            resource.apply(),
            Err(ResourceError::ConflictingState { .. })
        ));
        assert_eq!(std::fs::read_link(&old).unwrap(), source);
        assert!(!desired.is_symlink());
        if dev_null {
            assert_eq!(
                std::fs::read_link(&installed).unwrap(),
                Path::new("/dev/null")
            );
        } else {
            assert!(std::fs::read(&installed).unwrap().is_empty());
        }

        std::fs::remove_file(&installed).unwrap();
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        assert_eq!(std::fs::read_link(&desired).unwrap(), source);
        assert!(!old.is_symlink());
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }
}

#[test]
fn offline_install_targets_join_continued_directives() {
    for (content, expected) in [
        (
            "[Install]\nWantedBy=default.target \\\n  graphical-session.target\n\
             RequiredBy=basic.target \\\n# ignored comment\n; another comment\n  sockets.target\n",
            vec![
                "default.target.wants",
                "graphical-session.target.wants",
                "basic.target.requires",
                "sockets.target.requires",
            ],
        ),
        (
            "[Install]\nWantedBy=old.target\nWantedBy=\\\n  new.target\n",
            vec!["old.target.wants", "new.target.wants"],
        ),
        (
            "[Unit]\nDescription=continued \\\n  WantedBy=ignored.target\n\
             [Install]\nWantedBy=default.target \\\n",
            vec!["default.target.wants"],
        ),
        (
            "[Unit]\nDescription=literal backslash \\\\\n\
             [Install]\nWantedBy=default.target\n",
            vec!["default.target.wants"],
        ),
    ] {
        assert_eq!(install_targets(content).unwrap(), expected, "{content}");
    }
}

#[cfg(unix)]
#[test]
fn offline_enablement_converges_all_continued_install_targets() {
    use crate::engine::{ProcessOpts, TaskResult, process_resources};
    use crate::test_helpers::ContextBuilder;

    let fixture = tempfile::tempdir_in(".").unwrap();
    let home = fixture.path().canonicalize().unwrap();
    let unit_dir = home.join(".config/systemd/user");
    std::fs::create_dir_all(&unit_dir).unwrap();
    std::fs::write(
        unit_dir.join("example.service"),
        "[Install]\nWantedBy=default.target \\\n# continued\n  graphical-session.target\n\
         RequiredBy=basic.target \\\n  sockets.target\n",
    )
    .unwrap();
    let resource = || offline_unit(&home, "example.service", true);
    let ctx = ContextBuilder::new(home.clone()).overlay(None).build();
    let opts = ProcessOpts::strict("configure");

    let preview = process_resources(&ctx.with_dry_run(true), [resource()], &opts).unwrap();
    assert!(matches!(preview, TaskResult::Batch(stats) if stats.changed_count() == 1));
    assert_eq!(std::fs::read_dir(&unit_dir).unwrap().count(), 1);

    let applied = process_resources(&ctx, [resource()], &opts).unwrap();
    assert!(matches!(applied, TaskResult::Batch(stats) if stats.changed_count() == 1));
    for target in [
        "default.target.wants",
        "graphical-session.target.wants",
        "basic.target.requires",
        "sockets.target.requires",
    ] {
        assert_eq!(
            std::fs::read_link(unit_dir.join(target).join("example.service")).unwrap(),
            unit_dir.join("example.service"),
        );
    }
    let repeated = process_resources(&ctx, [resource()], &opts).unwrap();
    assert!(matches!(repeated, TaskResult::Batch(stats) if stats.already_ok_count() == 1));
}

#[test]
fn offline_install_targets_ignore_standalone_continued_comments() {
    for comment in [
        "# ignored \\",
        "; ignored \\",
        " \t# ignored \\",
        " \t; ignored \\",
    ] {
        let content = format!("[Unit]\n{comment}\n[Install]\n{comment}\nWantedBy=default.target\n");
        assert_eq!(
            install_targets(&content).unwrap(),
            ["default.target.wants"],
            "{comment:?}"
        );
    }
}

#[test]
fn offline_install_targets_blank_lines_end_continuations() {
    for blank in ["", "  \t"] {
        for (prefix, expected) in [
            (
                "WantedBy=first.target \\\n",
                vec!["first.target.wants", "second.target.wants"],
            ),
            (
                "WantedBy=old.target\nWantedBy=\\\n",
                vec!["second.target.wants"],
            ),
        ] {
            let content =
                format!("[Install]\n{prefix}# skipped comment\n{blank}\nWantedBy=second.target\n");
            assert_eq!(install_targets(&content).unwrap(), expected, "{content:?}");
        }
    }
}

#[test]
fn offline_install_targets_continue_only_after_an_unescaped_final_backslash() {
    for count in 0..=5 {
        let content = format!(
            "[Unit]\nDescription=trailing {}\n[Install]\nWantedBy=default.target\n",
            "\\".repeat(count),
        );
        let expected = if count % 2 == 0 {
            vec!["default.target.wants"]
        } else {
            vec![]
        };
        assert_eq!(
            install_targets(&content).unwrap(),
            expected,
            "{count} backslashes"
        );
    }
    let content = "[Unit]\nDescription=backslash followed by whitespace \\ \t\n\
                   [Install]\nWantedBy=default.target\n";
    assert_eq!(install_targets(content).unwrap(), ["default.target.wants"]);
}

#[test]
fn offline_install_targets_reset_each_relationship_list_independently() {
    for (content, expected) in [
        (
            "[Install]\nWantedBy=old.target\nRequiredBy=keep.target\n\
             WantedBy= \nWantedBy=new.target\n",
            vec!["keep.target.requires", "new.target.wants"],
        ),
        (
            "[Install]\nRequiredBy=old.target\nWantedBy=keep.target\n\
             RequiredBy=\nRequiredBy=new.target\n",
            vec!["keep.target.wants", "new.target.requires"],
        ),
        (
            "[Install]\nWantedBy=old.target\nRequiredBy=old.target\n\
             WantedBy=\nRequiredBy=\n",
            vec![],
        ),
    ] {
        assert_eq!(install_targets(content).unwrap(), expected, "{content}");
    }
}

#[cfg(unix)]
#[test]
fn offline_enablement_reconciles_reset_install_targets() {
    use crate::engine::{ProcessOpts, TaskResult, process_resources};
    use crate::test_helpers::ContextBuilder;

    let fixture = tempfile::tempdir_in(".").unwrap();
    let home = fixture.path().canonicalize().unwrap();
    let unit_dir = home.join(".config/systemd/user");
    std::fs::create_dir_all(&unit_dir).unwrap();
    let unit = unit_dir.join("example.service");
    std::fs::write(&unit, "[Install]\nWantedBy=old.target\n").unwrap();
    let resource = || offline_unit(&home, "example.service", true);
    resource().apply().unwrap();
    let obsolete = unit_dir.join("old.target.wants/example.service");
    let unrelated_dir = unit_dir.join("local.target.wants");
    std::fs::create_dir(&unrelated_dir).unwrap();
    let unrelated = unrelated_dir.join("example.service");
    std::os::unix::fs::symlink("../other.service", &unrelated).unwrap();
    std::fs::write(
        &unit,
        "[Install]\nWantedBy=old.target\nRequiredBy=keep.target\n\
         WantedBy=\nWantedBy=new.target\n",
    )
    .unwrap();
    let ctx = ContextBuilder::new(home.clone()).overlay(None).build();
    let preview = process_resources(
        &ctx.with_dry_run(true),
        [resource()],
        &ProcessOpts::strict("configure"),
    )
    .unwrap();
    assert!(matches!(preview, TaskResult::Batch(stats) if stats.changed_count() == 1));
    assert!(!unit_dir.join("new.target.wants").exists());
    assert!(!unit_dir.join("keep.target.requires").exists());
    assert!(obsolete.is_symlink());

    let keep_dir = unit_dir.join("keep.target.requires");
    std::fs::create_dir(&keep_dir).unwrap();
    let conflict = keep_dir.join("example.service");
    std::fs::write(&conflict, "local file").unwrap();
    assert!(resource().apply().is_err());
    assert!(obsolete.is_symlink());
    assert_eq!(std::fs::read_to_string(&conflict).unwrap(), "local file");
    std::fs::remove_file(&conflict).unwrap();

    assert_eq!(resource().apply().unwrap(), ResourceChange::Applied);
    assert!(!obsolete.is_symlink());
    assert!(
        unit_dir
            .join("new.target.wants/example.service")
            .is_symlink()
    );
    assert_eq!(
        std::fs::read_link(&unrelated).unwrap(),
        Path::new("../other.service")
    );
    // Stale links also count as drift when all desired links already exist.
    std::os::unix::fs::symlink("../example.service", &obsolete).unwrap();
    assert!(matches!(
        resource().current_state().unwrap(),
        ResourceState::Incorrect { .. }
    ));
    resource().apply().unwrap();
    assert!(!obsolete.is_symlink());
    assert!(
        unit_dir
            .join("keep.target.requires/example.service")
            .is_symlink()
    );
    let repeated =
        process_resources(&ctx, [resource()], &ProcessOpts::strict("configure")).unwrap();
    assert!(matches!(repeated, TaskResult::Batch(stats) if stats.already_ok_count() == 1));
}

#[cfg(unix)]
#[test]
fn live_user_enablement_reconciles_changed_targets_and_preserves_foreign_links() {
    use crate::engine::{ProcessOpts, process_resources};
    use crate::test_helpers::{ContextBuilder, task_batch};

    let fixture = tempfile::tempdir_in(".").unwrap();
    let home = fixture.path().canonicalize().unwrap();
    let user_dir = home.join(".config/systemd/user");
    let old = user_dir.join("old.target.wants");
    let foreign = user_dir.join("foreign.target.wants");
    for directory in [&old, &foreign] {
        std::fs::create_dir_all(directory).unwrap();
    }
    let name = "example.service";
    let unit = user_dir.join(name);
    std::fs::write(&unit, "[Install]\nWantedBy=new.target\n").unwrap();
    std::os::unix::fs::symlink(&unit, old.join(name)).unwrap();
    std::os::unix::fs::symlink("../another.service", foreign.join(name)).unwrap();
    let probe = || systemctl(&["--user", "is-enabled", name]);
    let scripted = ScriptedExecutor::new()
        .command(probe(), Ok(ExecResult::success("enabled")))
        .command(probe(), Ok(ExecResult::success("enabled")))
        .command(
            CommandSpec::new("systemctl").args(&["--user", "daemon-reload"]),
            Ok(ExecResult::success("")),
        )
        .command(probe(), Ok(ExecResult::success("enabled")));
    let executor: Arc<dyn Executor> = Arc::new(scripted);
    let resource = || {
        let mut resource = offline_unit(&home, name, true);
        resource.manager_available = true;
        resource.start = false;
        resource.executor = Arc::clone(&executor);
        resource.system_user_unit_dirs.clear();
        resource
    };
    let ctx = ContextBuilder::new(home.clone()).overlay(None).build();
    let preview = process_resources(
        &ctx.with_dry_run(true),
        [resource()],
        &ProcessOpts::strict("configure"),
    )
    .unwrap();
    assert_eq!(task_batch(&preview).changed_count(), 1);
    assert!(old.join(name).is_symlink());
    assert!(!user_dir.join("new.target.wants").exists());
    let resource = resource();
    assert!(matches!(
        resource.current_state().unwrap(),
        ResourceState::Incorrect { .. }
    ));
    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    assert!(!old.join(name).is_symlink());
    assert_eq!(
        std::fs::read_link(user_dir.join("new.target.wants").join(name)).unwrap(),
        unit
    );
    assert_eq!(
        std::fs::read_link(foreign.join(name)).unwrap(),
        Path::new("../another.service")
    );
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
}

#[cfg(unix)]
#[test]
fn offline_user_unit_rejects_install_targets_that_escape_the_user_directory() {
    let home = tempfile::tempdir().unwrap();
    let unit_dir = home.path().join(".config/systemd/user");
    std::fs::create_dir_all(&unit_dir).unwrap();
    std::fs::write(
        unit_dir.join("unsafe.service"),
        "[Install]\nWantedBy=../../outside.target\n",
    )
    .unwrap();
    let resource = offline_unit(home.path(), "unsafe.service", true);

    let error = resource
        .apply()
        .expect_err("escaping target must be rejected");
    assert!(error.to_string().contains("invalid [Install] target"));
    assert!(!home.path().join("outside.target.wants").exists());
}

#[cfg(unix)]
#[test]
fn offline_dropins_reset_relationships_follow_precedence_and_preserve_read_errors() {
    let home = tempfile::tempdir().unwrap();
    let package = tempfile::tempdir().unwrap();
    let user = home.path().join(".config/systemd/user");
    for directory in [&user, &package.path().to_path_buf()] {
        std::fs::create_dir_all(directory.join("audit.service.d")).unwrap();
    }
    std::fs::write(
        user.join("audit.service"),
        "[Install]\nWantedBy=default.target\nRequiredBy=old.target\n",
    )
    .unwrap();
    std::fs::write(
        package.path().join("audit.service.d/10-reset.conf"),
        "[Install]\nWantedBy=\nWantedBy=wrong.target\n",
    )
    .unwrap();
    std::fs::write(
        user.join("audit.service.d/10-reset.conf"),
        "[Install]\nWantedBy=\nWantedBy=other.target\nRequiredBy=\n",
    )
    .unwrap();
    std::fs::write(
        package.path().join("audit.service.d/20-extra.conf"),
        "[Install]\nRequiredBy=another.target\n",
    )
    .unwrap();
    std::fs::write(
        user.join("audit.service.d/30-without-section.conf"),
        "WantedBy=ignored.target\n",
    )
    .unwrap();
    std::fs::write(user.join("audit.service.d/README"), "ignored").unwrap();
    let mut resource = offline_unit(home.path(), "audit.service", true);
    resource.system_user_unit_dirs = vec![package.path().into()];
    assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    resource.apply().unwrap();
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    assert!(user.join("other.target.wants/audit.service").is_symlink());
    assert!(
        user.join("another.target.requires/audit.service")
            .is_symlink()
    );
    for name in [
        "default.target.wants",
        "wrong.target.wants",
        "old.target.requires",
        "ignored.target.wants",
    ] {
        assert!(!user.join(name).exists(), "{name}");
    }
    std::fs::create_dir(user.join("audit.service.d/40-invalid.conf")).unwrap();
    assert!(resource.current_state().is_err());
    assert!(resource.apply().is_err());
}

#[cfg(unix)]
#[test]
fn offline_dropin_directory_inspection_errors_block_discovery_and_apply() {
    let home = tempfile::tempdir().unwrap();
    let user = home.path().join(".config/systemd/user");
    std::fs::create_dir_all(&user).unwrap();
    std::fs::write(
        user.join("audit.service"),
        "[Install]\nWantedBy=default.target\n",
    )
    .unwrap();
    let dropin = user.join("audit.service.d");
    std::fs::write(&dropin, "not a directory").unwrap();
    let resource = offline_unit(home.path(), "audit.service", true);
    assert!(resource.current_state().is_err());
    assert!(resource.apply().is_err());
    assert_eq!(std::fs::read_to_string(dropin).unwrap(), "not a directory");
    assert!(!user.join("default.target.wants").exists());
}

#[test]
fn shared_dropin_detection_ignores_runtime_sections_comments_and_bare_text() {
    for (content, expected) in [
        ("[Install]\nWantedBy=default.target", true),
        ("[Install]\nWantedBy=", true),
        ("[Install]\nWantedBy=other.target]", true),
        ("[Install]\n[WantedBy=other.target", true),
        (
            "[Install]\n# WantedBy=default.target\n; RequiredBy=other.target",
            false,
        ),
        ("[Service]\nEnvironment=NAME=value", false),
        ("[Install]\n[Service]\nEnvironment=NAME=value", false),
        ("[Install]\nbare-text", false),
        ("WantedBy=default.target", false),
    ] {
        assert_eq!(has_install_directives(content), expected, "{content}");
    }
    assert_eq!(
        install_targets("[Install]\nAlso=\nWantedBy=default.target").unwrap(),
        ["default.target.wants"]
    );
}

#[cfg(target_os = "linux")]
#[test]
fn offline_dropin_enablement_matches_native_systemctl_in_a_disposable_root() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("native");
    let home = fixture.path().join("home");
    let native = root.join("etc/systemd/user");
    let user = home.join(".config/systemd/user");
    for directory in [&native, &user] {
        std::fs::create_dir_all(directory.join("audit.service.d")).unwrap();
        std::fs::write(
            directory.join("audit.service"),
            "[Service]\nExecStart=/usr/bin/true\n[Install]\nWantedBy=default.target\n",
        )
        .unwrap();
        std::fs::write(
            directory.join("audit.service.d/override.conf"),
            "[Install]\nWantedBy=\nWantedBy=other.target\nRequiredBy=another.target\n",
        )
        .unwrap();
    }
    let output = std::process::Command::new("systemctl")
        .args(["--root"])
        .arg(&root)
        .args(["--global", "enable", "audit.service"])
        .env_remove("SYSTEMD_UNIT_PATH")
        .output()
        .expect("native systemctl is required on Linux test workers");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let resource = offline_unit(&home, "audit.service", true);
    resource.apply().unwrap();
    for directory in [
        "other.target.wants",
        "another.target.requires",
        "default.target.wants",
    ] {
        assert_eq!(
            user.join(directory).join("audit.service").is_symlink(),
            native.join(directory).join("audit.service").is_symlink(),
            "{directory}"
        );
    }
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    let disabled = offline_unit(&home, "audit.service", false);
    disabled.apply().unwrap();
    assert_eq!(disabled.current_state().unwrap(), ResourceState::Correct);
}
