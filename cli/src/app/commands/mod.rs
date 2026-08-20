//! Top-level command handlers and shared command orchestration.

pub mod install;
pub mod log;
pub mod tasks;
pub mod test;
pub mod uninstall;
pub mod update;

pub(crate) mod error;
mod execution;
mod reexec;
mod runner;

pub(crate) use reexec::prepare_self_update;
pub use runner::CommandRunner;

#[cfg(test)]
use reexec::{REEXEC_GUARD_VAR, build_reexec_command, re_exec_path};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
