//! System tasks — operating-system integration.
//!
//! These tasks handle OS-level configuration: the Windows registry, systemd
//! units, Windows developer mode, and WSL configuration.

pub mod developer_mode;
pub mod registry;
pub mod systemd_units;
pub mod wsl_conf;
