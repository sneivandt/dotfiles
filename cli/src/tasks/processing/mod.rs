//! Generic resource processing loop: check state, apply or remove, collect stats.
//!
//! This module is split into sub-modules:
//!
//! - [`apply`] — single-resource processing (`process_single`, `apply_resource`, `remove_single`)
//! - [`context`] — shared execution context for tasks
//! - [`parallel`] — Rayon-based parallel processing helpers

mod apply;
pub mod context;
mod parallel;

pub use context::Context;

use anyhow::Result;

use crate::resources::{Resource, ResourceState};

/// Result of a single task execution.
///
/// # Examples
///
/// ```
/// use dotfiles_cli::tasks::TaskResult;
///
/// let ok = TaskResult::Ok;
/// let skipped = TaskResult::Skipped("not on arch".into());
/// let dry = TaskResult::DryRun;
///
/// assert!(matches!(ok, TaskResult::Ok));
/// assert!(matches!(skipped, TaskResult::Skipped(_)));
/// assert!(matches!(dry, TaskResult::DryRun));
/// ```
#[derive(Debug, Clone)]
pub enum TaskResult {
    /// Task completed successfully.
    Ok,
    /// Task was skipped (not applicable to this platform/profile).
    Skipped(String),
    /// Task ran in dry-run mode.
    DryRun,
}

/// Counters for batch tasks that process many items.
///
/// Provides consistent summary logging across all tasks.
///
/// # Examples
///
/// ```
/// use dotfiles_cli::tasks::TaskStats;
///
/// let mut stats = TaskStats::new();
/// stats.changed = 3;
/// stats.already_ok = 10;
///
/// assert_eq!(stats.summary(false), "3 changed, 10 already ok");
/// assert_eq!(stats.summary(true), "3 would change, 10 already ok");
/// ```
///
/// When items are skipped, the summary includes the count:
///
/// ```
/// use dotfiles_cli::tasks::TaskStats;
///
/// let stats = TaskStats { changed: 1, already_ok: 2, skipped: 3 };
/// assert_eq!(stats.summary(false), "1 changed, 2 already ok, 3 skipped");
/// ```
#[derive(Debug, Default)]
pub struct TaskStats {
    /// Number of items changed or applied.
    pub changed: u32,
    /// Number of items already in the correct state.
    pub already_ok: u32,
    /// Number of items skipped due to errors or inapplicability.
    pub skipped: u32,
}

impl TaskStats {
    /// Create a new empty stats counter.
    ///
    /// # Examples
    ///
    /// ```
    /// use dotfiles_cli::tasks::TaskStats;
    ///
    /// let stats = TaskStats::new();
    /// assert_eq!(stats.changed, 0);
    /// assert_eq!(stats.already_ok, 0);
    /// assert_eq!(stats.skipped, 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Format the summary string (e.g. "3 changed, 10 already ok, 1 skipped").
    ///
    /// # Examples
    ///
    /// ```
    /// use dotfiles_cli::tasks::TaskStats;
    ///
    /// let stats = TaskStats { changed: 5, already_ok: 12, skipped: 0 };
    /// assert_eq!(stats.summary(false), "5 changed, 12 already ok");
    /// assert_eq!(stats.summary(true), "5 would change, 12 already ok");
    /// ```
    #[must_use]
    pub fn summary(&self, dry_run: bool) -> String {
        let verb = if dry_run { "would change" } else { "changed" };
        if self.skipped > 0 {
            format!(
                "{} {verb}, {} already ok, {} skipped",
                self.changed, self.already_ok, self.skipped
            )
        } else {
            format!("{} {verb}, {} already ok", self.changed, self.already_ok)
        }
    }

    /// Log the summary and return the appropriate `TaskResult`.
    #[must_use]
    pub fn finish(self, ctx: &Context) -> TaskResult {
        ctx.log.info(&self.summary(ctx.dry_run));
        if ctx.dry_run {
            TaskResult::DryRun
        } else {
            TaskResult::Ok
        }
    }
}

