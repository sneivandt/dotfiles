//! Repository tasks — synchronise the dotfiles repository.
//!
//! These tasks run after the bootstrap phase completes and handle sparse
//! checkout configuration, repository updates, config reloading, and git hooks.

pub mod hooks;
pub mod reload_config;
pub mod sparse_checkout;
pub mod update;
