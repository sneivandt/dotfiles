//! Resource state discovery providers.

use super::{Resource, ResourceResult, ResourceState};

/// Provides current state for a batch of resources.
///
/// Implementations either inspect each resource directly (see
/// [`IntrinsicStateProvider`]) or answer from state the caller gathered up
/// front (see [`CachedStateProvider`]). Bulk state belongs in the provider's
/// own fields, so the trait itself stays a single question per resource.
pub trait ResourceStateProvider<R: Resource> {
    /// Determine the current state for one resource.
    ///
    /// # Errors
    ///
    /// Returns a [`ResourceError`](super::ResourceError) if the resource state
    /// cannot be determined.
    fn current_state(&self, resource: &R) -> ResourceResult<ResourceState>;
}

/// State-checking extension for resources that can inspect themselves.
///
/// This is bridged into the orchestration layer by [`IntrinsicStateProvider`].
pub trait IntrinsicState: Resource {
    /// Check the current state of the resource.
    ///
    /// # Errors
    ///
    /// Returns a [`ResourceError`](super::ResourceError) if the resource state
    /// cannot be determined due to I/O failures, permission issues, or other
    /// system errors. Returning the typed error (rather than `anyhow::Error`)
    /// keeps [`ResourceError::category`](super::ResourceError::category)
    /// available to the orchestration layer without downcasting.
    fn current_state(&self) -> ResourceResult<ResourceState>;
}

/// State provider for resources that implement [`IntrinsicState`].
#[derive(Debug, Clone, Copy, Default)]
pub struct IntrinsicStateProvider;

impl<R: IntrinsicState> ResourceStateProvider<R> for IntrinsicStateProvider {
    fn current_state(&self, resource: &R) -> ResourceResult<ResourceState> {
        resource.current_state()
    }
}

/// State provider backed by an already-loaded, borrowed cache.
#[derive(Debug, Clone)]
pub struct CachedStateProvider<'cache, Cache: ?Sized, State> {
    cache: &'cache Cache,
    state: State,
}

impl<'cache, Cache: ?Sized, State> CachedStateProvider<'cache, Cache, State> {
    /// Create a provider from a borrowed cache and state-mapping closure.
    #[must_use]
    pub const fn new(cache: &'cache Cache, state: State) -> Self {
        Self { cache, state }
    }
}

impl<R, Cache, State> ResourceStateProvider<R> for CachedStateProvider<'_, Cache, State>
where
    R: Resource,
    Cache: Sync + ?Sized,
    State: Fn(&R, &Cache) -> ResourceResult<ResourceState> + Sync,
{
    fn current_state(&self, resource: &R) -> ResourceResult<ResourceState> {
        (self.state)(resource, self.cache)
    }
}
