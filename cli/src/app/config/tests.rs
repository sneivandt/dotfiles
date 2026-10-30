//! Unit tests for configuration loading and validation.

use super::*;
use crate::infra::config::category_matcher::Category;
use crate::infra::platform::{Os, Platform};

#[test]
fn section_inventory_reports_every_user_configured_slice() {
    let config = crate::test_helpers::empty_config(PathBuf::from("/repo"));
    let labels: Vec<&str> = config
        .section_counts()
        .iter()
        .map(|section| section.plural)
        .collect();

    assert!(labels.contains(&"git settings"));
    assert!(labels.contains(&"agent settings"));
    assert!(labels.contains(&"system files"));
    assert_eq!(labels.len(), 10);
}

/// Create a temporary directory tree with the minimal conf/ files required
/// by `Config::load` and return the `TempDir` (keep alive) + profile.
fn setup_load(
    platform: Platform,
    overrides: &[(&str, &str)],
) -> (tempfile::TempDir, profiles::Profile, Platform) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let conf = dir.path().join("conf");
    std::fs::create_dir_all(&conf).expect("create conf dir");

    for file in REQUIRED_CONFIG_FILES {
        std::fs::write(conf.join(file), "").expect("write empty toml");
    }

    for (name, content) in overrides {
        std::fs::write(conf.join(name), content).expect("write override toml");
    }

    let profile = profiles::Profile {
        name: "base".to_string(),
        active_categories: vec![Category::Base],
    };
    (dir, profile, platform)
}

fn linux() -> Platform {
    Platform::new(Os::Linux, false)
}

fn windows() -> Platform {
    Platform::new(Os::Windows, false)
}

fn write_overlay_config(overlay: &tempfile::TempDir, file: &str, content: &str) -> PathBuf {
    let conf = overlay.path().join("conf");
    std::fs::create_dir_all(&conf).expect("create overlay conf");
    let path = conf.join(file);
    std::fs::write(&path, content).expect("write overlay config");
    path
}

#[test]
fn active_and_validation_views_share_one_decode_per_source() {
    fn decode_then_remove(
        document: &ConfigDocument<'_>,
    ) -> Result<Vec<(String, Vec<symlinks::Symlink>)>> {
        let items = symlinks::decode(document)?;
        std::fs::remove_file(document.path)?;
        Ok(items)
    }

    let content = "[base]\nsymlinks = [{ source = 'active', target = '.active' }]\n\
                   [desktop]\nsymlinks = [{ source = 'inactive', target = '.inactive' }]\n";
    let (main, profile, _) = setup_load(linux(), &[("symlinks.toml", content)]);
    let overlay = tempfile::tempdir().unwrap();
    write_overlay_config(&overlay, "symlinks.toml", content);
    let sections = SectionLoader::new(main.path(), Some(overlay.path()), &profile, linux());
    let (active, all) = sections
        .collect_views(
            symlinks::SYMLINKS_TOML,
            decode_then_remove,
            symlinks::set_origin,
        )
        .expect("both views must use the decoded snapshot, not reopen either file");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].source, "active");
    assert_eq!(active[0].origin.as_deref(), Some(overlay.path()));
    assert_eq!(all.len(), 4);
    for (root, all_index) in [(main.path(), 0), (overlay.path(), 2)] {
        assert!(!root.join("conf").join("symlinks.toml").exists());
        assert_eq!(all[all_index].source, "active");
        assert_eq!(all[all_index + 1].source, "inactive");
        assert_eq!(all[all_index + 1].origin.as_deref(), Some(root));
    }
}

#[test]
fn every_main_inventory_file_is_required_by_the_aggregate_loader() {
    for file in REQUIRED_CONFIG_FILES {
        let (root, profile, platform) = setup_load(linux(), &[]);
        std::fs::remove_file(root.path().join("conf").join(file)).unwrap();
        let error = Config::load(root.path(), &profile, platform, None).unwrap_err();
        assert!(format!("{error:#}").contains(file), "{file}: {error:#}");
    }
}

