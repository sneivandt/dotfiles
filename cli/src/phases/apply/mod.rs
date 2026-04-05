//! Apply tasks — apply declared state to the user environment.
//!
//! These tasks run after the repository phase completes and handle symlinks,
//! packages, git settings, file permissions, shell configuration, systemd units,
//! registry entries, editor extensions, and other system configuration.

pub mod chmod;
pub mod copilot_plugins;
pub mod git_config;
pub mod overlay_scripts;
pub mod packages;
pub mod pam;
pub mod registry;
pub mod shell;
pub mod symlinks;
pub mod systemd_units;
pub mod vscode_extensions;
pub mod wsl_conf;
