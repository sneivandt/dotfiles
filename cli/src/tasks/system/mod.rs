//! System tasks — manage the dotfiles system itself.
//!
//! These tasks run first and handle binary updates, repository sync,
//! sparse checkout configuration, config reloading, and platform prerequisites.

pub mod developer_mode;
pub mod hooks;
pub mod path;
pub mod reload_config;
pub mod self_update;
pub mod sparse_checkout;
pub mod update;
pub mod wrapper;
