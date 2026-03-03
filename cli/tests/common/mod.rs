// Shared helpers for integration tests.
//
// Provides a temporary-directory-backed test repository and a fluent builder
// so each integration test can set up an isolated environment without
// repeating filesystem boilerplate.
//
// Used by all integration test binaries that declare `mod common;`.
#![allow(dead_code)]

use std::path::Path;
use std::sync::{Arc, RwLock};

use dotfiles_cli::config::Config;
use dotfiles_cli::config::profiles;
use dotfiles_cli::exec::{ExecResult, Executor};
use dotfiles_cli::logging::{Log, Logger};
use dotfiles_cli::platform::Platform;
use dotfiles_cli::tasks::Context;

/// Write the minimal set of TOML config files required by the dotfiles engine
/// into `root`.
///
/// Creates:
/// - `conf/profiles.toml`          — base and desktop profile definitions
/// - `conf/symlinks.toml`           — empty symlink list
/// - `conf/packages.toml`           — empty package list
/// - `conf/chmod.toml`              — empty chmod list
/// - `conf/systemd-units.toml`
/// - `conf/vscode-extensions.toml`
/// - `conf/copilot-skills.toml`
/// - `conf/git-config.toml`
/// - `conf/registry.toml`
/// - `symlinks/`                    — directory expected by validation tasks
/// - `hooks/`                       — directory expected by validation tasks
pub fn setup_minimal_repo(root: &Path) {
    let conf = root.join("conf");
    std::fs::create_dir_all(&conf).expect("create conf dir");
    std::fs::create_dir_all(root.join("symlinks")).expect("create symlinks dir");
    std::fs::create_dir_all(root.join("hooks")).expect("create hooks dir");

    std::fs::write(
        conf.join("profiles.toml"),
        "[base]\ninclude = []\nexclude = [\"desktop\"]\n\n[desktop]\ninclude = [\"desktop\"]\nexclude = []\n",
    )
    .expect("write profiles.toml");

    for file in &[
        "symlinks.toml",
        "packages.toml",
        "chmod.toml",
        "systemd-units.toml",
        "vscode-extensions.toml",
        "copilot-skills.toml",
        "git-config.toml",
        "registry.toml",
    ] {
        std::fs::write(conf.join(file), "").expect("write config file");
    }
}

/// An isolated test repository backed by a [`tempfile::TempDir`].
///
/// The directory is automatically deleted when dropped (via the underlying
/// [`tempfile::TempDir`]).
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
            profiles::resolve(profile_name, &conf_dir, platform).expect("resolve profile");
        Config::load(self.root.path(), &profile, platform).expect("load config")
    }

    /// Load configuration for the given profile using the provided platform.
    ///
    /// Use this variant in tests that need to control platform-specific behaviour
    /// (e.g. registry loading on Windows, AUR warnings on non-Arch Linux) without
    /// depending on the host OS the test suite runs on.
    pub fn load_config_for_platform(&self, profile_name: &str, platform: Platform) -> Config {
        let conf_dir = self.root.path().join("conf");
        let profile =
            profiles::resolve(profile_name, &conf_dir, platform).expect("resolve profile");
        Config::load(self.root.path(), &profile, platform).expect("load config")
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

    /// Create a source file with specific content inside the `symlinks/` directory.
    pub fn with_symlink_source_content(self, source: &str, content: &str) -> Self {
        let path = self.ctx.root.path().join("symlinks").join(source);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create symlink source parent");
        }
        std::fs::write(&path, content).expect("write symlink source file");
        self
    }

    /// Create a hook file (no extension) inside the `hooks/` directory.
    pub fn with_hook_source(self, name: &str, content: &str) -> Self {
        let path = self.ctx.root.path().join("hooks").join(name);
        std::fs::write(path, content).expect("write hook source file");
        self
    }

    /// Create a `.git/hooks/` directory in the test repository.
    pub fn with_git_hooks_dir(self) -> Self {
        std::fs::create_dir_all(self.ctx.root.path().join(".git/hooks"))
            .expect("create .git/hooks dir");
        self
    }

    /// Finish building and return the configured context.
    pub fn build(self) -> IntegrationTestContext {
        self.ctx
    }
}

/// Stub executor for integration tests that don't invoke system commands.
///
/// Returns `false` for `which()` and panics if any command is actually run.
#[derive(Debug)]
pub struct StubExecutor;

#[allow(clippy::panic)]
impl Executor for StubExecutor {
    fn run(&self, program: &str, args: &[&str]) -> anyhow::Result<ExecResult> {
        panic!("unexpected executor call in integration test: {program} {args:?}")
    }

    fn run_in(&self, _: &Path, program: &str, args: &[&str]) -> anyhow::Result<ExecResult> {
        panic!("unexpected executor call in integration test: {program} {args:?}")
    }

    fn run_in_with_env(
        &self,
        _: &Path,
        program: &str,
        args: &[&str],
        _: &[(&str, &str)],
    ) -> anyhow::Result<ExecResult> {
        panic!("unexpected executor call in integration test: {program} {args:?}")
    }

    fn run_unchecked(&self, program: &str, args: &[&str]) -> anyhow::Result<ExecResult> {
        panic!("unexpected executor call in integration test: {program} {args:?}")
    }

    fn which(&self, _: &str) -> bool {
        false
    }

    fn which_path(&self, program: &str) -> anyhow::Result<std::path::PathBuf> {
        anyhow::bail!("{program} not found on PATH")
    }
}

/// A task execution context backed by a temporary home directory.
///
/// Returned by [`IntegrationTestContext::make_context`] to bundle the
/// task [`Context`] with ownership of the temporary home directory.
pub struct ExecutionContext {
    /// Task execution context.
    pub ctx: Context,
    /// Logger for inspecting task outcomes.
    pub log: Arc<Logger>,
    /// Temporary home directory — dropped when this struct is dropped.
    _home: tempfile::TempDir,
}

impl IntegrationTestContext {
    /// Create a task execution [`Context`] for the given profile.
    ///
    /// Sets up a temporary home directory and a [`StubExecutor`] so that
    /// filesystem-only tasks (symlinks, hooks, chmod) can execute without
    /// invoking external commands.
    pub fn make_context(&self, profile: &str) -> ExecutionContext {
        let config = self.load_config(profile);
        let home = tempfile::tempdir().expect("create home dir");
        let log = Arc::new(Logger::new("test"));
        let ctx = Context {
            config: Arc::new(RwLock::new(config)),
            platform: Platform::detect(),
            log: Arc::clone(&log) as Arc<dyn Log>,
            dry_run: false,
            home: home.path().to_path_buf(),
            executor: Arc::new(StubExecutor),
            parallel: false,
        };
        ExecutionContext {
            ctx,
            log,
            _home: home,
        }
    }

    /// Create a dry-run task execution [`Context`] for the given profile.
    pub fn make_dry_run_context(&self, profile: &str) -> ExecutionContext {
        let mut ec = self.make_context(profile);
        ec.ctx.dry_run = true;
        ec
    }
}