#[test]
fn category_errors_retain_main_and_overlay_provenance() {
    for is_overlay in [false, true] {
        let (main, profile, platform) = setup_load(linux(), &[]);
        let overlay = tempfile::tempdir().unwrap();
        let source = if is_overlay { &overlay } else { &main };
        let path = write_overlay_config(source, "packages.toml", "[windwos]\npackages = []\n");
        let error =
            Config::load(main.path(), &profile, platform, Some(overlay.path())).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("unknown category 'windwos'"), "{message}");
        assert!(message.contains(&path.display().to_string()), "{message}");
        if is_overlay {
            assert_eq!(
                error.to_string(),
                format!("Invalid configuration in overlay {}", path.display())
            );
        }
    }
}

#[test]
fn load_overlay_replaces_matching_git_and_registry_values_and_preserves_origins() {
    let (dir, profile, platform) = setup_load(
        windows(),
        &[
            (
                "git-config.toml",
                "[base]\nsettings = [\
                 { key = 'core.editor', value = 'vim' },\
                 { key = 'core.autocrlf', value = 'false' },\
                 { key = 'remote.Work.url', value = 'keep-case-sensitive' }]\n",
            ),
            (
                "registry.toml",
                "[console]\npath = 'HKCU:\\Console'\n[console.values]\nFontSize = 14\nFontFamily = 54\n",
            ),
        ],
    );
    for (editor, font_size, expected_font) in [("vim", "'0x0E'", "0x0E"), ("nano", "15", "15")] {
        let overlay = tempfile::tempdir().unwrap();
        let git_path = write_overlay_config(
            &overlay,
            "git-config.toml",
            &format!(
                "[base]\nsettings = [\
                 {{ key = 'CORE.EDITOR', value = '{editor}' }},\
                 {{ key = 'remote.work.url', value = 'different-subsection' }}]\n"
            ),
        );
        let registry_path = write_overlay_config(
            &overlay,
            "registry.toml",
            &format!(
                "[display]\npath = 'hkcu:\\console'\n[display.values]\nfontsize = {font_size}\n"
            ),
        );
        let config = Config::load(dir.path(), &profile, platform, Some(overlay.path())).unwrap();
        assert_eq!(config.git_settings.len(), 4);
        assert_eq!(config.git_settings[0].key, "core.autocrlf");
        assert_eq!(config.git_settings[1].value, "keep-case-sensitive");
        assert_eq!(config.git_settings[2].key, "CORE.EDITOR");
        assert_eq!(config.git_settings[2].value, editor);
        assert_eq!(config.git_settings[3].value, "different-subsection");
        assert!(
            config.git_settings[2]
                .origin
                .as_ref()
                .unwrap()
                .contains(&git_path.display().to_string())
        );
        assert_eq!(config.registry.len(), 2);
        assert_eq!(config.registry[0].value_name, "FontFamily");
        assert_eq!(config.registry[0].value_data, "54");
        assert_eq!(config.registry[1].value_name, "fontsize");
        assert_eq!(config.registry[1].value_data, expected_font);
        assert!(
            config.registry[1]
                .origin
                .as_ref()
                .unwrap()
                .contains(&registry_path.display().to_string())
        );
        assert!(git_config::validate_conflicts(&config.git_settings).is_empty());
        assert!(registry::validate_conflicts(&config.registry).is_empty());
    }
}

#[test]
fn load_checks_only_active_desired_state_for_conflicts() {
    let (dir, profile, platform) = setup_load(
        linux(),
        &[
            (
                "git-config.toml",
                "[base]\nsettings = [{ key = \"core.editor\", value = \"vim\" }]\n\
                 [desktop]\nsettings = [{ key = \"core.editor\", value = \"nano\" }]\n",
            ),
            (
                "registry.toml",
                "[first]\npath = 'HKCU:\\Console'\n[first.values]\nFontSize = 14\n\
                 [second]\npath = 'HKCU:\\Console'\n[second.values]\nFontSize = 15\n",
            ),
        ],
    );
    let config = Config::load(dir.path(), &profile, platform, None)
        .expect("inactive profile and platform declarations must not conflict");
    assert_eq!(config.git_settings.len(), 1);
    assert!(config.registry.is_empty());
    let mut desktop = profile.clone();
    desktop.active_categories.push(Category::Desktop);
    for (active_profile, active_platform, code) in [
        (&desktop, platform, "git.conflicting-values"),
        (&profile, windows(), "registry.conflicting-values"),
    ] {
        let error = Config::load(dir.path(), active_profile, active_platform, None).unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("contradictory desired state:\n"),
            "{message}"
        );
        assert!(message.contains(code), "{active_platform:?}: {error:#}");
    }
}

