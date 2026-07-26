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
    reason = "test helper: setup failures should abort the calling test"
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
    reason = "test helper: setup failures should abort the calling test"
)]
pub fn assert_load_missing_returns_empty<T>(
    loader: impl Fn(
        &std::path::Path,
        &[crate::infra::config::category_matcher::Category],
    ) -> anyhow::Result<Vec<T>>,
) {
    use crate::infra::config::category_matcher::Category;
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
    reason = "test helper: setup failures should abort the calling test"
)]
pub fn assert_load_missing_unfiltered_returns_empty<T>(
    loader: impl Fn(&std::path::Path) -> anyhow::Result<Vec<T>>,
) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let path = dir.path().join("nonexistent.toml");
    let result = loader(&path).expect("loader should not fail");
    assert!(result.is_empty(), "missing file should produce empty list");
}

/// Assert that a category-filtered loader rejects `content`, naming
/// `expected_fragment` in the reported error.
///
/// Guards the invariant that a misspelled key is a hard parse error rather
/// than a silently discarded value.
///
/// # Panics
///
/// Panics if the temp file cannot be written, the loader succeeds, or the
/// error does not mention `expected_fragment`.
#[allow(
    clippy::expect_used,
    reason = "test helper: setup failures should abort the calling test"
)]
pub fn assert_load_rejects<T>(
    loader: impl Fn(
        &std::path::Path,
        &[crate::infra::config::category_matcher::Category],
    ) -> anyhow::Result<T>,
    content: &str,
    expected_fragment: &str,
) {
    use crate::infra::config::category_matcher::Category;
    let (_dir, path) = write_temp_toml(content);
    let error = loader(&path, &[Category::Base])
        .err()
        .expect("loader should reject the document instead of ignoring the key");
    assert_error_mentions(&error, expected_fragment);
}

/// Assert that an unfiltered loader rejects `content`, naming
/// `expected_fragment` in the reported error.
///
/// # Panics
///
/// Panics if the temp file cannot be written, the loader succeeds, or the
/// error does not mention `expected_fragment`.
#[allow(
    clippy::expect_used,
    reason = "test helper: setup failures should abort the calling test"
)]
pub fn assert_load_unfiltered_rejects<T>(
    loader: impl Fn(&std::path::Path) -> anyhow::Result<T>,
    content: &str,
    expected_fragment: &str,
) {
    let (_dir, path) = write_temp_toml(content);
    let error = loader(&path)
        .err()
        .expect("loader should reject the document instead of ignoring the key");
    assert_error_mentions(&error, expected_fragment);
}

/// Assert that `error` or any of its sources mentions `fragment`.
///
/// Loader errors are wrapped with file context, so the underlying serde
/// message is only reachable through the source chain.
///
/// # Panics
///
/// Panics if no error in the chain mentions `fragment`.
fn assert_error_mentions(error: &anyhow::Error, fragment: &str) {
    let rendered = format!("{error:#}");
    assert!(
        rendered.contains(fragment),
        "error should mention {fragment:?}, got: {rendered}"
    );
}
