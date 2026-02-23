#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::wildcard_imports,
    clippy::indexing_slicing
)]
//! Integration tests for the `test` command — config validation.
//!
//! These tests exercise `Config::load` and `Config::validate` using isolated
//! temporary repositories, verifying that:
//! - valid configuration produces no warnings
//! - missing symlink sources are detected
//! - AUR packages on non-Arch platforms generate warnings
//! - missing required config files are detected by the validation task

mod common;

use dotfiles_cli::config::Config;
use dotfiles_cli::config::profiles;
use dotfiles_cli::platform::{Os, Platform};

// ---------------------------------------------------------------------------
// Config loading
// ---------------------------------------------------------------------------

/// Loading config from a minimal valid repository must not return an error.
#[test]
fn config_loads_from_minimal_valid_repo() {
    let ctx = common::IntegrationTestContext::new();
    let config = ctx.load_config("base");
    // An empty config has no items in any category.
    assert!(config.symlinks.is_empty(), "expected no symlinks");
    assert!(config.packages.is_empty(), "expected no packages");
}

/// Config loading must also succeed for the desktop profile.
#[test]
fn config_loads_with_desktop_profile() {
    let ctx = common::IntegrationTestContext::new();
    let config = ctx.load_config("desktop");
    assert!(config.symlinks.is_empty(), "expected no symlinks");
}

/// Loading config with the desktop profile fixture yields symlinks from both
/// the `[base]` and `[desktop]` sections.
///
/// Uses the [`desktop_profile.ini`](fixtures/desktop_profile.ini) fixture with
/// both source files created on disk.
#[test]
fn config_loads_with_desktop_fixture() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("symlinks.ini", include_str!("fixtures/desktop_profile.ini"))
        .with_symlink_source("bashrc")
        .with_symlink_source("config/Code/User/settings.json")
        .build();

    let config = ctx.load_config("desktop");
    assert_eq!(
        config.symlinks.len(),
        2,
        "desktop fixture should yield 2 symlinks (base + desktop sections)"
    );
}

/// Config loading must succeed even when optional config files are absent.
#[test]
fn config_loads_with_missing_optional_files() {
    let root = tempfile::tempdir().expect("tempdir");
    let conf = root.path().join("conf");
    std::fs::create_dir_all(&conf).expect("create conf dir");
    std::fs::create_dir_all(root.path().join("symlinks")).expect("create symlinks dir");

    // Only required files; optional ones are intentionally absent.
    std::fs::write(
        conf.join("profiles.ini"),
        "[base]\ninclude=\nexclude=desktop\n",
    )
    .expect("write profiles.ini");

    let platform = Platform::detect();
    let profile = profiles::resolve("base", &conf, &platform).expect("resolve profile");
    let config = Config::load(root.path(), &profile, &platform).expect("load config");
    assert!(config.symlinks.is_empty());
    assert!(config.packages.is_empty());
}

// ---------------------------------------------------------------------------
// Validation: no warnings for valid config
// ---------------------------------------------------------------------------

/// A minimal valid config must produce zero validation warnings.
#[test]
fn config_validate_no_warnings_for_minimal_config() {
    let ctx = common::IntegrationTestContext::new();
    let config = ctx.load_config("base");
    let platform = Platform::detect();
    let warnings = config.validate(&platform);
    assert!(
        warnings.is_empty(),
        "empty config should produce no warnings, got: {warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Validation: missing symlink sources
// ---------------------------------------------------------------------------

/// A symlink entry pointing to a non-existent source file must produce a
/// validation warning from `symlinks.ini`.
///
/// Uses the [`base_profile.ini`](fixtures/base_profile.ini) fixture, whose
/// `bashrc` source is intentionally not created on disk.
#[test]
fn config_validate_warns_on_missing_symlink_source() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("symlinks.ini", include_str!("fixtures/base_profile.ini"))
        .build();

    let config = ctx.load_config("base");
    let platform = Platform::detect();
    let warnings = config.validate(&platform);

    assert!(
        !warnings.is_empty(),
        "missing symlink source should produce at least one validation warning"
    );
    assert!(
        warnings.iter().any(|w| w.source == "symlinks.ini"),
        "expected a warning from symlinks.ini, got: {warnings:?}"
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.message.contains("does not exist")),
        "warning message should mention 'does not exist'"
    );
}

