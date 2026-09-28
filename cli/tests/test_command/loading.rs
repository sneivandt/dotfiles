//! Config loading, profile resolution, and parse-error reporting.

use dotfiles_cli::testing as test_api;
use test_api::config::Config;
use test_api::config::profiles;
use test_api::platform::{Os, Platform};

use crate::common;

// ---------------------------------------------------------------------------
// Config loading
// ---------------------------------------------------------------------------

#[test]
fn config_loads_from_minimal_repo_for_both_profiles() {
    for profile in ["base", "desktop"] {
        let ctx = common::IntegrationTestContext::new();
        let config = ctx.load_config(profile);
        assert!(
            config.symlinks.is_empty(),
            "{profile}: expected no symlinks"
        );
        assert!(
            config.packages.is_empty(),
            "{profile}: expected no packages"
        );
    }
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
    let profile = profiles::resolve("base", platform).expect("resolve profile");
    let error = Config::load(ctx.root_path(), &profile, platform, None)
        .expect_err("missing config should fail");
    assert!(error.to_string().contains("agent-settings.toml"));
}

// ---------------------------------------------------------------------------
// Profile resolution
// ---------------------------------------------------------------------------

/// Both built-in profiles must resolve successfully.
#[test]
fn both_builtin_profiles_resolve() {
    let platform = Platform::detect();

    let base = profiles::resolve("base", platform);
    let desktop = profiles::resolve("desktop", platform);

    assert!(base.is_ok(), "base profile should resolve");
    assert!(desktop.is_ok(), "desktop profile should resolve");
}

/// Requesting a non-existent profile must return an error.
#[test]
fn unknown_profile_returns_error() {
    let platform = Platform::detect();

    let result = profiles::resolve("nonexistent", platform);
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

#[test]
fn config_loads_registry_entries_only_on_windows() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "registry.toml",
            "[console]\npath = 'HKCU:\\Console'\n[console.values]\nFontSize = 14\n",
        )
        .build();

    for os in [Os::Windows, Os::Linux] {
        let config = ctx.load_config_for_platform(
            "base",
            Platform {
                os,
                is_arch: false,
                is_wsl: false,
            },
        );
        let entries: Vec<_> = config
            .registry
            .iter()
            .map(|entry| {
                (
                    entry.key_path.as_str(),
                    entry.value_name.as_str(),
                    entry.value_data.as_str(),
                )
            })
            .collect();
        let expected = if os == Os::Windows {
            vec![("HKCU:\\Console", "FontSize", "14")]
        } else {
            vec![]
        };
        assert_eq!(entries, expected, "{os:?}");
    }
}

// ---------------------------------------------------------------------------
// Config loading: parse errors identify the offending file and unknown key
// ---------------------------------------------------------------------------

#[test]
fn config_load_rejects_malformed_values_and_unknown_keys() {
    let cases = [
        (
            "invalid symlinks TOML",
            "symlinks.toml",
            "this is not valid toml ][[",
            None,
        ),
        (
            "invalid packages TOML",
            "packages.toml",
            "not valid {{ toml",
            None,
        ),
        (
            "type mismatch",
            "symlinks.toml",
            "[base]\nsymlinks = 42\n",
            None,
        ),
        // Regression: untagged entry enums silently discarded misspelled keys.
        (
            "symlink key",
            "symlinks.toml",
            "[base]\nsymlinks = [{ source = \"bashrc\", targett = \".bashrc\" }]\n",
            Some("targett"),
        ),
        (
            "package key",
            "packages.toml",
            "[base]\npackages = [{ name = \"paru-bin\", our = true }]\n",
            Some("our"),
        ),
        (
            "chmod key",
            "chmod.toml",
            "[base]\npermissions = [{ mode = \"600\", path = \"ssh/config\", pathh = \"x\" }]\n",
            Some("pathh"),
        ),
        (
            "section field",
            "symlinks.toml",
            "[base]\nsymlink = [\"bashrc\"]\n",
            Some("symlink"),
        ),
    ];
    for (case, file, content, unknown_key) in cases {
        let ctx = common::TestContextBuilder::new()
            .with_config_file(file, content)
            .build();
        let platform = Platform::detect();
        let profile = profiles::resolve("base", platform).expect("resolve profile");
        let error = Config::load(ctx.root_path(), &profile, platform, None).expect_err(case);
        let message = format!("{error:#}");
        assert!(
            message.contains(file),
            "{case}: missing filename in {message}"
        );
        if let Some(key) = unknown_key {
            assert!(
                message.contains(key),
                "{case}: missing unknown key {key} in {message}"
            );
        }
    }
}
