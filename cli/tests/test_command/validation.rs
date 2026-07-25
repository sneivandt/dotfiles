//! Config validation warnings surfaced by the `test` command.

use dotfiles_cli::testing as test_api;
use test_api::platform::{Os, Platform};

use crate::common;

// ---------------------------------------------------------------------------
// Validation: no warnings for valid config
// ---------------------------------------------------------------------------

/// A minimal valid config must produce zero validation warnings.
#[test]
fn config_validate_no_warnings_for_minimal_config() {
    let ctx = common::IntegrationTestContext::new();
    let config = ctx.load_config("base");
    let platform = Platform::detect();
    let warnings = config.validate(platform);
    assert!(
        warnings.is_empty(),
        "empty config should produce no warnings, got: {warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Validation: missing symlink sources
// ---------------------------------------------------------------------------

/// A symlink entry pointing to a non-existent source file must produce a
/// validation warning from `symlinks.toml`.
///
/// Uses the [`base_profile.toml`](fixtures/base_profile.toml) fixture, whose
/// `bashrc` source is intentionally not created on disk.
#[test]
fn config_validate_warns_on_missing_symlink_source() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "symlinks.toml",
            include_str!("../fixtures/base_profile.toml"),
        )
        .build();

    let config = ctx.load_config("base");
    let platform = Platform::detect();
    let warnings = config.validate(platform);

    assert!(
        !warnings.is_empty(),
        "missing symlink source should produce at least one validation warning"
    );
    assert!(
        warnings.iter().any(|w| w.source == "symlinks.toml"),
        "expected a warning from symlinks.toml, got: {warnings:?}"
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
/// Uses the [`base_profile.toml`](fixtures/base_profile.toml) fixture with the
/// `bashrc` source file created on disk.
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
    let platform = Platform::detect();
    let warnings = config.validate(platform);

    let symlink_warnings: Vec<_> = warnings
        .iter()
        .filter(|w| w.source == "symlinks.toml")
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
        .with_config_file(
            "vscode-extensions.toml",
            "[base]\nextensions = [\"invalid_no_dot\"]\n",
        )
        .build();

    let config = ctx.load_config("base");
    let platform = Platform::detect();
    let warnings = config.validate(platform);

    assert!(
        warnings
            .iter()
            .any(|w| w.source == "vscode-extensions.toml"),
        "expected a vscode-extensions.toml warning, got: {warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Validation: AUR packages on non-Arch platforms
// ---------------------------------------------------------------------------

/// An AUR package must produce a validation warning on a non-Arch Linux platform.
#[test]
fn config_validate_warns_on_aur_packages_on_non_arch() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "packages.toml",
            "[base]\npackages = [{ name = \"paru-bin\", aur = true }]\n",
        )
        .build();

    let platform = Platform {
        os: Os::Linux,
        is_arch: false,
        is_wsl: false,
    };
    let config = ctx.load_config_for_platform("base", platform);
    let warnings = config.validate(platform);

    assert!(
        warnings.iter().any(|w| w.source == "packages.toml"),
        "expected a packages.toml warning for AUR on non-Arch, got: {warnings:?}"
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
    let config = ctx.load_config_for_platform("base", platform);
    let warnings = config.validate(platform);

    let pkg_warnings: Vec<_> = warnings
        .iter()
        .filter(|w| w.source == "packages.toml" && w.message.contains("not Arch Linux"))
        .collect();
    assert!(
        pkg_warnings.is_empty(),
        "AUR packages on Arch should not produce warnings, got: {pkg_warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Validation: chmod entries
// ---------------------------------------------------------------------------

/// An invalid octal mode in chmod.toml must produce a validation warning.
#[test]
fn config_validate_warns_on_invalid_chmod_mode() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "chmod.toml",
            "[base]\npermissions = [{ mode = \"999\", path = \".ssh/config\" }]\n",
        )
        .build();

    let platform = Platform {
        os: Os::Linux,
        is_arch: false,
        is_wsl: false,
    };
    let config = ctx.load_config_for_platform("base", platform);
    let warnings = config.validate(platform);

    assert!(
        warnings.iter().any(|w| w.source == "chmod.toml"),
        "expected a chmod.toml warning for invalid mode, got: {warnings:?}"
    );
}

/// A chmod entry with an absolute path must produce a validation warning.
#[test]
fn config_validate_warns_on_absolute_chmod_path() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "chmod.toml",
            "[base]\npermissions = [{ mode = \"644\", path = \"/etc/something\" }]\n",
        )
        .build();

    let platform = Platform {
        os: Os::Linux,
        is_arch: false,
        is_wsl: false,
    };
    let config = ctx.load_config_for_platform("base", platform);
    let warnings = config.validate(platform);

    assert!(
        warnings
            .iter()
            .any(|w| w.source == "chmod.toml" && w.message.contains("relative")),
        "expected a chmod.toml warning about absolute path, got: {warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Validation: systemd units
// ---------------------------------------------------------------------------

/// A systemd unit name without a valid extension must produce a validation warning.
#[test]
fn config_validate_warns_on_invalid_systemd_unit_extension() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("systemd-units.toml", "[base]\nunits = [\"myunit\"]\n")
        .build();

    let platform = Platform {
        os: Os::Linux,
        is_arch: false,
        is_wsl: false,
    };
    let config = ctx.load_config_for_platform("base", platform);
    let warnings = config.validate(platform);

    assert!(
        warnings.iter().any(|w| w.source == "systemd-units.toml"),
        "expected a systemd-units.toml warning for invalid extension, got: {warnings:?}"
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
        .with_config_file(
            "systemd-units.toml",
            "[base]\nunits = [\"dunst.service\"]\n",
        )
        .build();

    let platform = Platform {
        os: Os::Linux,
        is_arch: false,
        is_wsl: false,
    };
    let config = ctx.load_config_for_platform("base", platform);
    let warnings = config.validate(platform);

    let unit_warnings: Vec<_> = warnings
        .iter()
        .filter(|w| w.source == "systemd-units.toml")
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
        .with_config_file(
            "symlinks.toml",
            "[base]\nsymlinks = [\"/absolute/path/to/file\"]\n",
        )
        .build();

    let config = ctx.load_config("base");
    let platform = Platform::detect();
    let warnings = config.validate(platform);

    assert!(
        warnings
            .iter()
            .any(|w| w.source == "symlinks.toml" && w.message.contains("should be relative")),
        "expected a symlinks.toml warning for absolute path, got: {warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Validation: registry entries
// ---------------------------------------------------------------------------

/// A valid HKCU registry entry must not produce a validation warning on Windows.
#[test]
fn config_validate_no_warning_for_valid_registry_on_windows() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "registry.toml",
            "[console]\npath = 'HKCU:\\Console'\n[console.values]\nFontSize = 14\n",
        )
        .build();

    let platform = Platform {
        os: Os::Windows,
        is_arch: false,
        is_wsl: false,
    };
    let config = ctx.load_config_for_platform("base", platform);
    let warnings = config.validate(platform);

    let registry_warnings: Vec<_> = warnings
        .iter()
        .filter(|w| w.source == "registry.toml")
        .collect();
    assert!(
        registry_warnings.is_empty(),
        "valid HKCU registry entry should produce no warnings, got: {registry_warnings:?}"
    );
}

/// A registry entry outside HKCU must be rejected by validation on Windows.
#[test]
fn config_validate_rejects_non_hkcu_registry_hive() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "registry.toml",
            "[test]\npath = 'HKLM:\\Software\\Test'\n[test.values]\nSetting = \"value\"\n",
        )
        .build();

    let platform = Platform {
        os: Os::Windows,
        is_arch: false,
        is_wsl: false,
    };
    let config = ctx.load_config_for_platform("base", platform);
    let warnings = config.validate(platform);

    assert!(
        warnings.iter().any(|w| w.source == "registry.toml"
            && w.message.contains("other registry hives are not supported")),
        "expected registry.toml to reject the non-HKCU hive, got: {warnings:?}"
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
        .with_config_file(
            "chmod.toml",
            "[base]\npermissions = [{ mode = \"600\", path = \".ssh/config\" }]\n",
        )
        .build();

    let platform = Platform {
        os: Os::Windows,
        is_arch: false,
        is_wsl: false,
    };
    let config = ctx.load_config_for_platform("base", platform);
    let warnings = config.validate(platform);

    assert!(
        warnings
            .iter()
            .any(|w| w.source == "chmod.toml" && w.message.contains("does not support chmod")),
        "expected a chmod.toml warning for Windows platform, got: {warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Validation: multiple sources accumulate warnings
// ---------------------------------------------------------------------------

/// Validation warnings from multiple config files must all be returned.
#[test]
fn config_validate_collects_warnings_from_multiple_sources() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "vscode-extensions.toml",
            "[base]\nextensions = [\"invalid_no_dot\"]\n",
        )
        .build();

    let config = ctx.load_config("base");
    let platform = Platform::detect();
    let warnings = config.validate(platform);

    let sources: std::collections::HashSet<&str> =
        warnings.iter().map(|w| w.source.as_str()).collect();
    assert!(
        sources.contains("vscode-extensions.toml"),
        "expected a vscode-extensions.toml warning"
    );
}

// ---------------------------------------------------------------------------
// Validation: empty config entries
// ---------------------------------------------------------------------------

/// Validation should warn about empty values in various config files.
#[test]
fn config_validate_warns_on_empty_package_name() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "packages.toml",
            "[base]\npackages = [{ name = \"  \", aur = false }]\n",
        )
        .build();

    let config = ctx.load_config("base");
    let platform = Platform::detect();
    let warnings = config.validate(platform);

    assert!(
        warnings
            .iter()
            .any(|w| w.source == "packages.toml" && w.message.contains("empty")),
        "expected a packages.toml warning for empty name, got: {warnings:?}"
    );
}

