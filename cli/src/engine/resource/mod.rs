//! Generic resource contract: the idempotent check + apply primitives shared
//! by all concrete domain resources.

mod contract;
mod error;
mod provider;

pub use contract::{Resource, ResourceChange, ResourceResult, ResourceState};
pub use error::ResourceError;
pub use provider::{
    BorrowedStateProvider, IntrinsicState, IntrinsicStateProvider, PreloadedStateProvider,
    ResourceStateProvider,
};

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests;
