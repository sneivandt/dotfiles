//! Dotfiles management engine.
//!
//! Cross-platform tool for declarative dotfile installation: symlinks,
//! packages, permissions, systemd units, registry entries, VS Code extensions,
//! and GitHub Copilot skills — all driven by INI configuration files in
//! `conf/` and filtered by profile and platform.
//!
//! The public API is organised into four layers:
//!
//! - **[`config`]** — parse and validate INI config files
//! - **[`resources`]** — idempotent `check + apply` primitives (symlinks, packages, …)
//! - **[`tasks`]** — named, dependency-ordered units of work wired to resources
//! - **[`commands`]** — top-level subcommand orchestration (`install`, `uninstall`, `test`)
#![deny(clippy::or_fun_call)]
#![deny(clippy::bool_to_int_with_if)]

pub mod cli;
pub mod commands;
pub mod config;
pub mod exec;
pub mod logging;
pub mod platform;
pub mod resources;
pub mod tasks;
