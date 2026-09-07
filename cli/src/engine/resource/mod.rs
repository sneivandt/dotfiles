//! Generic resource contract: the idempotent check + apply primitives shared
//! by all concrete domain resources.

mod contract;
mod error;

pub use contract::{
    IntrinsicState, RemovableResource, Resource, ResourceChange, ResourceResult, ResourceState,
    SkipKind,
};
pub use error::ResourceError;

#[cfg(test)]
mod tests;