/// A symlink entry whose source file *exists* must not produce a warning.
///
/// Uses the [`base_profile.ini`](fixtures/base_profile.ini) fixture with the
/// `bashrc` source file created on disk.
#[test]
fn config_validate_no_warning_when_symlink_source_exists() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("symlinks.ini", include_str!("fixtures/base_profile.ini"))
        .with_symlink_source("bashrc")
        .build();

    let config = ctx.load_config("base");
    let platform = Platform::detect();
    let warnings = config.validate(&platform);

    let symlink_warnings: Vec<_> = warnings
        .iter()
        .filter(|w| w.source == "symlinks.ini")
        .collect();
    assert!(
        symlink_warnings.is_empty(),
        "existing symlink source should produce no warnings, got: {symlink_warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Validation: warning detection for platform-specific config
// ---------------------------------------------------------------------------

/// A VS Code extension ID that does not contain a dot (`publisher.name`)
/// must produce a validation warning.
#[test]
fn config_validate_warns_on_invalid_vscode_extension_id() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("vscode-extensions.ini", "[base]\ninvalid_no_dot\n")
        .build();

    let config = ctx.load_config("base");
    let platform = Platform::detect();
    let warnings = config.validate(&platform);

    assert!(
        warnings.iter().any(|w| w.source == "vscode-extensions.ini"),
        "expected a vscode-extensions.ini warning, got: {warnings:?}"
    );
}

/// A Copilot skill URL that does not start with `http://` or `https://` must
/// produce a validation warning.
#[test]
fn config_validate_warns_on_invalid_copilot_skill_url() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("copilot-skills.ini", "[base]\nnot-a-url\n")
        .build();

    let config = ctx.load_config("base");
    let platform = Platform::detect();
    let warnings = config.validate(&platform);

    assert!(
        warnings.iter().any(|w| w.source == "copilot-skills.ini"),
        "expected a copilot-skills.ini warning, got: {warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Profile resolution
// ---------------------------------------------------------------------------

/// Both `base` and `desktop` profiles must resolve successfully from the
/// minimal `profiles.ini` written by `setup_minimal_repo`.
#[test]
fn both_profiles_resolve_from_minimal_repo() {
    let ctx = common::IntegrationTestContext::new();
    let conf_dir = ctx.root_path().join("conf");
    let platform = Platform::detect();

    let base = profiles::resolve("base", &conf_dir, &platform);
    let desktop = profiles::resolve("desktop", &conf_dir, &platform);

    assert!(base.is_ok(), "base profile should resolve");
    assert!(desktop.is_ok(), "desktop profile should resolve");
}

/// Requesting a non-existent profile must return an error.
#[test]
fn unknown_profile_returns_error() {
    let ctx = common::IntegrationTestContext::new();
    let conf_dir = ctx.root_path().join("conf");
    let platform = Platform::detect();

    let result = profiles::resolve("nonexistent", &conf_dir, &platform);
    assert!(
        result.is_err(),
        "resolving an unknown profile should return an error"
    );
}

// ---------------------------------------------------------------------------
// Config loading: packages
// ---------------------------------------------------------------------------

/// Packages listed in packages.ini must be loaded into `config.packages`.
#[test]
fn config_loads_packages_from_ini() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("packages.ini", "[base]\ngit\ncurl\n")
        .build();

    let platform = Platform {
        os: Os::Linux,
        is_arch: false,
    };
    let config = ctx.load_config_for_platform("base", &platform);
    assert_eq!(
        config.packages.len(),
        2,
        "expected 2 packages, got {}",
        config.packages.len()
    );
    assert_eq!(config.packages[0].name, "git");
    assert_eq!(config.packages[1].name, "curl");
    assert!(!config.packages[0].is_aur);
    assert!(!config.packages[1].is_aur);
}

