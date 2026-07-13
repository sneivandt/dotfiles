//! Package installation tasks.

mod install;

pub use install::{InstallAurPackages, InstallPackages, InstallParu};

#[cfg(test)]
use crate::engine::{Context, Task, TaskResult};

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests;