#[test]
fn load_agent_settings_overlay_overrides_only_exact_active_harness_keys() {
    let (main, profile, platform) = setup_load(
        linux(),
        &[(
            "agent-settings.toml",
            r#"[base]
settings = [
  { target = "copilot", key = "model", value = "baseline" },
  { target = "copilot", key = "Model", value = "case-sensitive" },
  { target = "codex", key = "model", value = "other-harness" },
  { target = "copilot", key = "beep", value = false },
]
"#,
        )],
    );
    let overlay = tempfile::tempdir().unwrap();
    write_overlay_config(
        &overlay,
        "agent-settings.toml",
        r#"[base]
settings = [
  { target = "copilot", key = "model", value = "work-model" },
  { target = "copilot", key = "memory", value = true },
]
[desktop]
settings = [{ target = "copilot", key = "model", value = "inactive" }]
"#,
    );
    let config = Config::load(main.path(), &profile, platform, Some(overlay.path())).unwrap();
    let actual: Vec<_> = config
        .agent_settings
        .iter()
        .map(|setting| {
            (
                setting.target.name(),
                setting.key.as_str(),
                setting.value.clone(),
            )
        })
        .collect();
    assert_eq!(
        actual,
        vec![
            (
                "copilot",
                "Model",
                toml::Value::String("case-sensitive".into())
            ),
            (
                "codex",
                "model",
                toml::Value::String("other-harness".into())
            ),
            ("copilot", "beep", toml::Value::Boolean(false)),
            ("copilot", "model", toml::Value::String("work-model".into())),
            ("copilot", "memory", toml::Value::Boolean(true)),
        ]
    );
    assert!(agent_settings::validate(&config.agent_settings).is_empty());
}

#[test]
fn load_overlay_overrides_packages_units_permissions_and_extensions_by_target() {
    let (main, profile, platform) = setup_load(
        linux(),
        &[
            ("packages.toml", "[base]\npackages = ['keep', 'tool']\n"),
            (
                "systemd-units.toml",
                "[base]\nunits = ['example.service', \
                 { name = 'example.service', scope = 'system', enabled = true }]\n",
            ),
            (
                "chmod.toml",
                "[base]\npermissions = [\
                 { path = 'ssh', mode = '700' },\
                 { path = 'ssh/config', mode = '600' }]\n",
            ),
            (
                "vscode-extensions.toml",
                "[base]\nextensions = ['keep.extension', 'Publisher.Extension']\n",
            ),
        ],
    );
    let overlay = tempfile::tempdir().unwrap();
    for (file, content) in [
        (
            "packages.toml",
            "[base]\npackages = [{ name = 'tool', aur = true }, 'new']\n",
        ),
        (
            "systemd-units.toml",
            "[base]\nunits = [{ name = 'example.service', scope = 'user', enabled = false }]\n",
        ),
        (
            "chmod.toml",
            "[base]\npermissions = [{ path = './ssh/config', mode = '640' }]\n",
        ),
        (
            "vscode-extensions.toml",
            "[base]\nextensions = ['publisher.extension', 'new.extension']\n",
        ),
    ] {
        write_overlay_config(&overlay, file, content);
    }
    let config = Config::load(main.path(), &profile, platform, Some(overlay.path())).unwrap();
    assert_eq!(
        config
            .packages
            .iter()
            .map(|p| (p.name.as_str(), p.is_aur))
            .collect::<Vec<_>>(),
        [("keep", false), ("tool", true), ("new", false)]
    );
    assert_eq!(config.units.len(), 2);
    assert_eq!(config.units[0].scope, systemd_units::UnitScope::System);
    assert!(config.units[0].enabled);
    assert_eq!(config.units[1].scope, systemd_units::UnitScope::User);
    assert!(!config.units[1].enabled);
    assert_eq!(
        config
            .chmod
            .iter()
            .map(|p| (p.path.as_str(), p.mode.as_str()))
            .collect::<Vec<_>>(),
        [("ssh", "700"), ("./ssh/config", "640")]
    );
    assert_eq!(
        config.vscode_extensions,
        ["keep.extension", "publisher.extension", "new.extension"]
    );
    assert_eq!(config.validation_chmod.len(), 3);
}

