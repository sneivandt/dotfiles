//! Runtime facilities: process execution, filesystem access, logging, platform
//! and elevation detection, error types, and generic configuration parsing
//! support.
//!
//! This is the lowest layer of the crate. It depends only on `std` and external
//! crates — never on `engine`, `domains`, or `app`.

pub mod config_handle;
pub mod config_support;
pub mod elevation;
pub mod error;
pub mod exec;
pub mod fs;
pub mod logging;
pub mod platform;

/// Shared one-shot boolean flag backing cross-thread signalling primitives.
pub(crate) mod atomic_flag;
/// Process-wide cancellation flag for graceful shutdown.
pub mod cancellation;

pub use config_handle::ConfigHandle;
