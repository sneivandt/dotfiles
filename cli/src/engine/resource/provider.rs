//! Resource state discovery providers.

use anyhow::Result;

use super::{Resource, ResourceState};

/// Provides current state for a batch of resources.
///
/// Implementations may either use no cache (for intrinsic checks) or load a
/// shared cache once and reuse it for every resource in the batch.
pub trait ResourceStateProvider<R: Resource> {
    /// Cached state shared across all resources in this batch.
    type Cache: Sync;

    /// Load shared state for this batch.
    ///
    /// # Errors
    ///
    /// Returns an error if the state cache cannot be loaded.
    fn load(&self, resources: &[R]) -> Result<Self::Cache>;

    /// Determine the current state for one resource.
    ///
    /// # Errors
    ///
    /// Returns an error if the resource state cannot be determined.
    fn current_state(&self, resource: &R, cache: &Self::Cache) -> Result<ResourceState>;
}

/// State-checking extension for resources that can inspect themselves.
///
/// This is bridged into the orchestration layer by [`IntrinsicStateProvider`].
pub trait IntrinsicState: Resource {
    /// Check the current state of the resource.
    ///
    /// # Errors
    ///
    /// Returns an error if the resource state cannot be determined due to I/O failures,
    /// permission issues, or other system errors.
    fn current_state(&self) -> Result<ResourceState>;

    /// Determine if the resource needs to be changed.
    ///
    /// # Errors
    ///
    /// Returns an error if the current state cannot be determined (propagates errors from
    /// `current_state()`).
    #[allow(dead_code, reason = "part of trait contract; used by test modules")]
    fn needs_change(&self) -> Result<bool> {
        Ok(matches!(
            self.current_state()?,
            ResourceState::Missing | ResourceState::Incorrect { .. }
        ))
    }
}

/// State provider for resources that implement [`IntrinsicState`].
#[derive(Debug, Clone, Copy, Default)]
pub struct IntrinsicStateProvider;

impl<R: IntrinsicState> ResourceStateProvider<R> for IntrinsicStateProvider {
    type Cache = ();

    fn load(&self, _resources: &[R]) -> Result<Self::Cache> {
        Ok(())
    }

    fn current_state(&self, resource: &R, _cache: &Self::Cache) -> Result<ResourceState> {
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
    State: Fn(&R, &Cache) -> Result<ResourceState> + Sync,
{
    type Cache = ();

    fn load(&self, _resources: &[R]) -> Result<Self::Cache> {
        Ok(())
    }

    fn current_state(&self, resource: &R, _cache: &Self::Cache) -> Result<ResourceState> {
        (self.state)(resource, self.cache)
    }
}
