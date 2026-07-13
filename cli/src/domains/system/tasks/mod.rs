//! System tasks — operating-system integration.
//!
//! These tasks handle OS-level configuration: the Windows registry, systemd
//! units, Windows developer mode, PAM, and WSL configuration.

pub mod developer_mode;
pub mod pam;
pub mod registry;
pub mod systemd_units;
pub mod wsl_conf;