#[test]
fn load_package_identity_respects_the_selected_platform() {
    let (main, profile, _) = setup_load(
        linux(),
        &[("packages.toml", "[base]\npackages = ['Vendor.Tool']\n")],
    );
    let overlay = tempfile::tempdir().unwrap();
    write_overlay_config(
        &overlay,
        "packages.toml",
        "[base]\npackages = ['vendor.tool']\n",
    );
    for (platform, expected) in [(windows(), 1), (linux(), 2)] {
        let config = Config::load(main.path(), &profile, platform, Some(overlay.path())).unwrap();
        assert_eq!(config.packages.len(), expected, "{platform:?}");
        assert_eq!(config.packages.last().unwrap().name, "vendor.tool");
    }
}

#[test]
fn load_permission_overrides_match_dot_prefixed_and_plain_paths() {
    for (main_path, overlay_path) in [
        ("ssh/config", ".ssh/config"),
        (".ssh/config", "./ssh/config"),
        ("./.ssh/config", "ssh/./config"),
    ] {
        let main_content =
            format!("[base]\npermissions = [{{ path = '{main_path}', mode = '600' }}]\n");
        let (main, profile, platform) = setup_load(linux(), &[("chmod.toml", &main_content)]);
        let overlay = tempfile::tempdir().unwrap();
        write_overlay_config(
            &overlay,
            "chmod.toml",
            &format!("[base]\npermissions = [{{ path = '{overlay_path}', mode = '640' }}]\n"),
        );
        let config = Config::load(main.path(), &profile, platform, Some(overlay.path())).unwrap();
        assert_eq!(config.chmod.len(), 1, "{main_path} -> {overlay_path}");
        assert_eq!(config.chmod[0].path, overlay_path);
        assert_eq!(config.chmod[0].mode, "640");
    }
}

#[cfg(unix)]
#[test]
fn load_system_file_override_replaces_fragment_and_strategy_from_overlay() {
    let (main, profile, platform) = setup_load(
        linux(),
        &[(
            "system-files.toml",
            "[base]\nfiles = [\
             { source = 'keep.conf', merge = 'ini' },\
             { target = '/etc/example.conf', source = 'main.conf', merge = 'toml' }]\n",
        )],
    );
    let overlay = tempfile::tempdir().unwrap();
    write_overlay_config(
        &overlay,
        "system-files.toml",
        "[base]\nfiles = [{ target = '/etc/./example.conf', source = 'work.conf', merge = 'ini' }]\n",
    );
    let config = Config::load(main.path(), &profile, platform, Some(overlay.path())).unwrap();
    assert_eq!(config.system_files.len(), 2);
    assert_eq!(config.system_files[0].origin.as_deref(), Some(main.path()));
    assert_eq!(
        config.system_files[1].merge,
        system_files::MergeStrategy::Ini
    );
    assert_eq!(
        config.system_files[1].source_path(),
        overlay.path().join("system/work.conf")
    );
    assert_eq!(config.validation_system_files.len(), 3);
}

