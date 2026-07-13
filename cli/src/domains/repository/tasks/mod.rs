//! Repository tasks — synchronise the dotfiles repository.
//!
//! These tasks run during the Sync phase and handle sparse checkout
//! configuration and repository updates.  Configuration reloading after a pull
//! is owned by the application layer, since it composes the aggregate
//! configuration.

pub mod sparse_checkout;
pub mod update;
