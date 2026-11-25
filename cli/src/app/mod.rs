//! Application layer: CLI parsing, command handlers, aggregate configuration,
//! the task catalog, task filtering, cross-domain validation, and startup
//! wiring.
//!
//! The app layer may depend on every other layer ([`crate::engine`],
//! [`crate::infra`], and [`crate::domains`]); nothing else may depend on it.

pub(crate) mod catalog;
pub(crate) mod cli;
mod cli_words;
pub(crate) mod commands;
pub(crate) mod completion;
pub(crate) mod config;
pub(crate) mod filter;
pub(crate) mod interrupt;
pub(crate) mod run;
pub(crate) mod validation;

#[cfg(test)]
pub(crate) mod test_helpers;
