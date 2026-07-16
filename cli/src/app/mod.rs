//! Application layer: CLI parsing, command handlers, aggregate configuration,
//! the task catalog, task filtering, cross-domain validation, and startup
//! wiring.
//!
//! The app layer may depend on every other layer ([`crate::engine`],
//! [`crate::infra`], and [`crate::domains`]); nothing else may depend on it.

pub mod catalog;
pub mod cli;
pub mod commands;
pub mod config;
pub mod filter;
pub mod reload;
pub mod run;
pub mod validation;

#[cfg(test)]
pub mod test_helpers;
