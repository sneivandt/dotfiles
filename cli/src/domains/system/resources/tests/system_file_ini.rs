use super::*;

#[cfg(unix)]
#[test]
fn paccache_drop_in_never_replaces_custom_environment_lists() {
    const DROP_IN: &str =
        include_str!("../../../../../../system/systemd/system/paccache.service.d/retention.conf");
    const ENV_FILE: &str =
        include_str!("../../../../../../system/systemd/system/paccache.service.d/retention.env");
    let previous = "[Service]\nEnvironmentFile=\nEnvironment=PACCACHE_ARGS=-k2\n";
    let custom = "[Service]\nEnvironment=SITE=blue\nEnvironmentFile=/srv/site/cache.env\n\
                      EnvironmentFile=-/srv/mirror.env\nNice=7\n";
    let directives: Vec<_> = DROP_IN
        .lines()
        .filter(|line| line.starts_with("EnvironmentFile="))
        .collect();
    assert_eq!(
        directives,
        ["EnvironmentFile=/etc/systemd/system/paccache.service.d/retention.env"]
    );
    assert!(
        DROP_IN.contains(
            "\nConditionPathExists=/etc/systemd/system/paccache.service.d/retention.env\n"
        )
    );
    assert_eq!(ENV_FILE.trim(), "PACCACHE_ARGS=-k2");
    for (name, desired, conflicts) in [
        ("retention.conf", DROP_IN, [previous, custom]),
        (
            "retention.env",
            ENV_FILE,
            ["", "# operator override\nCACHE=/srv/private\n"],
        ),
    ] {
        let fixture = PrivateStage::new(name);
        let prepare = || fixture.prepare(desired, "create-only");
        for existing in conflicts {
            fixture.write_target(existing);
            assert!(prepare().is_err());
            assert_eq!(fixture.target_contents(), existing);
            assert!(fixture.stage_is_empty());
        }
        let backup = fixture.root.path().join(format!("{name}.before-review"));
        fs::rename(&fixture.target, &backup).unwrap();
        assert_eq!(prepare().unwrap(), "prepared");
        assert!(!fixture.target.exists());
        fs::rename(fixture.stage.join("input"), &fixture.target).unwrap();
        assert_eq!(fixture.target_contents(), desired);
        assert_eq!(prepare().unwrap(), "already-correct");
        assert!(fixture.stage_is_empty());
        assert_eq!(fs::read_to_string(backup).unwrap(), conflicts[1]);
    }
}

#[test]
fn paccache_policy_file_is_required_before_publishing_its_drop_in() {
    use crate::engine::{ProcessOpts, process_resources_with_cache};
    use crate::test_helpers::make_linux_context;

    let fixture = tempfile::tempdir_in(".").unwrap();
    let root = fixture.path().canonicalize().unwrap();
    let mut entries = super::super::super::super::config::system_files::load_all(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../conf/system-files.toml"),
    )
    .unwrap();
    entries.retain(|entry| {
        entry
            .source
            .starts_with("systemd/system/paccache.service.d")
    });
    assert_eq!(entries.len(), 2);
    for entry in &mut entries {
        entry.origin = Some(root.clone());
        entry.target = root.join(entry.source.file_name().unwrap());
        let src = entry.source_path();
        fs::create_dir_all(src.parent().unwrap()).unwrap();
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../system")
                .join(&entry.source),
            src,
        )
        .unwrap();
    }
    let env_path = root.join("retention.env");
    let drop_in = root.join("retention.conf");
    let customized = "# site-specific mirror cache\nPACCACHE_ARGS=-k10\n";
    fs::write(&env_path, customized).unwrap();
    let run = || {
        process_resources_with_cache(
            &make_linux_context(root.clone(), None),
            entries
                .iter()
                .cloned()
                .map(|entry| fixture_resource(entry, Arc::new(MockExecutor::new()))),
            &(),
            |resource, ()| resource.installation_state(),
            &ProcessOpts::strict("configure").sequential(),
        )
    };
    assert!(run().is_err());
    assert!(
        !drop_in.exists(),
        "must not activate custom cleanup arguments"
    );
    assert_eq!(fs::read_to_string(&env_path).unwrap(), customized);
    // After the first installation step is interrupted the old service still
    // has no reference to the new env file, and retry need only add the drop-in.
    fs::write(&env_path, "PACCACHE_ARGS=-k2\n").unwrap();
    let states: Vec<_> = entries
        .iter()
        .cloned()
        .map(|entry| {
            fixture_resource(entry, Arc::new(MockExecutor::new()))
                .installation_state()
                .unwrap()
        })
        .collect();
    assert_eq!(states, [ResourceState::Correct, ResourceState::Missing]);
    let previous = "[Service]\nEnvironmentFile=\nEnvironment=PACCACHE_ARGS=-k2\n";
    fs::write(&drop_in, previous).unwrap();
    assert!(run().is_err());
    assert_eq!(fs::read_to_string(&drop_in).unwrap(), previous);
    // Operator review: keep the complete old file outside the active drop-ins.
    fs::rename(&drop_in, root.join("reviewed-old.conf.bak")).unwrap();
    fs::copy(entries.last().unwrap().source_path(), &drop_in).unwrap();
    assert!(run().is_ok());
    fs::write(&drop_in, "# post-install operator revision\n").unwrap();
    assert!(run().is_err());
    assert_eq!(
        fs::read_to_string(&drop_in).unwrap(),
        "# post-install operator revision\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("reviewed-old.conf.bak")).unwrap(),
        previous
    );
}

