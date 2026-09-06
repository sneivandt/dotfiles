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
    assert_eq!(labels.len(), 9);
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
        excluded_categories: vec![Category::Desktop],
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
fn load_appends_equivalent_values_and_rejects_all_conflicts_before_publication() {
    let (dir, profile, platform) = setup_load(
        windows(),
        &[
            (
                "git-config.toml",
                "[base]\nsettings = [{ key = \"core.editor\", value = \"vim\" }]\n",
            ),
            (
                "registry.toml",
                "[console]\npath = 'HKCU:\\Console'\n[console.values]\nFontSize = 14\n",
            ),
        ],
    );
    for (name, editor, font_size, conflict) in [
        ("equivalent values", "vim", "'0x0E'", false),
        ("contradictory values", "nano", "15", true),
    ] {
        let overlay = tempfile::tempdir().unwrap();
        let git_path = write_overlay_config(
            &overlay,
            "git-config.toml",
            &format!("[base]\nsettings = [{{ key = \"CORE.EDITOR\", value = \"{editor}\" }}]\n"),
        );
        let registry_path = write_overlay_config(
            &overlay,
            "registry.toml",
            &format!(
                "[display]\npath = 'hkcu:\\console'\n[display.values]\nfontsize = {font_size}\n"
            ),
        );
        let result = Config::load(dir.path(), &profile, platform, Some(overlay.path()));
        if !conflict {
            let config = result.expect(name);
            assert_eq!(config.git_settings.len(), 2, "preserve append semantics");
            assert_eq!(config.registry.len(), 2, "preserve append semantics");
            assert!(git_config::validate_conflicts(&config.git_settings).is_empty());
            assert!(registry::validate_conflicts(&config.registry).is_empty());
            continue;
        }
        let error = result.expect_err(name);
        let message = format!("{error:#}");
        assert!(message.starts_with("contradictory desired state:\n  git-config.toml "));
        assert!(
            message.find("git.conflicting-values").unwrap()
                < message.find("registry.conflicting-values").unwrap(),
            "Git diagnostics must precede registry diagnostics: {message}"
        );
        for expected in [
            "[base] settings entry 1",
            "[console.values] \"FontSize\"",
            "[display.values] \"fontsize\"",
        ] {
            assert!(
                message.contains(expected),
                "missing {expected:?}: {message}"
            );
        }
        for path in [
            dir.path().join("conf").join("git-config.toml"),
            dir.path().join("conf").join("registry.toml"),
            git_path,
            registry_path,
        ] {
            assert!(
                message.contains(&path.display().to_string()),
                "missing {}: {message}",
                path.display()
            );
        }
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
    assert!(Config::load(dir.path(), &desktop, platform, None).is_err());
    assert!(Config::load(dir.path(), &profile, windows(), None).is_err());
}

#[test]
fn load_keeps_profile_excluded_sources_and_origins_for_validation() {
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
fn load_still_parses_systemd_config_on_windows() {
    let (dir, profile, platform) = setup_load(
        windows(),
        &[(
            "systemd-units.toml",
            "[base]\nunits = [{ name = \"example.service\", scop = \"user\" }]\n",
        )],
    );
    let result = Config::load(dir.path(), &profile, platform, None);
    assert!(
        result.is_err(),
        "platform-inactive config should still be parsed strictly"
    );
}

#[test]
fn load_rejects_overlay_target_replacement_instead_of_overwriting() {
    let (dir, profile, platform) = setup_load(
        linux(),
        &[("symlinks.toml", "[base]\nsymlinks = [\"bashrc\"]\n")],
    );
    let overlay = tempfile::tempdir().unwrap();
    write_overlay_config(
        &overlay,
        "symlinks.toml",
        "[base]\nsymlinks = [{ source = \"overlay-bashrc\", target = \".bashrc\" }]\n",
    );
    let error = Config::load(dir.path(), &profile, platform, Some(overlay.path())).unwrap_err();
    assert_eq!(error.to_string(), "validating symlink targets");
    assert_eq!(
        error.root_cause().to_string(),
        "symlink target collision for '.bashrc': 'bashrc' and 'overlay-bashrc' both map to the same target"
    );
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
