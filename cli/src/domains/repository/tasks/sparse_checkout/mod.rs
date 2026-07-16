//! Sparse checkout task.

mod configure;

pub use configure::ConfigureSparseCheckout;

#[cfg(test)]
use crate::engine::{Task, TaskResult};
#[cfg(test)]
use crate::infra::fs::SystemFileSystemOps;
#[cfg(test)]
use anyhow::Result;
#[cfg(test)]
use configure::*;
#[cfg(test)]
use std::path::Path;

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests;