#[test]
fn ini_setting_recognition_preserves_lexical_edge_cases() {
    for (label, line, expected) in [
        (
            "only the first equals sign separates the value",
            " \tkey \t= first = second \t",
            Some(("key", Some("first = second"))),
        ),
        ("empty value", "key \t= \t", Some(("key", Some("")))),
        ("bare flag with tabs", "\tFlag\t", Some(("Flag", None))),
        (
            "whitespace inside an assigned key is retained",
            " first\tsecond = value ",
            Some(("first\tsecond", Some("value"))),
        ),
        (
            "inline comment characters are literal",
            "key=value # hash ; semicolon",
            Some(("key", Some("value # hash ; semicolon"))),
        ),
        ("empty line", "", None),
        ("whitespace", " \t ", None),
        ("hash comment", "\t# key=value", None),
        ("semicolon comment", "\t; Flag", None),
        ("section header", " \t[other]\t ", None),
        ("equals sign inside a section header", "[other=value]", None),
    ] {
        assert_eq!(ini_line_key(line), expected.map(|(key, _)| key), "{label}");
        let fragment = format!("[options]\n{line}\n");
        let expected_settings = expected
            .map(|(key, value)| {
                (
                    "[options]".to_string(),
                    key.to_string(),
                    value.map(str::to_string),
                )
            })
            .into_iter()
            .collect::<Vec<_>>();
        for strict in [false, true] {
            assert_eq!(
                ini_settings(&fragment, strict).unwrap(),
                expected_settings,
                "{label}, strict={strict}"
            );
        }

        if let Some((key, value)) = expected {
            let current = format!("[options]\n{line}\n{line}\n");
            let setting = value.map_or_else(|| key.to_string(), |value| format!("{key}={value}"));
            let merged = merge_ini(&current, &fragment).unwrap();
            assert_eq!(merged, format!("[options]\n{setting}\n"), "{label}");
            assert!(contains_ini(&merged, &fragment).unwrap(), "{label}");
            assert_eq!(merge_ini(&merged, &fragment).unwrap(), merged, "{label}");
        }
    }
}

#[test]
fn ini_settings_preserve_strict_diagnostics_and_permissive_skips() {
    for (label, content, expected_error) in [
        (
            "invalid bare setting inside a section",
            "[options]\n\tbad setting\t\n",
            "INI fragment contains an invalid bare setting: bad setting",
        ),
        (
            "invalid bare setting precedes the missing-section diagnostic",
            "\tbad setting\t\n",
            "INI fragment contains an invalid bare setting: bad setting",
        ),
        (
            "assignment outside a section",
            "\tkey = value\t\n",
            "INI fragment contains a setting outside a section: key = value",
        ),
        (
            "bare flag outside a section",
            "\tFlag\t\n",
            "INI fragment contains a setting outside a section: Flag",
        ),
        (
            "first invalid line wins",
            "bad setting\nkey=value\n",
            "INI fragment contains an invalid bare setting: bad setting",
        ),
        (
            "first out-of-section setting wins",
            "key=value\nbad setting\n",
            "INI fragment contains a setting outside a section: key=value",
        ),
    ] {
        assert_eq!(
            ini_settings(content, true).unwrap_err().to_string(),
            expected_error,
            "{label}"
        );
        assert!(ini_settings(content, false).unwrap().is_empty(), "{label}");
        let continued = format!("{content}[options]\nkeep=yes\n");
        assert_eq!(
            ini_settings(&continued, false).unwrap(),
            [(
                "[options]".to_string(),
                "keep".to_string(),
                Some("yes".to_string()),
            )],
            "{label}"
        );
    }
}

#[test]
fn malformed_ini_section_cannot_claim_a_key_from_another_section() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let root = dunce::canonicalize(fixture.path()).unwrap();
    fs::create_dir(root.join("system")).unwrap();
    fs::write(root.join("system/fragment"), "[boot]\ncommand=managed\n").unwrap();
    let target = root.join("wsl.conf");
    let mut executor = MockExecutor::new();
    executor.expect_execute().never();
    let resource = fixture_resource(
        entry(&root, &target, "fragment", MergeStrategy::Ini),
        Arc::new(executor),
    );
    for value in ["managed", "site-local"] {
        let original = format!("[boot]\n[other\ncommand={value}\n");
        fs::write(&target, &original).unwrap();
        assert!(merge_ini(&original, "[boot]\ncommand=managed\n").is_err());
        assert!(matches!(resource.current_state().unwrap(),
                ResourceState::Invalid { reason } if reason.contains("line 2")));
        assert!(resource.apply().is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), original);
    }
}

