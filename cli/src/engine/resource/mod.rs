//! Generic resource contract: the idempotent check + apply primitives shared
//! by all concrete domain resources.

mod contract;
mod error;
mod provider;

pub use contract::{
    RemovableResource, Resource, ResourceChange, ResourceResult, ResourceState, SkipKind,
};
pub use error::ResourceError;
pub use provider::{
    CachedStateProvider, IntrinsicState, IntrinsicStateProvider, ResourceStateProvider,
};

#[cfg(test)]
mod tests;