/// Packages prefixed with `aur:` in packages.ini must be loaded with
/// `is_aur = true` and the prefix stripped from the name.
#[test]
fn config_loads_aur_packages_correctly() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("packages.ini", "[base]\ngit\naur:paru-bin\n")
        .build();

    let platform = Platform {
        os: Os::Linux,
        is_arch: true,
    };
    let config = ctx.load_config_for_platform("base", &platform);
    assert_eq!(config.packages.len(), 2);

    let aur_pkg = config
        .packages
        .iter()
        .find(|p| p.is_aur)
        .expect("aur package");
    assert_eq!(aur_pkg.name, "paru-bin", "aur: prefix should be stripped");

    let regular_pkg = config
        .packages
        .iter()
        .find(|p| !p.is_aur)
        .expect("regular package");
    assert_eq!(regular_pkg.name, "git");
}

// ---------------------------------------------------------------------------
// Validation: AUR packages on non-Arch platforms
// ---------------------------------------------------------------------------

/// An AUR package must produce a validation warning on a non-Arch Linux platform.
#[test]
fn config_validate_warns_on_aur_packages_on_non_arch() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("packages.ini", "[base]\naur:paru-bin\n")
        .build();

    let platform = Platform {
        os: Os::Linux,
        is_arch: false,
    };
    let config = ctx.load_config_for_platform("base", &platform);
    let warnings = config.validate(&platform);

    assert!(
        warnings.iter().any(|w| w.source == "packages.ini"),
        "expected a packages.ini warning for AUR on non-Arch, got: {warnings:?}"
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.message.contains("not Arch Linux")),
        "warning message should mention 'not Arch Linux'"
    );
}

/// An AUR package must NOT produce a validation warning on an Arch Linux platform.
#[test]
fn config_validate_no_warning_for_aur_packages_on_arch() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("packages.ini", "[base]\naur:paru-bin\n")
        .build();

    let platform = Platform {
        os: Os::Linux,
        is_arch: true,
    };
    let config = ctx.load_config_for_platform("base", &platform);
    let warnings = config.validate(&platform);

    let pkg_warnings: Vec<_> = warnings
        .iter()
        .filter(|w| w.source == "packages.ini" && w.message.contains("not Arch Linux"))
        .collect();
    assert!(
        pkg_warnings.is_empty(),
        "AUR packages on Arch should not produce warnings, got: {pkg_warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Validation: chmod entries
// ---------------------------------------------------------------------------

/// An invalid octal mode in chmod.ini must produce a validation warning.
#[test]
fn config_validate_warns_on_invalid_chmod_mode() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("chmod.ini", "[base]\n999 .ssh/config\n")
        .build();

    let platform = Platform {
        os: Os::Linux,
        is_arch: false,
    };
    let config = ctx.load_config_for_platform("base", &platform);
    let warnings = config.validate(&platform);

    assert!(
        warnings.iter().any(|w| w.source == "chmod.ini"),
        "expected a chmod.ini warning for invalid mode, got: {warnings:?}"
    );
}

/// A chmod entry with an absolute path must produce a validation warning.
#[test]
fn config_validate_warns_on_absolute_chmod_path() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("chmod.ini", "[base]\n644 /etc/something\n")
        .build();

    let platform = Platform {
        os: Os::Linux,
        is_arch: false,
    };
    let config = ctx.load_config_for_platform("base", &platform);
    let warnings = config.validate(&platform);

    assert!(
        warnings
            .iter()
            .any(|w| w.source == "chmod.ini" && w.message.contains("relative")),
        "expected a chmod.ini warning about absolute path, got: {warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Validation: systemd units
// ---------------------------------------------------------------------------

/// A systemd unit name without a valid extension must produce a validation warning.
#[test]
fn config_validate_warns_on_invalid_systemd_unit_extension() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("systemd-units.ini", "[base]\nmyunit\n")
        .build();

    let platform = Platform {
        os: Os::Linux,
        is_arch: false,
    };
    let config = ctx.load_config_for_platform("base", &platform);
    let warnings = config.validate(&platform);

    assert!(
        warnings.iter().any(|w| w.source == "systemd-units.ini"),
        "expected a systemd-units.ini warning for invalid extension, got: {warnings:?}"
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.message.contains("valid systemd extension")),
        "warning should mention 'valid systemd extension'"
    );
}

