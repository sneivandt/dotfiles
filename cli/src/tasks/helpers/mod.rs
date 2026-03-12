//! Internal support modules for the task system — macros and registration.

mod catalog;
mod macros;

pub use catalog::{all_install_tasks, all_uninstall_tasks};
pub(crate) use macros::{batch_resource_task, resource_task, task_deps};
