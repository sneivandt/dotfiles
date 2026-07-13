//! Shared helpers for task and engine unit tests.
//!
//! Provides common mock types and factory functions so each task test module
//! does not have to duplicate boilerplate.
#![allow(clippy::panic, reason = "test code uses panicking helpers")]

use std::path::PathBuf;
use std::sync::Arc;

use crate::app::config::Config;
use crate::app::config::profiles::Profile;
use crate::domains::repository::config::manifest::Manifest;
use crate::runtime::config_support::category_matcher::Category;
use crate::runtime::exec::{Executor, MockExecutor};
use crate::runtime::logging::Logger;
use crate::runtime::platform::Platform;

use crate::engine::Context;

/// Build a [`Config`] with all lists empty and `root` set to `root`.
#[must_use]
#[allow(
    clippy::expect_used,
    reason = "panicking allowed at this trust boundary"
)]
pub fn empty_config(root: PathBuf) -> Config {
    Config {
        root,
        overlay: None,
        profile: Profile {
            name: "test".to_string(),
            active_categories: vec![Category::Base],
            excluded_categories: vec![],
        },
        packages: vec![],
        symlinks: vec![],
        registry: vec![],
        units: vec![],
        pam_services: vec![],
        chmod: vec![],
        vscode_extensions: vec![],
        git_settings: vec![],
        copilot_settings: vec![],
        manifest: Manifest {
            excluded_files: vec![],
        },
        scripts: vec![],
    }
}

/// Build a [`Context`] from the given config, platform and executor.
pub fn make_context(config: Config, platform: Platform, executor: Arc<dyn Executor>) -> Context {
    let root = config.root;
    let overlay = config.overlay;
    Context::from_raw(
        root,
        overlay,
        platform,
        Arc::new(Logger::new("test")),
        executor,
        PathBuf::from("/home/test"),
        crate::engine::ContextOpts {
            dry_run: false,
            parallel: false,
            advance_versions: false,
            is_ci: Some(false),
        },
    )
}

/// Build a stub [`MockExecutor`] that returns `which_result` for every
/// `which()` / `which_path()` call and panics on any `run*()` call.
#[must_use]
pub fn stub_executor(which_result: bool) -> MockExecutor {
    let mut mock = MockExecutor::new();
    mock.expect_which().returning(move |_| which_result);
    mock.expect_which_path().returning(move |program| {
        if which_result {
            #[cfg(windows)]
            let path = PathBuf::from(format!(r"C:\Windows\System32\{program}.exe"));
            #[cfg(not(windows))]
            let path = PathBuf::from(format!("/usr/bin/{program}"));
            Ok(path)
        } else {
            anyhow::bail!("{program} not found on PATH")
        }
    });
    mock
}

/// Builder for test [`Context`] instances.
///
/// Provides a fluent API so that tests can construct exactly the context
/// variant they need without relying on a growing list of factory functions.
///
/// # Example
///
/// ```ignore
/// let ctx = ContextBuilder::new(config)
///     .os(crate::runtime::platform::Os::Linux)
///     .arch(true)
///     .which(true)
///     .build();
/// ```
#[derive(Debug)]
#[must_use]
#[allow(clippy::struct_excessive_bools, reason = "test fixture")]
pub struct ContextBuilder {
    config: Config,
    os: crate::runtime::platform::Os,
    is_arch: bool,
    is_wsl: bool,
    which_result: bool,
    is_ci: bool,
}

impl ContextBuilder {
    /// Create a new builder with Linux, non-arch, `which = false` defaults.
    pub fn new(config: Config) -> Self {
        Self {
            config,
            os: crate::runtime::platform::Os::Linux,
            is_arch: false,
            is_wsl: false,
            which_result: false,
            is_ci: false,
        }
    }

    /// Set the target OS.
    pub fn os(mut self, os: crate::runtime::platform::Os) -> Self {
        self.os = os;
        self
    }

    /// Set whether the platform is Arch Linux.
    pub fn arch(mut self, is_arch: bool) -> Self {
        self.is_arch = is_arch;
        self
    }

    /// Set whether the platform is Windows Subsystem for Linux.
    pub fn wsl(mut self, is_wsl: bool) -> Self {
        self.is_wsl = is_wsl;
        self
    }

    /// Set the value returned by `executor.which()`.
    pub fn which(mut self, which_result: bool) -> Self {
        self.which_result = which_result;
        self
    }

    /// Set whether the context simulates a CI environment.
    ///
    /// Tasks that check [`Context::is_ci`] (such as `ConfigureShell`)
    /// can be tested without mutating process-global environment variables.
    pub fn ci(mut self, is_ci: bool) -> Self {
        self.is_ci = is_ci;
        self
    }

    /// Consume the builder and produce a [`Context`].
    #[must_use]
    pub fn build(self) -> Context {
        make_context(
            self.config,
            Platform {
                os: self.os,
                is_arch: self.is_arch,
                is_wsl: self.is_wsl,
            },
            Arc::new(stub_executor(self.which_result)),
        )
        .with_ci(self.is_ci)
    }
}

/// Build a [`Context`] with the specified OS/arch and a [`MockExecutor`]
/// that returns the given `which_result`.
///
/// Use this when a task's `should_run` or `run` method gates on tool
/// availability via `ctx.executor.which(...)`.
#[must_use]
pub fn make_platform_context_with_which(
    config: Config,
    os: crate::runtime::platform::Os,
    is_arch: bool,
    which_result: bool,
) -> Context {
    ContextBuilder::new(config)
        .os(os)
        .arch(is_arch)
        .which(which_result)
        .build()
}

/// Build a [`Context`] with a Linux non-arch platform and default [`MockExecutor`].
///
/// Convenience shorthand for tests that only need a plain Linux context.
#[must_use]
pub fn make_linux_context(config: Config) -> Context {
    ContextBuilder::new(config).build()
}

/// Build a [`Context`] with a Windows platform and default [`MockExecutor`].
///
/// Convenience shorthand for tests that only need a plain Windows context.
#[must_use]
pub fn make_windows_context(config: Config) -> Context {
    ContextBuilder::new(config)
        .os(crate::runtime::platform::Os::Windows)
        .build()
}

/// Build a [`Context`] with an Arch Linux platform and default [`MockExecutor`].
///
/// Convenience shorthand for tests that target Arch-specific behaviour.
#[must_use]
pub fn make_arch_context(config: Config) -> Context {
    ContextBuilder::new(config).arch(true).build()
}

/// Build a [`Context`] with a default Linux platform and
/// default [`MockExecutor`], also returning the [`Logger`] so tests can
/// inspect recorded task state.
#[must_use]
pub fn make_static_context(config: Config) -> (Context, Arc<Logger>) {
    let log = Arc::new(Logger::new("test"));
    let log_output: Arc<dyn crate::runtime::logging::Log> = Arc::<Logger>::clone(&log);
    let ctx = make_linux_context(config).with_log(log_output);
    (ctx, log)
}
