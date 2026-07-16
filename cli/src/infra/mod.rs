//! Infrastructure mechanisms: process execution, filesystem access, logging,
//! platform and elevation detection, cancellation, and generic configuration
//! support.
//!
//! This is the lowest layer of the crate. It depends only on `std` and external
//! crates — never on `engine`, `domains`, or `app`.

pub mod config;
pub mod elevation;
pub mod exec;
pub mod fs;
pub mod logging;
pub mod platform;

/// Shared one-shot boolean flag backing cross-thread signalling primitives.
pub(crate) mod atomic_flag;
/// Process-wide cancellation flag for graceful shutdown.
pub mod cancellation;

pub use config::ConfigHandle;