/// A valid systemd unit name must not produce a warning.
#[test]
fn config_validate_no_warning_for_valid_systemd_unit() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("systemd-units.ini", "[base]\ndunst.service\n")
        .build();

    let platform = Platform {
        os: Os::Linux,
        is_arch: false,
    };
    let config = ctx.load_config_for_platform("base", &platform);
    let warnings = config.validate(&platform);

    let unit_warnings: Vec<_> = warnings
        .iter()
        .filter(|w| w.source == "systemd-units.ini")
        .collect();
    assert!(
        unit_warnings.is_empty(),
        "valid unit name should produce no warnings, got: {unit_warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Validation: symlink absolute path
// ---------------------------------------------------------------------------

/// A symlink entry with an absolute source path must produce a validation warning.
#[test]
fn config_validate_warns_on_absolute_symlink_source() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("symlinks.ini", "[base]\n/absolute/path/to/file\n")
        .build();

    let config = ctx.load_config("base");
    let platform = Platform::detect();
    let warnings = config.validate(&platform);

    assert!(
        warnings
            .iter()
            .any(|w| w.source == "symlinks.ini" && w.message.contains("should be relative")),
        "expected a symlinks.ini warning for absolute path, got: {warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Config loading: vscode extensions, copilot skills, and chmod
// ---------------------------------------------------------------------------

/// VS Code extensions listed in vscode-extensions.ini must be loaded into
/// `config.vscode_extensions`.
#[test]
fn config_loads_vscode_extensions_correctly() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "vscode-extensions.ini",
            "[base]\nms-vscode.cpptools\nrust-lang.rust-analyzer\n",
        )
        .build();

    let config = ctx.load_config("base");
    assert_eq!(
        config.vscode_extensions.len(),
        2,
        "expected 2 VS Code extensions, got {}",
        config.vscode_extensions.len()
    );
    assert_eq!(config.vscode_extensions[0].id, "ms-vscode.cpptools");
    assert_eq!(config.vscode_extensions[1].id, "rust-lang.rust-analyzer");
}

/// Copilot skill URLs listed in copilot-skills.ini must be loaded into
/// `config.copilot_skills`.
#[test]
fn config_loads_copilot_skills_correctly() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "copilot-skills.ini",
            "[base]\nhttps://github.com/example/skill-a\nhttps://github.com/example/skill-b\n",
        )
        .build();

    let config = ctx.load_config("base");
    assert_eq!(
        config.copilot_skills.len(),
        2,
        "expected 2 Copilot skills, got {}",
        config.copilot_skills.len()
    );
    assert_eq!(
        config.copilot_skills[0].url,
        "https://github.com/example/skill-a"
    );
    assert_eq!(
        config.copilot_skills[1].url,
        "https://github.com/example/skill-b"
    );
}

/// Chmod entries listed in chmod.ini must be loaded into `config.chmod`.
#[test]
fn config_loads_chmod_entries_correctly() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("chmod.ini", "[base]\n600 .ssh/config\n700 .ssh\n")
        .build();

    let platform = Platform {
        os: Os::Linux,
        is_arch: false,
    };
    let config = ctx.load_config_for_platform("base", &platform);
    assert_eq!(
        config.chmod.len(),
        2,
        "expected 2 chmod entries, got {}",
        config.chmod.len()
    );
    assert_eq!(config.chmod[0].mode, "600");
    assert_eq!(config.chmod[0].path, ".ssh/config");
    assert_eq!(config.chmod[1].mode, "700");
    assert_eq!(config.chmod[1].path, ".ssh");
}

// ---------------------------------------------------------------------------
// Config loading: registry entries (Windows-only)
// ---------------------------------------------------------------------------

