#![allow(clippy::expect_used, clippy::unwrap_used, clippy::wildcard_imports)]
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
use dotfiles_cli::platform::Platform;

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
