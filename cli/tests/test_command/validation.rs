//! Config validation aggregation surfaced by the `test` command.

use dotfiles_cli::testing as test_api;
use test_api::platform::{Os, Platform};

use crate::common;

const fn linux() -> Platform {
    Platform {
        os: Os::Linux,
        is_arch: false,
        is_wsl: false,
    }
}

const fn windows() -> Platform {
    Platform {
        os: Os::Windows,
        is_arch: false,
        is_wsl: false,
    }
}

#[test]
fn config_validate_no_warnings_for_minimal_config() {
    let ctx = common::IntegrationTestContext::new();
    let config = ctx.load_config("base");
    let warnings = config.validate(Platform::detect());

    assert!(
        warnings.is_empty(),
        "empty config should produce no warnings, got: {warnings:?}"
    );
}

#[test]
fn config_validate_no_warning_when_symlink_source_exists() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "symlinks.toml",
            include_str!("../fixtures/base_profile.toml"),
        )
        .with_symlink_source("bashrc")
        .build();

    let config = ctx.load_config("base");
    let warnings = config.validate(Platform::detect());

    assert!(
        warnings
            .iter()
            .all(|warning| warning.source != "symlinks.toml"),
        "existing symlink source should produce no warnings, got: {warnings:?}"
    );
}

#[test]
fn config_validate_no_warning_for_aur_packages_on_arch() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "packages.toml",
            "[base]\npackages = [{ name = \"paru-bin\", aur = true }]\n",
        )
        .build();
    let platform = Platform {
        os: Os::Linux,
        is_arch: true,
        is_wsl: false,
    };

    let warnings = ctx
        .load_config_for_platform("base", platform)
        .validate(platform);

    assert!(
        warnings.iter().all(|warning| {
            warning.source != "packages.toml" || !warning.message.contains("not Arch Linux")
        }),
        "AUR packages on Arch should not produce warnings, got: {warnings:?}"
    );
}

#[test]
fn config_validate_no_warning_for_valid_systemd_unit() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "systemd-units.toml",
            "[base]\nunits = [\"dunst.service\"]\n",
        )
        .build();
    let platform = linux();

    let warnings = ctx
        .load_config_for_platform("base", platform)
        .validate(platform);

    assert!(
        warnings
            .iter()
            .all(|warning| warning.source != "systemd-units.toml"),
        "valid unit name should produce no warnings, got: {warnings:?}"
    );
}

#[test]
fn config_validate_warns_on_absolute_chmod_path() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "chmod.toml",
            "[base]\npermissions = [{ mode = \"644\", path = \"/etc/something\" }]\n",
        )
        .build();
    let platform = linux();

    let warnings = ctx
        .load_config_for_platform("base", platform)
        .validate(platform);

    assert!(
        warnings.iter().any(|warning| {
            warning.source == "chmod.toml" && warning.message.contains("relative")
        }),
        "expected a chmod.toml warning about absolute path, got: {warnings:?}"
    );
}

#[test]
fn config_validate_warns_on_chmod_entries_on_windows() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "chmod.toml",
            "[base]\npermissions = [{ mode = \"600\", path = \".ssh/config\" }]\n",
        )
        .build();
    let platform = windows();

    let warnings = ctx
        .load_config_for_platform("base", platform)
        .validate(platform);

    assert!(
        warnings.iter().any(|warning| {
            warning.source == "chmod.toml" && warning.message.contains("does not support chmod")
        }),
        "expected a chmod.toml warning on Windows, got: {warnings:?}"
    );
}

#[test]
fn config_validate_collects_diagnostics_from_each_config_domain() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", "[base]\nsymlinks = [\"missing\"]\n")
        .with_config_file(
            "packages.toml",
            "[base]\npackages = [{ name = \"  \", aur = false }]\n",
        )
        .with_config_file(
            "chmod.toml",
            "[base]\npermissions = [{ mode = \"999\", path = \".ssh/config\" }]\n",
        )
        .with_config_file("systemd-units.toml", "[base]\nunits = [\"invalid\"]\n")
        .with_config_file(
            "vscode-extensions.toml",
            "[base]\nextensions = [\"invalid_no_dot\"]\n",
        )
        .with_config_file(
            "git-config.toml",
            "[base]\nsettings = [{ key = \"  \", value = \"value\" }]\n",
        )
        .with_config_file(
            "agent-settings.toml",
            "[base]\nsettings = [{ target = \"copilot\", key = \"  \", value = true }]\n",
        )
        .build();
    let platform = linux();
    let warnings = ctx
        .load_config_for_platform("base", platform)
        .validate(platform);
    let sources: std::collections::HashSet<&str> = warnings
        .iter()
        .map(|warning| warning.source.as_str())
        .collect();

    for source in [
        "symlinks.toml",
        "packages.toml",
        "chmod.toml",
        "systemd-units.toml",
        "vscode-extensions.toml",
        "git-config.toml",
        "agent-settings.toml",
    ] {
        assert!(
            sources.contains(source),
            "expected a diagnostic from {source}, got: {warnings:?}"
        );
    }
}