/// Registry entries in registry.ini must be loaded into `config.registry`
/// when the platform is Windows.
#[test]
fn config_loads_registry_entries_on_windows() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("registry.ini", "[HKCU:\\Console]\nFontSize = 14\n")
        .build();

    let platform = Platform {
        os: Os::Windows,
        is_arch: false,
    };
    let config = ctx.load_config_for_platform("base", &platform);
    assert_eq!(
        config.registry.len(),
        1,
        "expected 1 registry entry on Windows, got {}",
        config.registry.len()
    );
    assert_eq!(config.registry[0].key_path, "HKCU:\\Console");
    assert_eq!(config.registry[0].value_name, "FontSize");
    assert_eq!(config.registry[0].value_data, "14");
}

/// Registry entries in registry.ini must be skipped when the platform is Linux.
#[test]
fn config_does_not_load_registry_on_linux() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("registry.ini", "[HKCU:\\Console]\nFontSize = 14\n")
        .build();

    let platform = Platform {
        os: Os::Linux,
        is_arch: false,
    };
    let config = ctx.load_config_for_platform("base", &platform);
    assert!(
        config.registry.is_empty(),
        "expected no registry entries on Linux"
    );
}

// ---------------------------------------------------------------------------
// Validation: registry entries
// ---------------------------------------------------------------------------

/// A valid HKCU registry entry must not produce a validation warning on Windows.
#[test]
fn config_validate_no_warning_for_valid_registry_on_windows() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("registry.ini", "[HKCU:\\Console]\nFontSize = 14\n")
        .build();

    let platform = Platform {
        os: Os::Windows,
        is_arch: false,
    };
    let config = ctx.load_config_for_platform("base", &platform);
    let warnings = config.validate(&platform);

    let registry_warnings: Vec<_> = warnings
        .iter()
        .filter(|w| w.source == "registry.ini")
        .collect();
    assert!(
        registry_warnings.is_empty(),
        "valid HKCU registry entry should produce no warnings, got: {registry_warnings:?}"
    );
}

/// A registry entry using a non-HKCU hive must produce a validation warning on Windows.
#[test]
fn config_validate_warns_on_non_hkcu_registry_hive() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("registry.ini", "[HKLM:\\Software\\Test]\nSetting = value\n")
        .build();

    let platform = Platform {
        os: Os::Windows,
        is_arch: false,
    };
    let config = ctx.load_config_for_platform("base", &platform);
    let warnings = config.validate(&platform);

    assert!(
        warnings
            .iter()
            .any(|w| w.source == "registry.ini" && w.message.contains("non-HKCU")),
        "expected a registry.ini warning for non-HKCU hive, got: {warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Validation: chmod entries on non-Unix platforms
// ---------------------------------------------------------------------------

/// Chmod entries must produce a validation warning on Windows because the
/// platform does not support POSIX file permissions.
#[test]
fn config_validate_warns_on_chmod_entries_on_windows() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("chmod.ini", "[base]\n600 .ssh/config\n")
        .build();

    let platform = Platform {
        os: Os::Windows,
        is_arch: false,
    };
    let config = ctx.load_config_for_platform("base", &platform);
    let warnings = config.validate(&platform);

    assert!(
        warnings
            .iter()
            .any(|w| w.source == "chmod.ini" && w.message.contains("does not support chmod")),
        "expected a chmod.ini warning for Windows platform, got: {warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Validation: multiple sources accumulate warnings
// ---------------------------------------------------------------------------

/// Validation warnings from multiple config files must all be returned.
#[test]
fn config_validate_collects_warnings_from_multiple_sources() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("vscode-extensions.ini", "[base]\ninvalid_no_dot\n")
        .with_config_file("copilot-skills.ini", "[base]\nnot-a-url\n")
        .build();

    let config = ctx.load_config("base");
    let platform = Platform::detect();
    let warnings = config.validate(&platform);

    let sources: std::collections::HashSet<&str> =
        warnings.iter().map(|w| w.source.as_str()).collect();
    assert!(
        sources.contains("vscode-extensions.ini"),
        "expected a vscode-extensions.ini warning"
    );
    assert!(
        sources.contains("copilot-skills.ini"),
        "expected a copilot-skills.ini warning"
    );
}