impl std::ops::AddAssign for TaskStats {
    fn add_assign(&mut self, other: Self) {
        self.changed += other.changed;
        self.already_ok += other.already_ok;
        self.skipped += other.skipped;
    }
}

/// Configuration for the generic resource processing loop.
///
/// Controls how each [`ResourceState`] variant is handled.
///
/// Use the named constructors [`apply_all`](Self::apply_all) and
/// [`install_missing`](Self::install_missing) for common presets, with
/// optional modifiers like [`no_bail`](Self::no_bail) and
/// [`skip_missing`](Self::skip_missing).
///
/// # Examples
///
/// ```
/// use dotfiles_cli::tasks::ProcessOpts;
///
/// // Fix everything, bail on errors (strict):
/// let opts = ProcessOpts::apply_all("link");
/// assert!(opts.fix_incorrect && opts.fix_missing && opts.bail_on_error);
///
/// // Fix everything, warn on errors (lenient):
/// let opts = ProcessOpts::apply_all("install").no_bail();
/// assert!(opts.fix_incorrect && opts.fix_missing && !opts.bail_on_error);
///
/// // Install only missing resources (lenient):
/// let opts = ProcessOpts::install_missing("enable");
/// assert!(!opts.fix_incorrect && opts.fix_missing && !opts.bail_on_error);
///
/// // Fix existing only, bail on errors:
/// let opts = ProcessOpts::apply_all("chmod").skip_missing();
/// assert!(opts.fix_incorrect && !opts.fix_missing && opts.bail_on_error);
/// ```
#[derive(Debug)]
pub struct ProcessOpts<'a> {
    /// Verb for log messages (e.g., "install", "link", "chmod").
    pub verb: &'a str,
    /// Treat `Incorrect` as fixable (apply the change). If `false`, skip it.
    pub fix_incorrect: bool,
    /// Treat `Missing` as fixable (apply the change). If `false`, skip it.
    pub fix_missing: bool,
    /// Propagate errors from `apply()` (bail). If `false`, warn and count as skipped.
    pub bail_on_error: bool,
}

impl<'a> ProcessOpts<'a> {
    /// Fix both missing and incorrect resources, bailing on errors.
    ///
    /// This is the strict default — suitable for resources where every
    /// failure must be surfaced (e.g. symlinks, hooks, git config).
    #[must_use]
    pub const fn apply_all(verb: &'a str) -> Self {
        Self {
            verb,
            fix_incorrect: true,
            fix_missing: true,
            bail_on_error: true,
        }
    }

    /// Install only missing resources, warning on errors instead of bailing.
    ///
    /// Suitable for resources that should not be overwritten when already
    /// present (e.g. VS Code extensions, systemd units, Copilot skills).
    #[must_use]
    pub const fn install_missing(verb: &'a str) -> Self {
        Self {
            verb,
            fix_incorrect: false,
            fix_missing: true,
            bail_on_error: false,
        }
    }

    /// Warn on errors instead of bailing.
    #[must_use]
    pub const fn no_bail(mut self) -> Self {
        self.bail_on_error = false;
        self
    }

    /// Skip missing resources (only fix incorrect ones).
    #[must_use]
    pub const fn skip_missing(mut self) -> Self {
        self.fix_missing = false;
        self
    }
}

/// Process resources by checking each one's current state and applying as needed.
///
/// For tasks where each resource can independently determine its own state via
/// `resource.current_state()`.
///
/// # Errors
///
/// Returns an error if any resource fails to check its state or apply changes,
/// depending on the `bail_on_error` setting in `opts`. If `bail_on_error` is `false`,
/// errors are logged as warnings instead.
pub fn process_resources<R: Resource + Send>(
    ctx: &Context,
    resources: impl IntoIterator<Item = R>,
    opts: &ProcessOpts,
) -> Result<TaskResult> {
    let resources: Vec<R> = resources.into_iter().collect();
    if ctx.parallel && resources.len() > 1 {
        ctx.log.debug(&format!(
            "processing {} resources in parallel",
            resources.len()
        ));
        parallel::process_resources_parallel(ctx, resources, opts)
    } else {
        let mut stats = TaskStats::new();
        for resource in resources {
            let current = resource.current_state()?;
            stats += apply::process_single(ctx, &resource, current, opts)?;
        }
        Ok(stats.finish(ctx))
    }
}