#[test]
fn load_overrides_expanded_symlinks_but_keeps_nonmatching_glob_entries() {
    let (main, profile, platform) = setup_load(
        linux(),
        &[("symlinks.toml", "[base]\nsymlinks = ['skills/*']\n")],
    );
    for name in ["keep", "replace"] {
        std::fs::create_dir_all(main.path().join("symlinks/skills").join(name)).unwrap();
    }
    let overlay = tempfile::tempdir().unwrap();
    write_overlay_config(
        &overlay,
        "symlinks.toml",
        "[base]\nsymlinks = [{ source = 'work', target = '.skills/./replace' }]\n",
    );
    let config = Config::load(main.path(), &profile, platform, Some(overlay.path())).unwrap();
    assert_eq!(config.symlinks.len(), 2);
    assert_eq!(config.symlinks[0].source, "skills/keep");
    assert_eq!(config.symlinks[0].origin.as_deref(), Some(main.path()));
    assert_eq!(config.symlinks[1].source, "work");
    assert_eq!(config.symlinks[1].origin.as_deref(), Some(overlay.path()));
}

#[test]
fn load_still_rejects_symlink_ambiguity_within_a_source_and_cross_source_overlaps() {
    for (main_content, overlay_content) in [
        (
            "[base]\nsymlinks = ['bashrc', { source = 'other', target = '.bashrc' }]\n",
            "[base]\nsymlinks = ['bashrc']\n",
        ),
        (
            "[base]\nsymlinks = ['bashrc']\n",
            "[base]\nsymlinks = ['bashrc', { source = 'other', target = '.bashrc' }]\n",
        ),
        (
            "[base]\nsymlinks = ['config/example']\n",
            "[base]\nsymlinks = ['config/example/child']\n",
        ),
    ] {
        let (main, profile, platform) = setup_load(linux(), &[("symlinks.toml", main_content)]);
        let overlay = tempfile::tempdir().unwrap();
        write_overlay_config(&overlay, "symlinks.toml", overlay_content);
        let error =
            Config::load(main.path(), &profile, platform, Some(overlay.path())).unwrap_err();
        assert!(format!("{error:#}").contains("symlink target"), "{error:#}");
    }
}

#[test]
fn load_agent_settings_keeps_main_defaults_without_active_overrides() {
    let (main, profile, platform) = setup_load(
        linux(),
        &[(
            "agent-settings.toml",
            "[base]\nsettings = [{ target = 'copilot', key = 'model', value = 'baseline' }]\n",
        )],
    );
    let overlay = tempfile::tempdir().unwrap();
    for (use_overlay, content) in [
        (false, None),
        (true, None),
        (true, Some("")),
        (
            true,
            Some(
                "[desktop]\nsettings = [{ target = 'copilot', key = 'model', value = 'inactive' }]\n",
            ),
        ),
    ] {
        if let Some(content) = content {
            write_overlay_config(&overlay, "agent-settings.toml", content);
        }
        let config = Config::load(
            main.path(),
            &profile,
            platform,
            use_overlay.then_some(overlay.path()),
        )
        .unwrap();
        assert_eq!(config.agent_settings.len(), 1);
        assert_eq!(config.agent_settings[0].value.as_str(), Some("baseline"));
    }
}

#[test]
fn load_agent_settings_rejects_conflicts_within_either_source_before_overriding() {
    let valid = "[base]\nsettings = [{ target = 'copilot', key = 'model', value = 'baseline' }]\n";
    for is_overlay in [false, true] {
        for conflict in [
            "[base]\nsettings = [\
             { target = 'copilot', key = 'model', value = 'first' },\
             { target = 'copilot', key = 'model', value = 'second' }]\n",
            "[base]\nsettings = [{ target = 'copilot', key = 'model', value = 'first' }]\n\
             [desktop]\nsettings = [{ target = 'copilot', key = 'model', value = 'second' }]\n",
        ] {
            let (main, mut profile, platform) =
                setup_load(linux(), &[("agent-settings.toml", valid)]);
            profile.active_categories.push(Category::Desktop);
            let overlay = tempfile::tempdir().unwrap();
            write_overlay_config(&overlay, "agent-settings.toml", valid);
            let source = if is_overlay { &overlay } else { &main };
            let path = write_overlay_config(source, "agent-settings.toml", conflict);
            let error =
                Config::load(main.path(), &profile, platform, Some(overlay.path())).unwrap_err();
            let message = format!("{error:#}");
            assert!(
                message.contains("agent-settings.conflicting-values"),
                "{message}"
            );
            assert!(message.contains("copilot:model"), "{message}");
            assert!(message.contains(&path.display().to_string()), "{message}");
        }
    }
}