/// Validation should warn about empty git config keys.
#[test]
fn config_validate_warns_on_empty_git_config_key() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "git-config.toml",
            "[base]\nsettings = [{ key = \"  \", value = \"val\" }]\n",
        )
        .build();

    let config = ctx.load_config("base");
    let warnings = config.validate(Platform::detect());

    assert!(
        warnings
            .iter()
            .any(|w| w.source == "git-config.toml" && w.message.contains("key is empty")),
        "expected a git-config.toml warning for empty key, got: {warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Validation: systemd on non-linux
// ---------------------------------------------------------------------------

/// Systemd units defined on a Windows platform should produce a warning.
#[test]
fn config_validate_warns_on_systemd_units_on_windows() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("systemd-units.toml", "[base]\nunits = [\"test.service\"]\n")
        .build();

    // Load on Linux so units are actually parsed (Config::load skips them on Windows).
    let linux = Platform {
        os: Os::Linux,
        is_arch: false,
        is_wsl: false,
    };
    let config = ctx.load_config_for_platform("base", linux);

    // Validate against Windows to trigger the platform-mismatch warning.
    let windows = Platform {
        os: Os::Windows,
        is_arch: false,
        is_wsl: false,
    };
    let warnings = config.validate(windows);

    assert!(
        warnings
            .iter()
            .any(|w| w.source == "systemd-units.toml"
                && w.message.contains("does not support systemd")),
        "expected a systemd-units.toml warning on Windows, got: {warnings:?}"
    );
}
