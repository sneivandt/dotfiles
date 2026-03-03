//! Filesystem operation abstractions.
//!
//! The [`FileSystemOps`] trait and its implementations now live in
//! [`crate::resources::helpers::fs`], which is the authoritative source for
//! all filesystem helpers.  This module re-exports them so that any code
//! still referencing the `operations` path continues to compile.
pub use crate::resources::helpers::fs::{FileSystemOps, SystemFileSystemOps};
#[cfg(test)]
pub use crate::resources::helpers::fs::MockFileSystemOps;
