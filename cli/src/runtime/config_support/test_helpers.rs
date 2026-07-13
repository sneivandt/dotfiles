//! Test helpers for configuration loader tests.

use std::path::PathBuf;

/// Write content to a temp TOML file and return the temp dir + path.
/// The `TempDir` must be kept alive for the file to persist during the test.
///
/// # Panics
///
/// Panics if the temp directory or file cannot be created.
#[must_use]
#[allow(
    clippy::expect_used,
    reason = "panicking allowed at this trust boundary"
)]
pub fn write_temp_toml(content: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let path = dir.path().join("test.toml");
    std::fs::write(&path, content).expect("failed to write temp toml");
    (dir, path)
}

/// Assert that a config loader returns an empty list for a missing file.
///
/// Eliminates the repeated pattern of creating a temp dir, pointing at a
/// nonexistent file, calling the loader, and asserting the result is empty.
///
/// # Panics
///
/// Panics if the temp directory cannot be created or the loader fails.
#[allow(
    clippy::expect_used,
    reason = "panicking allowed at this trust boundary"
)]
pub fn assert_load_missing_returns_empty<T>(
    loader: impl Fn(
        &std::path::Path,
        &[crate::runtime::config_support::category_matcher::Category],
    ) -> anyhow::Result<Vec<T>>,
) {
    use crate::runtime::config_support::category_matcher::Category;
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let path = dir.path().join("nonexistent.toml");
    let result = loader(&path, &[Category::Base]).expect("loader should not fail");
    assert!(result.is_empty(), "missing file should produce empty list");
}

/// Assert that an unfiltered config loader returns an empty list for a
/// missing file.
///
/// # Panics
///
/// Panics if the temp directory cannot be created or the loader fails.
#[allow(
    clippy::expect_used,
    reason = "panicking allowed at this trust boundary"
)]
pub fn assert_load_missing_unfiltered_returns_empty<T>(
    loader: impl Fn(&std::path::Path) -> anyhow::Result<Vec<T>>,
) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let path = dir.path().join("nonexistent.toml");
    let result = loader(&path).expect("loader should not fail");
    assert!(result.is_empty(), "missing file should produce empty list");
}