#[test]
fn ini_merge_supports_bare_settings() {
    let current = "[options]\n#Color\nColor\nColor\nCheckSpace\nParallelDownloads = 2\n";
    let fragment = "[options]\nColor\nILoveCandy\nParallelDownloads = 5\n";

    let merged = merge_ini(current, fragment).unwrap();

    assert!(merged.contains("#Color"));
    assert!(merged.contains("CheckSpace"));
    assert_eq!(merged.lines().filter(|line| *line == "Color").count(), 1);
    assert!(merged.contains("ILoveCandy"));
    assert!(!merged.contains("ParallelDownloads = 2"));
    assert!(merged.contains("ParallelDownloads=5"));
    assert!(contains_ini(&merged, fragment).unwrap());
    assert!(!contains_ini("[options]\ncolor\n", "[options]\nColor\n").unwrap());
    assert!(merge_ini("[options]\nColor\n", "[options]\nColor enabled\n").is_err());
}

#[test]
fn ini_fragments_reject_conflicting_values_including_repeated_sections() {
    for fragment in [
        "[options]\nmanaged=first\nmanaged=second\n",
        "[options]\nmanaged\nmanaged=value\n",
        "[options]\nmanaged=first\n[other]\nkeep=yes\n[options]\nmanaged=second\n",
    ] {
        let error = ini_settings(fragment, true).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("conflicting values for managed in [options]"),
            "{fragment}: {error}"
        );
        assert!(merge_ini("", fragment).is_err(), "{fragment}");
        assert!(contains_ini("", fragment).is_err(), "{fragment}");
        assert!(
            ini_settings(fragment, false).is_ok(),
            "existing target duplicates remain repairable"
        );
    }
}

#[test]
fn identical_ini_fragment_duplicates_and_keys_in_other_sections_converge() {
    let fragment = "[options]\nmanaged=value\nmanaged=value\nFlag\nFlag\n\
            [other]\nmanaged=other\n[options]\nmanaged=value\n";
    let current = "[options]\nmanaged=old\nmanaged=duplicate\nFlag=old\n\
            [unmanaged]\nmanaged=keep\n";

    let merged = merge_ini(current, fragment).unwrap();

    assert!(contains_ini(&merged, fragment).unwrap());
    assert_eq!(merge_ini(&merged, fragment).unwrap(), merged);
    assert!(merged.contains("[unmanaged]\nmanaged=keep\n"));
    assert_eq!(merged.matches("managed=value").count(), 1);
    assert_eq!(merged.lines().filter(|line| *line == "Flag").count(), 1);
    assert!(merged.contains("[other]\nmanaged=other\n"));
}

#[test]
fn ini_merge_preserves_setting_positions_and_removes_only_managed_duplicates() {
    for (label, current, expected) in [
        ("empty file", "", "[options]\nmanaged=new\n"),
        (
            "missing section",
            "[other]\nmanaged=keep\n",
            "[other]\nmanaged=keep\n\n[options]\nmanaged=new\n",
        ),
        (
            "empty section",
            "[options]\n# retained comment\n\n[other]\nmanaged=keep\n",
            "[options]\nmanaged=new\n# retained comment\n\n[other]\nmanaged=keep\n",
        ),
        (
            "missing key follows last setting, not trailing comments",
            "[options]\n# heading\nkeep=yes\n# trailing\n\n",
            "[options]\n# heading\nkeep=yes\nmanaged=new\n# trailing\n\n",
        ),
        (
            "existing key stays before following settings",
            "[options]\n# heading\nmanaged=old\nkeep=yes\nmanaged=duplicate\n",
            "[options]\n# heading\nmanaged=new\nkeep=yes\n",
        ),
        (
            "repeated sections retain unmanaged keys",
            "[options]\nmanaged=old\n[other]\nmanaged=keep\n [options] \nmanaged\nkeep=yes\n",
            "[options]\nmanaged=new\n[other]\nmanaged=keep\n [options] \nkeep=yes\n",
        ),
        (
            "key moves from later occurrence into first section",
            "[options]\nkeep=yes\n# trailing\n[options]\nmanaged=old\n",
            "[options]\nkeep=yes\nmanaged=new\n# trailing\n[options]\n",
        ),
        (
            "bare settings keep their positions",
            "[options]\n# managed=comment\nBareFlag\n; trailing\n",
            "[options]\n# managed=comment\nBareFlag\nmanaged=new\n; trailing\n",
        ),
    ] {
        let fragment = "[options]\nmanaged=new\n";
        let merged = merge_ini(current, fragment).unwrap();
        assert_eq!(merged, expected, "{label}");
        assert!(contains_ini(&merged, fragment).unwrap(), "{label}");
        assert_eq!(merge_ini(&merged, fragment).unwrap(), merged, "{label}");
    }
}
