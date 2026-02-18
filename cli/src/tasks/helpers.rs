//! Helper utilities for common task patterns.
//!
//! This module provides abstractions for recurring patterns in task implementations:
//! - Config-based batch processing (symlinks, packages, extensions, etc.)
//! - Stats tracking and completion
//! - Dry-run state checking

use super::{Context, TaskResult, TaskStats};

/// Helper for tasks that process a batch of config items.
///
/// This encapsulates the common pattern of:
/// 1. Creating `TaskStats`
/// 2. Iterating over config items
/// 3. Checking current state vs desired state
/// 4. Logging appropriately for dry-run vs actual execution
/// 5. Finalizing with `stats.finish()`
///
/// # Example
///
/// ```rust,ignore
/// use crate::tasks::helpers::ConfigBatchProcessor;
///
/// let mut processor = ConfigBatchProcessor::new();
/// for item in &ctx.config.items {
///     if is_already_ok(item) {
///         ctx.log.debug(&format!("ok: {item} (already ok)"));
///         processor.stats.already_ok += 1;
///     } else if ctx.dry_run {
///         ctx.log.dry_run(&format!("would process: {item}"));
///         processor.stats.changed += 1;
///     } else {
///         // Do actual work
///         process_item(item)?;
///         processor.stats.changed += 1;
///     }
/// }
/// Ok(processor.finish(ctx))
/// ```
pub struct ConfigBatchProcessor {
    pub stats: TaskStats,
}

impl ConfigBatchProcessor {
    /// Create a new batch processor with empty stats.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stats: TaskStats::new(),
        }
    }

    /// Finalize the batch processing and return appropriate `TaskResult`.
    ///
    /// This logs the summary and returns:
    /// - `TaskResult::DryRun` if `ctx.dry_run` is true
    /// - `TaskResult::Ok` otherwise
    #[must_use]
    pub fn finish(self, ctx: &Context) -> TaskResult {
        self.stats.finish(ctx)
    }
}

impl Default for ConfigBatchProcessor {
    fn default() -> Self {
        Self::new()
    }
}

/// Log that an item is already in the desired state.
///
/// This is a common pattern across many tasks.
#[allow(dead_code)]
pub fn log_already_ok(ctx: &Context, item: &str) {
    ctx.log
        .debug(&format!("ok: {item} (already in desired state)"));
}

/// Log that an item would be changed in dry-run mode.
#[allow(dead_code)]
pub fn log_would_change(ctx: &Context, item: &str, action: &str) {
    ctx.log.dry_run(&format!("would {action}: {item}"));
}

/// Log that an item was successfully changed.
#[allow(dead_code)]
pub fn log_changed(ctx: &Context, item: &str, action: &str) {
    ctx.log.debug(&format!("{action}: {item}"));
}

/// Log that an item is being skipped.
#[allow(dead_code)]
pub fn log_skipped(ctx: &Context, item: &str, reason: &str) {
    ctx.log.debug(&format!("skip: {item} ({reason})"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::config::manifest::Manifest;
    use crate::config::profiles::Profile;
    use crate::logging::Logger;
    use crate::platform::{Os, Platform};

    fn test_ctx(log: &Logger, dry_run: bool) -> Context {
        let config = Config {
            root: std::path::PathBuf::from("/tmp/test"),
            profile: Profile {
                name: "base".to_string(),
                active_categories: vec!["base".to_string()],
                excluded_categories: Vec::new(),
            },
            packages: Vec::new(),
            symlinks: Vec::new(),
            registry: Vec::new(),
            units: Vec::new(),
            chmod: Vec::new(),
            vscode_extensions: Vec::new(),
            copilot_skills: Vec::new(),
            manifest: Manifest {
                excluded_files: Vec::new(),
            },
        };
        let platform = Platform::new(Os::Linux, false);
        let config = Box::leak(Box::new(config));
        let platform = Box::leak(Box::new(platform));
        Context {
            config,
            platform,
            log,
            dry_run,
            home: std::path::PathBuf::from("/tmp"),
        }
    }

    #[test]
    fn processor_new() {
        let processor = ConfigBatchProcessor::new();
        assert_eq!(processor.stats.changed, 0);
        assert_eq!(processor.stats.already_ok, 0);
        assert_eq!(processor.stats.skipped, 0);
    }

    #[test]
    fn processor_finish_ok() {
        let log = Logger::new(false);
        let ctx = test_ctx(&log, false);
        let processor = ConfigBatchProcessor::new();
        let result = processor.finish(&ctx);
        assert!(matches!(result, TaskResult::Ok));
    }

    #[test]
    fn processor_finish_dry_run() {
        let log = Logger::new(false);
        let ctx = test_ctx(&log, true);
        let processor = ConfigBatchProcessor::new();
        let result = processor.finish(&ctx);
        assert!(matches!(result, TaskResult::DryRun));
    }

    #[test]
    fn processor_with_stats() {
        let log = Logger::new(false);
        let ctx = test_ctx(&log, false);
        let mut processor = ConfigBatchProcessor::new();
        processor.stats.changed = 3;
        processor.stats.already_ok = 5;
        let result = processor.finish(&ctx);
        assert!(matches!(result, TaskResult::Ok));
    }
}
