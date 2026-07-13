//! Core tasks — the dotfiles tool itself.
//!
//! These tasks handle binary self-update, wrapper installation, and PATH
//! configuration.  They run during the Bootstrap phase so the tool is current
//! before any repository or apply work begins.

pub mod path;
pub mod self_update;
pub mod wrapper;
