//! Top-level command handlers and shared command orchestration.

pub mod install;
pub mod log;
pub mod test;
pub mod uninstall;
pub mod update;
pub mod version;

pub(crate) mod error;
mod execution;
mod reexec;
mod runner;

pub(crate) use reexec::prepare_self_update;
pub use runner::CommandRunner;

#[cfg(all(test, windows))]
use reexec::build_windows_restart_helper_script;
#[cfg(all(test, unix))]
use reexec::re_exec_path;
#[cfg(test)]
use runner::log_overlay_path;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
