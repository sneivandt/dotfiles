//! Config loading, profile resolution, and parse-error reporting.

use dotfiles_cli::testing as test_api;
use test_api::config::Config;
use test_api::config::profiles;
use test_api::platform::{Os, Platform};

use crate::common;

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
/// Uses the [`desktop_profile.toml`](fixtures/desktop_profile.toml) fixture with
/// both source files created on disk.
#[test]
fn config_loads_with_desktop_fixture() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "symlinks.toml",
            include_str!("../fixtures/desktop_profile.toml"),
        )
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

/// Config loading must reject a missing main configuration file.
#[test]
fn config_rejects_missing_main_config_file() {
    let ctx = common::IntegrationTestContext::new();
    let conf = ctx.root_path().join("conf");
    std::fs::remove_file(conf.join("agent-settings.toml")).expect("remove agent-settings.toml");

    let platform = Platform::detect();
    let profile = profiles::resolve("base", &conf, platform).expect("resolve profile");
    let error = Config::load(ctx.root_path(), &profile, platform, None)
        .expect_err("missing config should fail");
    assert!(error.to_string().contains("agent-settings.toml"));
}

// ---------------------------------------------------------------------------
// Profile resolution
// ---------------------------------------------------------------------------

/// Both `base` and `desktop` profiles must resolve successfully from the
/// minimal `profiles.toml` written by `setup_minimal_repo`.
#[test]
fn both_profiles_resolve_from_minimal_repo() {
    let ctx = common::IntegrationTestContext::new();
    let conf_dir = ctx.root_path().join("conf");
    let platform = Platform::detect();

    let base = profiles::resolve("base", &conf_dir, platform);
    let desktop = profiles::resolve("desktop", &conf_dir, platform);

    assert!(base.is_ok(), "base profile should resolve");
    assert!(desktop.is_ok(), "desktop profile should resolve");
}

/// Requesting a non-existent profile must return an error.
#[test]
fn unknown_profile_returns_error() {
    let ctx = common::IntegrationTestContext::new();
    let conf_dir = ctx.root_path().join("conf");
    let platform = Platform::detect();

    let result = profiles::resolve("nonexistent", &conf_dir, platform);
    assert!(
        result.is_err(),
        "resolving an unknown profile should return an error"
    );
}

// ---------------------------------------------------------------------------
// Config loading: packages
// ---------------------------------------------------------------------------

/// Packages listed in packages.toml must be loaded into `config.packages`.
#[test]
fn config_loads_packages_from_ini() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("packages.toml", "[base]\npackages = [\"git\", \"curl\"]\n")
        .build();

    let platform = Platform {
        os: Os::Linux,
        is_arch: false,
        is_wsl: false,
    };
    let config = ctx.load_config_for_platform("base", platform);
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

/// Packages with `aur = true` in packages.toml must be loaded with
/// `is_aur = true`.
#[test]
fn config_loads_aur_packages_correctly() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "packages.toml",
            "[base]\npackages = [\"git\", { name = \"paru-bin\", aur = true }]\n",
        )
        .build();

    let platform = Platform {
        os: Os::Linux,
        is_arch: true,
        is_wsl: false,
    };
    let config = ctx.load_config_for_platform("base", platform);
    assert_eq!(config.packages.len(), 2);

    let aur_pkg = config
        .packages
        .iter()
        .find(|p| p.is_aur)
        .expect("aur package");
    assert_eq!(aur_pkg.name, "paru-bin");

    let regular_pkg = config
        .packages
        .iter()
        .find(|p| !p.is_aur)
        .expect("regular package");
    assert_eq!(regular_pkg.name, "git");
}

// ---------------------------------------------------------------------------
// Config loading: vscode extensions, copilot plugins, and chmod
// ---------------------------------------------------------------------------

/// VS Code extensions listed in vscode-extensions.toml must be loaded into
/// `config.vscode_extensions`.
#[test]
fn config_loads_vscode_extensions_correctly() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "vscode-extensions.toml",
            "[base]\nextensions = [\"ms-vscode.cpptools\", \"rust-lang.rust-analyzer\"]\n",
        )
        .build();

    let config = ctx.load_config("base");
    assert_eq!(
        config.vscode_extensions.len(),
        2,
        "expected 2 VS Code extensions, got {}",
        config.vscode_extensions.len()
    );
    assert!(
        config
            .vscode_extensions
            .iter()
            .any(|id| id == "ms-vscode.cpptools")
    );
    assert!(
        config
            .vscode_extensions
            .iter()
            .any(|id| id == "rust-lang.rust-analyzer")
    );
}

