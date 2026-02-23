// Shared helpers for integration tests.
//
// Provides a temporary-directory-backed test repository and a fluent builder
// so each integration test can set up an isolated environment without
// repeating filesystem boilerplate.
//
// Used by all integration test binaries that declare `mod common;`.
#![allow(dead_code)]

use std::path::Path;

use dotfiles_cli::config::Config;
use dotfiles_cli::config::profiles;
use dotfiles_cli::platform::Platform;

/// Write the minimal set of INI config files required by the dotfiles engine
/// into `root`.
///
/// Creates:
/// - `conf/profiles.ini`   — base and desktop profile definitions
/// - `conf/symlinks.ini`   — empty symlink list
/// - `conf/packages.ini`   — empty package list
/// - `conf/manifest.ini`   — empty manifest
/// - `conf/chmod.ini`      — empty chmod list
/// - `conf/systemd-units.ini`
/// - `conf/vscode-extensions.ini`
/// - `conf/copilot-skills.ini`
/// - `conf/registry.ini`
/// - `symlinks/`           — directory expected by validation tasks
/// - `hooks/`              — directory expected by validation tasks
pub fn setup_minimal_repo(root: &Path) {
    let conf = root.join("conf");
    std::fs::create_dir_all(&conf).expect("create conf dir");
    std::fs::create_dir_all(root.join("symlinks")).expect("create symlinks dir");
    std::fs::create_dir_all(root.join("hooks")).expect("create hooks dir");

    std::fs::write(
        conf.join("profiles.ini"),
        "[base]\ninclude=\nexclude=desktop\n\n[desktop]\ninclude=desktop\nexclude=\n",
    )
    .expect("write profiles.ini");

    for file in &[
        "symlinks.ini",
        "packages.ini",
        "manifest.ini",
        "chmod.ini",
        "systemd-units.ini",
        "vscode-extensions.ini",
        "copilot-skills.ini",
        "registry.ini",
    ] {
        std::fs::write(conf.join(file), "").expect("write config file");
    }
}

/// An isolated test repository backed by a [`tempfile::TempDir`].
///
/// The directory is deleted automatically when the context is dropped.
pub struct IntegrationTestContext {
    /// Temporary directory containing the test dotfiles repository.
    pub root: tempfile::TempDir,
}

impl IntegrationTestContext {
    /// Create a new context with a minimal but valid repository structure.
    pub fn new() -> Self {
        let root = tempfile::tempdir().expect("create temp dir");
        setup_minimal_repo(root.path());
        Self { root }
    }

    /// Path to the repository root.
    pub fn root_path(&self) -> &Path {
        self.root.path()
    }

    /// Load configuration for the given profile using the current platform.
    pub fn load_config(&self, profile_name: &str) -> Config {
        let platform = Platform::detect();
        let conf_dir = self.root.path().join("conf");
        let profile =
            profiles::resolve(profile_name, &conf_dir, &platform).expect("resolve profile");
        Config::load(self.root.path(), &profile, &platform).expect("load config")
    }
}

/// Fluent builder for [`IntegrationTestContext`].
///
/// Allows individual tests to customise the repository before the context
/// is finalised without modifying the shared setup.
pub struct TestContextBuilder {
    ctx: IntegrationTestContext,
}

impl TestContextBuilder {
    /// Begin building a new context backed by a minimal repository.
    pub fn new() -> Self {
        Self {
            ctx: IntegrationTestContext::new(),
        }
    }

    /// Write `content` to `conf/<filename>` in the test repository,
    /// overwriting any file written by [`setup_minimal_repo`].
    pub fn with_config_file(self, filename: &str, content: &str) -> Self {
        let path = self.ctx.root.path().join("conf").join(filename);
        std::fs::write(path, content).expect("write config file");
        self
    }

    /// Create a source file inside the `symlinks/` directory so that
    /// symlink validation does not complain about missing sources.
    pub fn with_symlink_source(self, source: &str) -> Self {
        let path = self.ctx.root.path().join("symlinks").join(source);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create symlink source parent");
        }
        std::fs::write(&path, "").expect("write symlink source file");
        self
    }

    /// Finish building and return the configured context.
    pub fn build(self) -> IntegrationTestContext {
        self.ctx
    }
}
