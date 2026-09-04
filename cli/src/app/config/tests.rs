//! Unit tests for configuration loading and validation.

use super::*;
use crate::infra::config::category_matcher::Category;
use crate::infra::platform::{Os, Platform};

#[test]
fn section_inventory_reports_every_user_configured_slice() {
    let config = crate::test_helpers::empty_config(PathBuf::from("/tmp"));
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
fn load_rejects_all_conflicting_main_and_overlay_values_before_publishing_config() {
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
    let overlay = tempfile::tempdir().unwrap();
    let git_path = write_overlay_config(
        &overlay,
        "git-config.toml",
        "[base]\nsettings = [{ key = \"CORE.EDITOR\", value = \"nano\" }]\n",
    );
    let registry_path = write_overlay_config(
        &overlay,
        "registry.toml",
        "[display]\npath = 'hkcu:\\console'\n[display.values]\nfontsize = 15\n",
    );
    let error = Config::load(dir.path(), &profile, platform, Some(overlay.path()))
        .expect_err("conflicts must fail before a configuration snapshot is published");
    let message = format!("{error:#}");
    for expected in [
        "git.conflicting-values",
        "registry.conflicting-values",
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

#[test]
fn load_accepts_equivalent_main_and_overlay_values() {
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
    let overlay = tempfile::tempdir().unwrap();
    write_overlay_config(
        &overlay,
        "git-config.toml",
        "[base]\nsettings = [{ key = \"CORE.EDITOR\", value = \"vim\" }]\n",
    );
    write_overlay_config(
        &overlay,
        "registry.toml",
        "[display]\npath = 'hkcu:\\console'\n[display.values]\nfontsize = '0x0E'\n",
    );
    let config = Config::load(dir.path(), &profile, platform, Some(overlay.path()))
        .expect("equivalent declarations should remain valid");
    assert_eq!(config.git_settings.len(), 2, "preserve append semantics");
    assert_eq!(config.registry.len(), 2, "preserve append semantics");
    assert!(git_config::validate_conflicts(&config.git_settings).is_empty());
    assert!(registry::validate_conflicts(&config.registry).is_empty());
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
fn load_keeps_profile_excluded_main_symlinks_for_validation() {
    let (dir, profile, platform) = setup_load(
        linux(),
        &[(
            "symlinks.toml",
            "[base]\nsymlinks = [\"bashrc\"]\n[desktop]\nsymlinks = [\"config/i3\"]\n",
        )],
    );

    let config = Config::load(dir.path(), &profile, platform, None).expect("load should succeed");

    assert_eq!(
        config
            .symlinks
            .iter()
            .map(|symlink| symlink.source.as_str())
            .collect::<Vec<_>>(),
        vec!["bashrc"]
    );
    assert_eq!(
        config
            .validation_symlinks
            .iter()
            .map(|symlink| symlink.source.as_str())
            .collect::<Vec<_>>(),
        vec!["bashrc", "config/i3"]
    );
    assert!(
        config
            .validation_symlinks
            .iter()
            .all(|symlink| symlink.origin.as_deref() == Some(dir.path()))
    );
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
}

#[test]
fn load_appends_overlay_packages_and_scripts() {
    let (dir, profile, platform) = setup_load(
        linux(),
        &[("packages.toml", "[base]\npackages = [\"git\"]\n")],
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
fn load_reports_overlay_path_for_overlay_syntax_errors() {
    let (dir, profile, platform) = setup_load(linux(), &[]);
    let overlay = tempfile::tempdir().expect("create overlay dir");
    let invalid_path = write_overlay_config(&overlay, "scripts.toml", "[base\nscripts = [");

    let result = Config::load(dir.path(), &profile, platform, Some(overlay.path()));

    assert!(result.is_err(), "invalid overlay config should fail");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("overlay"),
        "error should identify overlay config source: {msg}"
    );
    assert!(
        msg.contains(invalid_path.to_str().unwrap_or("scripts.toml")),
        "error should include overlay config path: {msg}"
    );
}

#[test]
fn load_stores_root_path() {
    let (dir, profile, platform) = setup_load(linux(), &[]);
    let config = Config::load(dir.path(), &profile, platform, None).expect("load should succeed");
    assert_eq!(config.root, dir.path());
}

#[test]
fn load_populates_systemd_units_on_linux() {
    let (dir, profile, platform) = setup_load(
        linux(),
        &[("systemd-units.toml", "[base]\nunits = [\"ssh.service\"]\n")],
    );
    let config = Config::load(dir.path(), &profile, platform, None).expect("load should succeed");
    assert_eq!(config.units.len(), 1);
}

#[test]
fn load_skips_systemd_units_on_windows() {
    let (dir, profile, platform) = setup_load(windows(), &[]);
    let config = Config::load(dir.path(), &profile, platform, None).expect("load should succeed");
    assert!(config.units.is_empty(), "systemd units skipped on windows");
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