/// Chmod entries listed in chmod.toml must be loaded into `config.chmod`.
#[test]
fn config_loads_chmod_entries_correctly() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "chmod.toml",
            "[base]\npermissions = [{ mode = \"600\", path = \".ssh/config\" }, { mode = \"700\", path = \".ssh\" }]\n",
        )
        .build();

    let platform = Platform {
        os: Os::Linux,
        is_arch: false,
        is_wsl: false,
    };
    let config = ctx.load_config_for_platform("base", platform);
    assert_eq!(
        config.chmod.len(),
        2,
        "expected 2 chmod entries, got {}",
        config.chmod.len()
    );
    assert!(
        config
            .chmod
            .iter()
            .any(|e| e.mode == "600" && e.path == ".ssh/config")
    );
    assert!(
        config
            .chmod
            .iter()
            .any(|e| e.mode == "700" && e.path == ".ssh")
    );
}

// ---------------------------------------------------------------------------
// Config loading: registry entries (Windows-only)
// ---------------------------------------------------------------------------

/// Registry entries in registry.toml must be loaded into `config.registry`
/// when the platform is Windows.
#[test]
fn config_loads_registry_entries_on_windows() {
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

/// Registry entries in registry.toml must be skipped when the platform is Linux.
#[test]
fn config_does_not_load_registry_on_linux() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "registry.toml",
            "[console]\npath = 'HKCU:\\Console'\n[console.values]\nFontSize = 14\n",
        )
        .build();

    let platform = Platform {
        os: Os::Linux,
        is_arch: false,
        is_wsl: false,
    };
    let config = ctx.load_config_for_platform("base", platform);
    assert!(
        config.registry.is_empty(),
        "expected no registry entries on Linux"
    );
}

// ---------------------------------------------------------------------------
// Config loading: invalid TOML returns Err
// ---------------------------------------------------------------------------

/// `Config::load` must return `Err` (not panic) when a config file contains
/// invalid TOML syntax.
#[test]
fn config_load_returns_error_on_invalid_toml() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let conf = dir.path().join("conf");
    std::fs::create_dir_all(&conf).expect("create conf dir");

    // Write a valid profiles.toml.
    std::fs::write(
        conf.join("profiles.toml"),
        "[base]\ninclude = []\nexclude = [\"desktop\"]\n",
    )
    .expect("write profiles.toml");

    // Write an intentionally invalid symlinks.toml.
    std::fs::write(conf.join("symlinks.toml"), "this is not valid toml ][[")
        .expect("write invalid symlinks.toml");

    // Write the remaining required config files as empty so only symlinks.toml is bad.
    for file in &[
        "packages.toml",
        "manifest.toml",
        "chmod.toml",
        "systemd-units.toml",
        "vscode-extensions.toml",
        "git-config.toml",
        "registry.toml",
    ] {
        std::fs::write(conf.join(file), "").expect("write config file");
    }

    let platform = Platform::detect();
    let profile = profiles::resolve("base", &conf, platform).expect("resolve profile");
    let result = Config::load(dir.path(), &profile, platform, None);
    assert!(
        result.is_err(),
        "Config::load should return Err on invalid TOML, got Ok"
    );
}

// ---------------------------------------------------------------------------
// Config loading: error context includes filename
// ---------------------------------------------------------------------------

/// `Config::load` error messages must identify which file is broken so the
/// user knows where to look.
#[test]
fn config_load_error_context_includes_filename() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("packages.toml", "not valid {{ toml")
        .build();

    let platform = Platform::detect();
    let conf_dir = ctx.root_path().join("conf");
    let profile = profiles::resolve("base", &conf_dir, platform).expect("resolve profile");
    let result = Config::load(ctx.root_path(), &profile, platform, None);

    assert!(result.is_err(), "should fail on invalid packages.toml");
    let msg = format!("{:#}", result.unwrap_err());
    assert!(
        msg.contains("packages.toml"),
        "error should mention the file name: {msg}"
    );
}

// ---------------------------------------------------------------------------
// Config loading: type mismatch returns Err
// ---------------------------------------------------------------------------

