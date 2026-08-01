#![allow(
    unreachable_pub,
    reason = "internal modules are hidden behind the binary entry point and the testing facade"
)]

//! Dotfiles management engine library entry point.
//!
//! Cross-platform tool for declarative dotfile installation: symlinks,
//! packages, permissions, systemd units, registry entries, VS Code extensions,
//! and AI plugin manifests (via Microsoft APM) — all driven by TOML
//! configuration files in `conf/` and filtered by profile and platform.
//!
//! The stable public API is intentionally small: [`run`] executes the CLI.
//! Engine internals remain crate-private so implementation details can evolve
//! without becoming an accidental library contract.

use std::process::ExitCode;

mod app;
mod domains;
mod engine;
mod infra;

/// Internal API facade for integration tests and doctests.
///
/// Not a supported API surface. The crate is `publish = false` and this module
/// is `#[doc(hidden)]`; it exists so `tests/` can drive the real engine without
/// making every internal item `pub`.
#[doc(hidden)]
pub mod testing;

#[cfg(test)]
pub(crate) use app::config::Config;
#[cfg(test)]
pub(crate) use app::test_helpers;

/// Run the dotfiles CLI and return the process exit code.
///
/// This is the only supported public entry point for the library crate.  The
/// binary target delegates here so argument parsing, logging setup, graceful
/// cancellation, elevation handling, and command dispatch live in one place.
#[must_use]
pub fn run() -> ExitCode {
    app::run::run()
}