#[test]
fn load_agent_settings_cross_source_parent_child_conflicts_fail_before_task_execution() {
    for harness in ["copilot", "codex"] {
        for (parent_value, child_value, conflicting) in [
            ("{ enabled = true }", "false", true),
            ("false", "true", true),
            ("{ enabled = true }", "true", false),
        ] {
            let parent = format!(
                "[base]\nsettings = [{{ target = '{harness}', key = 'feature', value = {parent_value} }}]\n"
            );
            let child = format!(
                "[base]\nsettings = [{{ target = '{harness}', key = 'feature.enabled', value = {child_value} }}]\n"
            );
            for (main_content, overlay_content) in [(&parent, &child), (&child, &parent)] {
                let (main, profile, platform) =
                    setup_load(linux(), &[("agent-settings.toml", main_content)]);
                let overlay = tempfile::tempdir_in(".").unwrap();
                write_overlay_config(&overlay, "agent-settings.toml", overlay_content);
                let result = Config::load(main.path(), &profile, platform, Some(overlay.path()));
                if conflicting {
                    let error =
                        result.expect_err("merged conflicts must abort configuration loading");
                    let message = format!("{error:#}");
                    assert!(
                        message.contains("agent-settings.conflicting-values"),
                        "{message}"
                    );
                    assert!(message.contains("feature.enabled"), "{message}");
                    assert!(
                        message.contains("conflicting parent/child settings"),
                        "{message}"
                    );
                } else {
                    let config = result.expect("compatible parent/child declarations remain valid");
                    assert_eq!(config.agent_settings.len(), 2);
                    assert!(agent_settings::validate_conflicts(&config.agent_settings).is_empty());
                }
            }
        }
    }
}