/// Writing a TOML value with an incompatible type (e.g. integer instead of
/// array) must produce an error rather than silently ignoring the data.
#[test]
fn config_load_returns_error_on_type_mismatch() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", "[base]\nsymlinks = 42\n")
        .build();

    let platform = Platform::detect();
    let conf_dir = ctx.root_path().join("conf");
    let profile = profiles::resolve("base", &conf_dir, platform).expect("resolve profile");
    let result = Config::load(ctx.root_path(), &profile, platform, None);

    assert!(
        result.is_err(),
        "Config::load should return Err on type mismatch, got Ok"
    );
}

// ---------------------------------------------------------------------------
// Config loading: invalid profiles.toml returns Err
// ---------------------------------------------------------------------------

/// Malformed profiles.toml should return an error during profile resolution.
#[test]
fn config_load_returns_error_on_invalid_profiles_toml() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let conf = dir.path().join("conf");
    std::fs::create_dir_all(&conf).expect("create conf dir");

    std::fs::write(conf.join("profiles.toml"), "[base\ninclude = []\n")
        .expect("write invalid profiles.toml");

    let platform = Platform::detect();
    let result = profiles::resolve("base", &conf, platform);
    assert!(
        result.is_err(),
        "invalid profiles.toml should cause resolve to fail"
    );
}

// ---------------------------------------------------------------------------
// Config loading: unknown keys are rejected, not silently discarded
// ---------------------------------------------------------------------------

/// Load `conf/<file>` from a minimal repo and return the error it produced.
///
/// Panics if the configuration loads successfully — a misspelled key must
/// never be accepted.
fn expect_load_error(file: &str, content: &str) -> String {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(file, content)
        .build();

    let platform = Platform::detect();
    let conf_dir = ctx.root_path().join("conf");
    let profile = profiles::resolve("base", &conf_dir, platform).expect("resolve profile");
    let error = Config::load(ctx.root_path(), &profile, platform, None)
        .expect_err("a config with an unknown key must not load");
    format!("{error:#}")
}

/// A misspelled key inside a structured entry must fail the load.
///
/// Regression: these entries were `#[serde(untagged)]` enums, and untagged
/// enums ignore `deny_unknown_fields` — serde buffers the input and falls
/// through to the next variant, so `targett` parsed as if the key were absent
/// and the symlink silently used its conventional target instead.
#[test]
fn config_load_rejects_unknown_key_in_symlink_entry() {
    let message = expect_load_error(
        "symlinks.toml",
        "[base]\nsymlinks = [{ source = \"bashrc\", targett = \".bashrc\" }]\n",
    );
    assert!(
        message.contains("targett"),
        "error should name the unknown key, got: {message}"
    );
}

/// A misspelled key in a package entry must fail the load.
#[test]
fn config_load_rejects_unknown_key_in_package_entry() {
    let message = expect_load_error(
        "packages.toml",
        "[base]\npackages = [{ name = \"paru-bin\", our = true }]\n",
    );
    assert!(
        message.contains("our"),
        "error should name the unknown key, got: {message}"
    );
}

/// A misspelled key in a chmod entry must fail the load.
#[test]
fn config_load_rejects_unknown_key_in_chmod_entry() {
    let message = expect_load_error(
        "chmod.toml",
        "[base]\npermissions = [{ mode = \"600\", path = \"ssh/config\", pathh = \"x\" }]\n",
    );
    assert!(
        message.contains("pathh"),
        "error should name the unknown key, got: {message}"
    );
}

/// A misspelled section field must fail the load.
#[test]
fn config_load_rejects_unknown_section_field() {
    let message = expect_load_error("symlinks.toml", "[base]\nsymlink = [\"bashrc\"]\n");
    assert!(
        message.contains("symlink"),
        "error should name the unknown field, got: {message}"
    );
}

/// A misspelled key in `profiles.toml` must fail profile resolution.
///
/// Regression: `ProfileDef` marked every field `#[serde(default)]` without
/// `deny_unknown_fields`, so `excludee` produced an empty `exclude` list and
/// the `base` profile silently stopped excluding the `desktop` category.
#[test]
fn profile_resolution_rejects_unknown_key() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let conf = dir.path().join("conf");
    std::fs::create_dir_all(&conf).expect("create conf dir");
    std::fs::write(
        conf.join("profiles.toml"),
        "[base]\ninclude = []\nexcludee = [\"desktop\"]\n",
    )
    .expect("write profiles.toml");

    let error = profiles::resolve("base", &conf, Platform::detect())
        .expect_err("a misspelled profile key must not resolve");

    let message = format!("{error:#}");
    assert!(
        message.contains("excludee"),
        "error should name the unknown key, got: {message}"
    );
}
