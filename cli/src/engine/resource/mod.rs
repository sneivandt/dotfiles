//! Generic resource contract: the idempotent check + apply primitives shared
//! by all concrete domain resources.

mod contract;
mod error;

pub use contract::{IntrinsicState, ResourceChange, ResourceState};
pub(crate) use contract::{RemovableResource, Resource, ResourceResult, SkipKind};
pub use error::ResourceError;

#[cfg(test)]
mod tests;
