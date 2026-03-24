//! Bootstrap tasks — prepare the dotfiles tool itself.
//!
//! These tasks run first and handle binary updates, wrapper installation,
//! PATH configuration, and platform prerequisites like Windows developer mode.

pub mod developer_mode;
pub mod path;
pub mod self_update;
pub mod wrapper;
