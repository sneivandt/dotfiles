//! Repository tasks — synchronise the dotfiles repository.
//!
//! These tasks run during the Repository phase and handle sparse checkout
//! configuration, repository updates, and config reloading.

pub mod reload_config;
pub mod sparse_checkout;
pub mod update;