/// Process resources with pre-computed states.
///
/// For tasks that batch-query state (e.g., registry, packages, VS Code extensions)
/// and then iterate with cached results.
///
/// # Errors
///
/// Returns an error if any resource fails to apply changes, depending on the
/// `bail_on_error` setting in `opts`. If `bail_on_error` is `false`, errors are
/// logged as warnings instead.
pub fn process_resource_states<R: Resource + Send>(
    ctx: &Context,
    resource_states: impl IntoIterator<Item = (R, ResourceState)>,
    opts: &ProcessOpts,
) -> Result<TaskResult> {
    let resource_states: Vec<(R, ResourceState)> = resource_states.into_iter().collect();
    if ctx.parallel && resource_states.len() > 1 {
        ctx.log.debug(&format!(
            "processing {} resources in parallel",
            resource_states.len()
        ));
        parallel::process_resource_states_parallel(ctx, resource_states, opts)
    } else {
        let mut stats = TaskStats::new();
        for (resource, current) in resource_states {
            stats += apply::process_single(ctx, &resource, current, opts)?;
        }
        Ok(stats.finish(ctx))
    }
}

/// Process resources for removal.
///
/// Only resources in [`ResourceState::Correct`] are removed (they are "ours").
/// Resources that are `Missing`, `Incorrect`, or `Invalid` are skipped.
///
/// When `ctx.parallel` is `true` and there is more than one resource, removal
/// runs in parallel using Rayon (matching the behaviour of [`process_resources`]
/// and [`process_resource_states`]).
///
/// # Errors
///
/// Returns an error if a resource fails to check its current state or fails
/// during the removal process.
pub fn process_resources_remove<R: Resource + Send>(
    ctx: &Context,
    resources: impl IntoIterator<Item = R>,
    verb: &str,
) -> Result<TaskResult> {
    let resources: Vec<R> = resources.into_iter().collect();
    if ctx.parallel && resources.len() > 1 {
        ctx.log.debug(&format!(
            "processing {} resources in parallel",
            resources.len()
        ));
        parallel::process_remove_parallel(ctx, resources, verb)
    } else {
        let mut stats = TaskStats::new();
        for resource in resources {
            let current = resource.current_state()?;
            stats += apply::remove_single(ctx, &resource, &current, verb)?;
        }
        Ok(stats.finish(ctx))
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::resources::{Resource, ResourceChange, ResourceState};
    use crate::tasks::test_helpers::{empty_config, make_static_context};
    use std::path::PathBuf;

    // -----------------------------------------------------------------------
    // Test doubles
    // -----------------------------------------------------------------------

    /// A configurable mock resource for testing the processing pipeline.
    struct MockResource {
        state_result: Result<ResourceState, String>,
        apply_result: Result<ResourceChange, String>,
        remove_result: Result<ResourceChange, String>,
        desc: String,
    }

    impl MockResource {
        fn new(state: ResourceState) -> Self {
            Self {
                state_result: Ok(state),
                apply_result: Ok(ResourceChange::Applied),
                remove_result: Ok(ResourceChange::Applied),
                desc: "mock resource".to_string(),
            }
        }

        fn with_state_error(mut self, err: impl Into<String>) -> Self {
            self.state_result = Err(err.into());
            self
        }

        fn with_apply(mut self, result: Result<ResourceChange, String>) -> Self {
            self.apply_result = result;
            self
        }

        fn with_remove(mut self, result: Result<ResourceChange, String>) -> Self {
            self.remove_result = result;
            self
        }
    }

    impl Resource for MockResource {
        fn description(&self) -> String {
            self.desc.clone()
        }

        fn current_state(&self) -> Result<ResourceState> {
            self.state_result
                .clone()
                .map_err(|s| anyhow::anyhow!("{s}"))
        }

        fn apply(&self) -> Result<ResourceChange> {
            self.apply_result
                .clone()
                .map_err(|s| anyhow::anyhow!("{s}"))
        }

        fn remove(&self) -> Result<ResourceChange> {
            self.remove_result
                .clone()
                .map_err(|s| anyhow::anyhow!("{s}"))
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn test_context(
        config: crate::config::Config,
    ) -> (Context, std::sync::Arc<crate::logging::Logger>) {
        make_static_context(config)
    }

    fn dry_run_context(
        config: crate::config::Config,
    ) -> (Context, std::sync::Arc<crate::logging::Logger>) {
        let (mut ctx, log) = test_context(config);
        ctx.dry_run = true;
        (ctx, log)
    }

    fn parallel_context(
        config: crate::config::Config,
    ) -> (Context, std::sync::Arc<crate::logging::Logger>) {
        let (mut ctx, log) = test_context(config);
        ctx.parallel = true;
        (ctx, log)
    }

    fn default_opts() -> ProcessOpts<'static> {
        ProcessOpts::apply_all("install").no_bail()
    }

    fn bail_opts() -> ProcessOpts<'static> {
        ProcessOpts::apply_all("install")
    }

    // -----------------------------------------------------------------------
    // TaskStats
    // -----------------------------------------------------------------------

    #[test]
    fn stats_summary_changed_only() {
        let stats = TaskStats {
            changed: 3,
            already_ok: 0,
            skipped: 0,
        };
        assert_eq!(stats.summary(false), "3 changed, 0 already ok");
    }

    #[test]
    fn stats_summary_dry_run() {
        let stats = TaskStats {
            changed: 2,
            already_ok: 5,
            skipped: 0,
        };
        assert_eq!(stats.summary(true), "2 would change, 5 already ok");
    }

    #[test]
    fn stats_summary_with_skipped() {
        let stats = TaskStats {
            changed: 1,
            already_ok: 2,
            skipped: 3,
        };
        assert_eq!(stats.summary(false), "1 changed, 2 already ok, 3 skipped");
    }

    #[test]
    fn stats_finish_returns_dry_run_result() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = dry_run_context(config);
        let stats = TaskStats::new();
        let result = stats.finish(&ctx);
        assert!(matches!(result, TaskResult::DryRun));
    }

    #[test]
    fn stats_finish_returns_ok_result() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        let stats = TaskStats::new();
        let result = stats.finish(&ctx);
        assert!(matches!(result, TaskResult::Ok));
    }

    // -----------------------------------------------------------------------
    // process_single
    // -----------------------------------------------------------------------

    #[test]
    fn process_single_correct_increments_already_ok() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        let resource = MockResource::new(ResourceState::Correct);
        let opts = default_opts();

        let stats = apply::process_single(&ctx, &resource, ResourceState::Correct, &opts).unwrap();

        assert_eq!(stats.already_ok, 1);
        assert_eq!(stats.changed, 0);
        assert_eq!(stats.skipped, 0);
    }

    #[test]
    fn process_single_invalid_increments_skipped() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        let resource = MockResource::new(ResourceState::Invalid {
            reason: "test".to_string(),
        });
        let opts = default_opts();

        let stats = apply::process_single(
            &ctx,
            &resource,
            ResourceState::Invalid {
                reason: "test".to_string(),
            },
            &opts,
        )
        .unwrap();

        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.changed, 0);
    }

    #[test]
    fn process_single_missing_skips_when_fix_missing_false() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        let resource = MockResource::new(ResourceState::Missing);
        let opts = ProcessOpts {
            fix_missing: false,
            ..default_opts()
        };

        let stats = apply::process_single(&ctx, &resource, ResourceState::Missing, &opts).unwrap();

        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.changed, 0);
    }

    #[test]
    fn process_single_incorrect_skips_when_fix_incorrect_false() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        let resource = MockResource::new(ResourceState::Incorrect {
            current: "wrong".to_string(),
        });
        let opts = ProcessOpts {
            fix_incorrect: false,
            ..default_opts()
        };

        let stats = apply::process_single(
            &ctx,
            &resource,
            ResourceState::Incorrect {
                current: "wrong".to_string(),
            },
            &opts,
        )
        .unwrap();

        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.changed, 0);
    }

    #[test]
    fn process_single_missing_applies_and_increments_changed() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        let resource = MockResource::new(ResourceState::Missing);
        let opts = default_opts();

        let stats = apply::process_single(&ctx, &resource, ResourceState::Missing, &opts).unwrap();

        assert_eq!(stats.changed, 1);
        assert_eq!(stats.already_ok, 0);
    }

    #[test]
    fn process_single_incorrect_applies_and_increments_changed() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        let resource = MockResource::new(ResourceState::Incorrect {
            current: "wrong".to_string(),
        });
        let opts = default_opts();

        let stats = apply::process_single(
            &ctx,
            &resource,
            ResourceState::Incorrect {
                current: "wrong".to_string(),
            },
            &opts,
        )
        .unwrap();

        assert_eq!(stats.changed, 1);
    }

    #[test]
    fn process_single_dry_run_missing_increments_changed_without_apply() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = dry_run_context(config);
        // Apply would error if called — but dry-run should skip it
        let resource =
            MockResource::new(ResourceState::Missing).with_apply(Err("should not call".into()));
        let opts = default_opts();

        let stats = apply::process_single(&ctx, &resource, ResourceState::Missing, &opts).unwrap();

        assert_eq!(stats.changed, 1);
    }

    #[test]
    fn process_single_dry_run_incorrect_increments_changed() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = dry_run_context(config);
        let resource = MockResource::new(ResourceState::Incorrect {
            current: "old-value".to_string(),
        });
        let opts = default_opts();

        let stats = apply::process_single(
            &ctx,
            &resource,
            ResourceState::Incorrect {
                current: "old-value".to_string(),
            },
            &opts,
        )
        .unwrap();

        assert_eq!(stats.changed, 1);
    }

    // -----------------------------------------------------------------------
    // apply_resource
    // -----------------------------------------------------------------------

    #[test]
    fn apply_resource_applied_increments_changed() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        let resource = MockResource::new(ResourceState::Missing);
        let opts = default_opts();

        let stats = apply::apply_resource(&ctx, &resource, &opts).unwrap();

        assert_eq!(stats.changed, 1);
    }

    #[test]
    fn apply_resource_already_correct_increments_already_ok() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        let resource = MockResource::new(ResourceState::Missing)
            .with_apply(Ok(ResourceChange::AlreadyCorrect));
        let opts = default_opts();

        let stats = apply::apply_resource(&ctx, &resource, &opts).unwrap();

        assert_eq!(stats.already_ok, 1);
        assert_eq!(stats.changed, 0);
    }

    #[test]
    fn apply_resource_skipped_no_bail_increments_skipped() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        let resource =
            MockResource::new(ResourceState::Missing).with_apply(Ok(ResourceChange::Skipped {
                reason: "not supported".to_string(),
            }));
        let opts = default_opts();

        let stats = apply::apply_resource(&ctx, &resource, &opts).unwrap();

        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.changed, 0);
    }

    #[test]
    fn apply_resource_error_no_bail_increments_skipped() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        let resource =
            MockResource::new(ResourceState::Missing).with_apply(Err("boom".to_string()));
        let opts = default_opts();

        let stats = apply::apply_resource(&ctx, &resource, &opts).unwrap();

        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.changed, 0);
    }

    #[test]
    fn apply_resource_bail_on_applied() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        let resource = MockResource::new(ResourceState::Missing);
        let opts = bail_opts();

        let stats = apply::apply_resource(&ctx, &resource, &opts).unwrap();

        assert_eq!(stats.changed, 1);
    }

    #[test]
    fn apply_resource_bail_on_already_correct() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        let resource = MockResource::new(ResourceState::Missing)
            .with_apply(Ok(ResourceChange::AlreadyCorrect));
        let opts = bail_opts();

        let stats = apply::apply_resource(&ctx, &resource, &opts).unwrap();

        assert_eq!(stats.already_ok, 1);
    }

    #[test]
    fn apply_resource_bail_on_skipped_returns_error() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        let resource =
            MockResource::new(ResourceState::Missing).with_apply(Ok(ResourceChange::Skipped {
                reason: "denied".to_string(),
            }));
        let opts = bail_opts();

        let err = apply::apply_resource(&ctx, &resource, &opts);
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("denied"));
    }

    #[test]
    fn apply_resource_bail_on_error_propagates() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        let resource =
            MockResource::new(ResourceState::Missing).with_apply(Err("critical".to_string()));
        let opts = bail_opts();

        let err = apply::apply_resource(&ctx, &resource, &opts);
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("critical"));
    }

    // -----------------------------------------------------------------------
    // process_resources
    // -----------------------------------------------------------------------

    #[test]
    fn process_resources_mixed_states() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        let resources = vec![
            MockResource::new(ResourceState::Correct),
            MockResource::new(ResourceState::Missing),
            MockResource::new(ResourceState::Invalid {
                reason: "bad".to_string(),
            }),
        ];
        let opts = default_opts();

        let result = process_resources(&ctx, resources, &opts).unwrap();
        assert!(matches!(result, TaskResult::Ok));
    }

    #[test]
    fn process_resources_empty_list() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        let resources: Vec<MockResource> = vec![];
        let opts = default_opts();

        let result = process_resources(&ctx, resources, &opts).unwrap();
        assert!(matches!(result, TaskResult::Ok));
    }

    // -----------------------------------------------------------------------
    // process_resource_states
    // -----------------------------------------------------------------------

    #[test]
    fn process_resource_states_applies_precomputed() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        let resource_states = vec![
            (
                MockResource::new(ResourceState::Missing),
                ResourceState::Missing,
            ),
            (
                MockResource::new(ResourceState::Correct),
                ResourceState::Correct,
            ),
        ];
        let opts = default_opts();

        let result = process_resource_states(&ctx, resource_states, &opts).unwrap();
        assert!(matches!(result, TaskResult::Ok));
    }

    // -----------------------------------------------------------------------
    // process_resources_remove
    // -----------------------------------------------------------------------

    #[test]
    fn process_resources_remove_removes_correct_resources() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        let resources = vec![
            MockResource::new(ResourceState::Correct),
            MockResource::new(ResourceState::Missing),
        ];

        let result = process_resources_remove(&ctx, resources, "unlink").unwrap();
        assert!(matches!(result, TaskResult::Ok));
    }

    #[test]
    fn process_resources_remove_dry_run() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = dry_run_context(config);
        // Remove should NOT be called in dry-run
        let resources = vec![
            MockResource::new(ResourceState::Correct).with_remove(Err("should not call".into())),
        ];

        let result = process_resources_remove(&ctx, resources, "unlink").unwrap();
        assert!(matches!(result, TaskResult::DryRun));
    }

    // -----------------------------------------------------------------------
    // Parallel dispatch — process_resources
    // -----------------------------------------------------------------------

    #[test]
    fn process_resources_parallel_accumulates_stats() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = parallel_context(config);
        // Three resources: one already correct, one missing (will be applied), one invalid (skipped).
        let resources = vec![
            MockResource::new(ResourceState::Correct),
            MockResource::new(ResourceState::Missing),
            MockResource::new(ResourceState::Invalid {
                reason: "bad".to_string(),
            }),
        ];
        let opts = default_opts();

        let result = process_resources(&ctx, resources, &opts).unwrap();
        assert!(matches!(result, TaskResult::Ok));
    }

    #[test]
    fn process_resources_parallel_single_resource_runs_sequentially() {
        // When there is only one resource, the sequential path is taken even if parallel=true.
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = parallel_context(config);
        let resources = vec![MockResource::new(ResourceState::Missing)];
        let opts = default_opts();

        let result = process_resources(&ctx, resources, &opts).unwrap();
        assert!(matches!(result, TaskResult::Ok));
    }

    #[test]
    fn process_resources_parallel_bail_on_error_propagates() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = parallel_context(config);
        let resources = vec![
            MockResource::new(ResourceState::Missing).with_apply(Err("fatal".into())),
            MockResource::new(ResourceState::Missing).with_apply(Err("fatal".into())),
        ];
        let opts = bail_opts();

        let result = process_resources(&ctx, resources, &opts);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Parallel dispatch — process_resource_states
    // -----------------------------------------------------------------------

    #[test]
    fn process_resource_states_parallel_dispatch() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = parallel_context(config);
        let resource_states = vec![
            (
                MockResource::new(ResourceState::Missing),
                ResourceState::Missing,
            ),
            (
                MockResource::new(ResourceState::Correct),
                ResourceState::Correct,
            ),
            (
                MockResource::new(ResourceState::Incorrect {
                    current: "old".to_string(),
                }),
                ResourceState::Incorrect {
                    current: "old".to_string(),
                },
            ),
        ];
        let opts = default_opts();

        let result = process_resource_states(&ctx, resource_states, &opts).unwrap();
        assert!(matches!(result, TaskResult::Ok));
    }

    // -----------------------------------------------------------------------
    // Parallel dispatch — process_resources_remove
    // -----------------------------------------------------------------------

    #[test]
    fn process_resources_remove_parallel_dispatch() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = parallel_context(config);
        let resources = vec![
            MockResource::new(ResourceState::Correct),
            MockResource::new(ResourceState::Missing),
        ];

        let result = process_resources_remove(&ctx, resources, "unlink").unwrap();
        assert!(matches!(result, TaskResult::Ok));
    }

    // -----------------------------------------------------------------------
    // Error propagation — current_state() failures
    // -----------------------------------------------------------------------

    #[test]
    fn process_resources_current_state_error_propagates() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        let resources =
            vec![MockResource::new(ResourceState::Missing).with_state_error("state failed")];
        let opts = default_opts();

        let result = process_resources(&ctx, resources, &opts);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("state failed"));
    }

    #[test]
    fn process_resources_remove_current_state_error_propagates() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        let resources =
            vec![MockResource::new(ResourceState::Missing).with_state_error("state failed")];

        let result = process_resources_remove(&ctx, resources, "unlink");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("state failed"));
    }

    // -----------------------------------------------------------------------
    // End-to-end bail-on-error through process_resources (sequential)
    // -----------------------------------------------------------------------

    #[test]
    fn process_resources_bail_on_apply_error_propagates() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        let resources =
            vec![MockResource::new(ResourceState::Missing).with_apply(Err("fatal".into()))];
        let opts = bail_opts();

        let result = process_resources(&ctx, resources, &opts);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("fatal"));
    }

    // -----------------------------------------------------------------------
    // Stats accumulation across multiple resources in process_resource_states
    // -----------------------------------------------------------------------

    #[test]
    fn process_resource_states_stats_accumulate_across_resources() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _log) = test_context(config);
        // 2 correct, 1 missing (applied), 1 invalid (skipped)
        let resource_states = vec![
            (
                MockResource::new(ResourceState::Correct),
                ResourceState::Correct,
            ),
            (
                MockResource::new(ResourceState::Correct),
                ResourceState::Correct,
            ),
            (
                MockResource::new(ResourceState::Missing),
                ResourceState::Missing,
            ),
            (
                MockResource::new(ResourceState::Invalid {
                    reason: "bad".to_string(),
                }),
                ResourceState::Invalid {
                    reason: "bad".to_string(),
                },
            ),
        ];
        let opts = default_opts();

        // Just verify it succeeds — individual counts are exercised by process_single tests
        let result = process_resource_states(&ctx, resource_states, &opts).unwrap();
        assert!(matches!(result, TaskResult::Ok));
    }
}
