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
fn load_with_empty_config_files() {
    let (dir, profile, platform) = setup_load(linux(), &[]);
    let config = Config::load(dir.path(), &profile, platform, None).expect("load should succeed");
    assert!(config.packages.is_empty());
    assert!(config.symlinks.is_empty());
    assert!(config.validation_symlinks.is_empty());
    assert!(config.registry.is_empty());
    assert!(config.units.is_empty());
    assert!(config.chmod.is_empty());
    assert!(config.vscode_extensions.is_empty());
}

#[test]
fn load_populates_symlinks() {
    let (dir, profile, platform) = setup_load(
        linux(),
        &[(
            "symlinks.toml",
            "[base]\nsymlinks = [\".bashrc\", \".vimrc\"]\n",
        )],
    );
    let config = Config::load(dir.path(), &profile, platform, None).expect("load should succeed");
    assert_eq!(config.symlinks.len(), 2);
    assert_eq!(config.symlinks[0].source, ".bashrc");
    assert_eq!(config.symlinks[1].source, ".vimrc");
    assert_eq!(config.validation_symlinks.len(), 2);
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
fn load_populates_packages() {
    let (dir, profile, platform) = setup_load(
        linux(),
        &[("packages.toml", "[base]\npackages = [\"git\", \"curl\"]\n")],
    );
    let config = Config::load(dir.path(), &profile, platform, None).expect("load should succeed");
    assert_eq!(config.packages.len(), 2);
}

#[test]
fn load_stores_root_path() {
    let (dir, profile, platform) = setup_load(linux(), &[]);
    let config = Config::load(dir.path(), &profile, platform, None).expect("load should succeed");
    assert_eq!(config.root, dir.path());
}

#[test]
fn load_skips_registry_on_linux() {
    let (dir, profile, platform) = setup_load(
        linux(),
        &[(
            "registry.toml",
            "[test]\npath = \"HKCU:\\\\Test\"\n[test.values]\nKey = \"Value\"\n",
        )],
    );
    let config = Config::load(dir.path(), &profile, platform, None).expect("load should succeed");
    assert!(config.registry.is_empty(), "registry skipped on linux");
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

#[test]
fn load_rejects_missing_main_config_file() {
    let (dir, profile, platform) = setup_load(linux(), &[]);
    std::fs::remove_file(dir.path().join("conf").join("agent-settings.toml"))
        .expect("remove agent settings config");

    let error = Config::load(dir.path(), &profile, platform, None)
        .expect_err("missing main config should fail");

    assert!(error.to_string().contains("agent-settings.toml"));
}

#[test]
fn load_returns_error_on_invalid_packages_toml() {
    let (dir, profile, platform) = setup_load(linux(), &[("packages.toml", "[base\npackages = [")]);
    let result = Config::load(dir.path(), &profile, platform, None);
    assert!(result.is_err(), "invalid packages.toml should return error");
    let msg = result.unwrap_err().to_string();
    let expected_path = dir.path().join("conf").join("packages.toml");
    assert!(
        msg.contains(expected_path.to_str().unwrap_or("packages.toml")),
        "error should mention the full path: {msg}"
    );
}

#[test]
fn load_returns_error_on_invalid_git_config_toml() {
    let (dir, profile, platform) = setup_load(linux(), &[("git-config.toml", "not valid [[ toml")]);
    let result = Config::load(dir.path(), &profile, platform, None);
    assert!(
        result.is_err(),
        "invalid git-config.toml should return error"
    );
    let msg = result.unwrap_err().to_string();
    let expected_path = dir.path().join("conf").join("git-config.toml");
    assert!(
        msg.contains(expected_path.to_str().unwrap_or("git-config.toml")),
        "error should mention the full path: {msg}"
    );
}

#[test]
fn load_returns_error_on_type_mismatch_in_symlinks() {
    let (dir, profile, platform) = setup_load(
        linux(),
        &[("symlinks.toml", "[base]\nsymlinks = \"not-an-array\"\n")],
    );
    let result = Config::load(dir.path(), &profile, platform, None);
    assert!(
        result.is_err(),
        "type mismatch in symlinks.toml should return error"
    );
}