#[test]
fn load_keeps_inactive_profile_sources_and_origins_for_validation() {
    let (dir, profile, platform) = setup_load(
        linux(),
        &[
            (
                "symlinks.toml",
                "[base]\nsymlinks = [\"bashrc\"]\n[desktop]\nsymlinks = [\"config/i3\"]\n",
            ),
            (
                "chmod.toml",
                "[base]\npermissions = [{ mode = \"755\", path = \"bin/main\" }]\n\
                 [desktop]\npermissions = [{ mode = \"755\", path = \"bin/desktop\" }]\n",
            ),
        ],
    );
    let overlay = tempfile::tempdir().unwrap();
    write_overlay_config(
        &overlay,
        "symlinks.toml",
        "[base]\nsymlinks = [\"zshrc\"]\n[desktop]\nsymlinks = [\"config/hypr\"]\n",
    );
    write_overlay_config(
        &overlay,
        "chmod.toml",
        "[base]\npermissions = [{ mode = \"755\", path = \"bin/overlay\" }]\n\
         [desktop]\npermissions = [{ mode = \"755\", path = \"bin/overlay-desktop\" }]\n",
    );

    for overlay_root in [None, Some(overlay.path())] {
        let config = Config::load(dir.path(), &profile, platform, overlay_root).unwrap();
        for (items, expected) in [
            (
                &config.symlinks,
                vec![("bashrc", dir.path()), ("zshrc", overlay.path())],
            ),
            (
                &config.validation_symlinks,
                vec![
                    ("bashrc", dir.path()),
                    ("config/i3", dir.path()),
                    ("zshrc", overlay.path()),
                    ("config/hypr", overlay.path()),
                ],
            ),
        ] {
            let expected: Vec<_> = expected
                .into_iter()
                .filter(|(_, root)| *root == dir.path() || overlay_root.is_some())
                .map(|(source, root)| (source, Some(root)))
                .collect();
            let actual: Vec<_> = items
                .iter()
                .map(|item| (item.source.as_str(), item.origin.as_deref()))
                .collect();
            assert_eq!(actual, expected, "overlay: {overlay_root:?}");
        }
        let expected = if overlay_root.is_some() {
            vec![
                "bin/main",
                "bin/desktop",
                "bin/overlay",
                "bin/overlay-desktop",
            ]
        } else {
            vec!["bin/main", "bin/desktop"]
        };
        assert_eq!(
            config
                .validation_chmod
                .iter()
                .map(|item| item.path.as_str())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            config
                .chmod
                .iter()
                .map(|item| item.path.as_str())
                .collect::<Vec<_>>(),
            expected
                .into_iter()
                .filter(|path| !path.ends_with("desktop"))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn load_expands_overlay_symlink_globs() {
    let (dir, profile, platform) = setup_load(linux(), &[]);
    let overlay = tempfile::tempdir().expect("create overlay dir");
    write_overlay_config(
        &overlay,
        "symlinks.toml",
        "[base]\nsymlinks = [{ source = \"skills/*\", target = \".copilot/skills/*\" }]\n",
    );
    std::fs::create_dir_all(
        overlay
            .path()
            .join("symlinks")
            .join("skills")
            .join("example-skill"),
    )
    .expect("create overlay skill");

    let config = Config::load(dir.path(), &profile, platform, Some(overlay.path()))
        .expect("load should succeed");
    assert_eq!(config.symlinks.len(), 1);
    assert_eq!(config.symlinks[0].source, "skills/example-skill");
    assert_eq!(
        config.symlinks[0].target.as_deref(),
        Some(".copilot/skills/example-skill")
    );
    assert_eq!(config.symlinks[0].origin.as_deref(), Some(overlay.path()));
    assert_eq!(config.validation_symlinks[0].source, "skills/*");
    assert_eq!(
        config.validation_symlinks[0].origin.as_deref(),
        Some(overlay.path())
    );
}

#[test]
fn load_appends_overlay_packages_and_scripts() {
    let (dir, profile, platform) = setup_load(
        linux(),
        &[
            ("packages.toml", "[base]\npackages = [\"git\"]\n"),
            (
                "scripts.toml",
                "not valid TOML: main scripts are never loaded",
            ),
        ],
    );
    let overlay = tempfile::tempdir().expect("create overlay dir");
    write_overlay_config(&overlay, "packages.toml", "[base]\npackages = [\"curl\"]\n");
    write_overlay_config(
        &overlay,
        "scripts.toml",
        r#"
[base]
scripts = [{ name = "Setup SSH", path = "scripts/ssh.sh" }]

[desktop]
scripts = [{ name = "Setup desktop", path = "scripts/desktop.sh" }]
"#,
    );

    let config = Config::load(dir.path(), &profile, platform, Some(overlay.path()))
        .expect("load should succeed");

    assert_eq!(config.root, dir.path());
    assert_eq!(config.overlay.as_deref(), Some(overlay.path()));
    assert_eq!(
        config
            .packages
            .iter()
            .map(|package| package.name.as_str())
            .collect::<Vec<_>>(),
        vec!["git", "curl"],
        "overlay packages should append to main packages"
    );
    assert_eq!(config.scripts.len(), 1);
    assert_eq!(config.scripts[0].name, "Setup SSH");
    assert_eq!(config.scripts[0].path, "scripts/ssh.sh");
}

#[test]
fn load_preserves_main_and_overlay_parse_error_context() {
    let (dir, profile, platform) = setup_load(linux(), &[]);
    let overlay = tempfile::tempdir().expect("create overlay dir");
    let syntax_path = write_overlay_config(&overlay, "scripts.toml", "[base\nscripts = [");
    let syntax_error =
        Config::load(dir.path(), &profile, platform, Some(overlay.path())).unwrap_err();
    assert_eq!(
        syntax_error.to_string(),
        format!("Invalid configuration in overlay {}", syntax_path.display())
    );
    std::fs::remove_file(syntax_path).unwrap();
    for (root, prefix) in [(dir.path(), ""), (overlay.path(), "overlay ")] {
        let path = root.join("conf").join("packages.toml");
        std::fs::write(&path, "[base]\npackages = 42\n").unwrap();
        let error = Config::load(dir.path(), &profile, platform, Some(overlay.path())).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("Invalid syntax in {prefix}{}", path.display())
        );
        let sources = format!("{error:#}");
        assert!(sources.contains("Failed to parse TOML config"));
        assert!(sources.contains("invalid type: integer"));
        std::fs::write(path, "").unwrap();
    }
}

#[test]
fn load_filters_systemd_units_by_platform() {
    for (platform, expected_count) in [(linux(), 1), (windows(), 0)] {
        let (dir, profile, platform) = setup_load(
            platform,
            &[("systemd-units.toml", "[base]\nunits = [\"ssh.service\"]\n")],
        );
        let config = Config::load(dir.path(), &profile, platform, None).unwrap();
        assert_eq!(config.units.len(), expected_count, "{platform:?}");
    }
}

#[test]
fn load_still_parses_inactive_platform_and_profile_config_strictly() {
    for (platform, file, contents, unknown_field) in [
        (
            windows(),
            "systemd-units.toml",
            "[base]\nunits = [{ name = \"example.service\", scop = \"user\" }]\n",
            "scop",
        ),
        (
            linux(),
            "registry.toml",
            "[console]\npath = 'HKCU:\\Console'\nvaluess = {}\n",
            "valuess",
        ),
        (
            linux(),
            "packages.toml",
            "[desktop]\npackages = [{ name = 'tool', provder = 'pacman' }]\n",
            "provder",
        ),
    ] {
        let (dir, profile, platform) = setup_load(platform, &[(file, contents)]);
        let error = Config::load(dir.path(), &profile, platform, None)
            .expect_err("inactive sections must still be structurally valid");
        assert_eq!(
            error.to_string(),
            format!(
                "Invalid syntax in {}",
                dir.path().join("conf").join(file).display()
            )
        );
        assert!(
            format!("{error:#}").contains(&format!("unknown field `{unknown_field}`")),
            "{platform:?}, {file}: {error:#}"
        );
    }
}

#[test]
fn load_overlay_replaces_a_normalized_symlink_target_and_preserves_origin() {
    let (dir, profile, platform) = setup_load(
        linux(),
        &[("symlinks.toml", "[base]\nsymlinks = [\"bashrc\"]\n")],
    );
    let overlay = tempfile::tempdir().unwrap();
    write_overlay_config(
        &overlay,
        "symlinks.toml",
        "[base]\nsymlinks = [{ source = \"overlay-bashrc\", target = \"./.bashrc\" }]\n",
    );
    let config = Config::load(dir.path(), &profile, platform, Some(overlay.path())).unwrap();
    assert_eq!(config.symlinks.len(), 1);
    assert_eq!(config.symlinks[0].source, "overlay-bashrc");
    assert_eq!(config.symlinks[0].origin.as_deref(), Some(overlay.path()));
    assert_eq!(config.validation_symlinks.len(), 2);
}

#[test]
fn load_preserves_nonfatal_validation_findings() {
    let (dir, profile, platform) = setup_load(
        linux(),
        &[
            (
                "packages.toml",
                "[base]\npackages = [\"\", \"git\", \"git\"]\n",
            ),
            (
                "chmod.toml",
                "[base]\npermissions = [{ mode = \"invalid\", path = \"../tool\" }]\n",
            ),
        ],
    );
    let config = Config::load(dir.path(), &profile, platform, None).unwrap();
    assert_eq!(
        config.packages.len(),
        3,
        "append loading must not deduplicate"
    );
    let diagnostics = config.validate(platform);
    let warning = diagnostics
        .iter()
        .find(|item| item.code.to_string() == "package.empty-name")
        .unwrap();
    assert_eq!(
        warning,
        &Diagnostic::warning(
            "packages.toml",
            "",
            crate::infra::config::DiagnosticCode::new("package", "empty-name"),
            "package name is empty",
        )
    );
    assert!(
        diagnostics
            .iter()
            .any(|item| item.code.to_string() == "chmod.invalid-mode")
    );
    assert!(
        diagnostics.iter().any(|item| {
            item.code.to_string() == "chmod.parent-in-path"
                && item.severity == crate::infra::config::Severity::Error
        }),
        "ordinary error-severity diagnostics are reported, not converted to load errors"
    );
}
