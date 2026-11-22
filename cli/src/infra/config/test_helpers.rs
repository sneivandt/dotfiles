//! Test helpers for configuration loader tests.

use super::category_matcher::Category;
use super::{Diagnostic, DiagnosticCode, Severity};
use std::fmt::Debug;
use std::path::{Path, PathBuf};

/// Write content to a temp TOML file and return the temp dir + path.
/// The `TempDir` must be kept alive for the file to persist during the test.
///
/// # Panics
///
/// Panics if the temp directory or file cannot be created.
#[must_use]
pub fn write_temp_toml(content: &str) -> (tempfile::TempDir, PathBuf) {
    write_named_toml("test.toml", content)
}

pub(crate) fn write_named_toml(name: &str, content: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let path = dir.path().join(name);
    std::fs::write(&path, content).expect("failed to write temp toml");
    (dir, path)
}

/// Assert that a config loader returns an empty list for a missing file.
///
/// # Panics
///
/// Panics if the temp directory cannot be created or the loader fails.
pub fn assert_load_missing_returns_empty<T>(
    loader: impl Fn(&Path, &[Category]) -> anyhow::Result<Vec<T>>,
) {
    assert_load_missing_unfiltered_returns_empty(|path| loader(path, &[Category::Base]));
}

/// Assert that an unfiltered config loader returns an empty list for a
/// missing file.
///
/// # Panics
///
/// Panics if the temp directory cannot be created or the loader fails.
pub fn assert_load_missing_unfiltered_returns_empty<T>(
    loader: impl Fn(&Path) -> anyhow::Result<Vec<T>>,
) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let path = dir.path().join("nonexistent.toml");
    let result = loader(&path).expect("loader should not fail");
    assert!(result.is_empty(), "missing file should produce empty list");
}

/// Keep each rejected document individually runnable. An optional `=> fragment`
/// checks the full error chain (the loader wraps serde errors with file context).
macro_rules! rejects_toml {
    ($filename:literal, $load:expr; $($case:ident: $toml:literal $(=> $fragment:literal)?;)+) => {
        $(
            #[test]
            fn $case() {
                $crate::infra::config::test_helpers::assert_rejected(
                    $filename, $load, $toml, &[$($fragment)?],
                );
            }
        )+
    };
}

pub(crate) use rejects_toml;

#[track_caller]
pub(crate) fn assert_rejected<T: Debug>(
    filename: &str,
    load: impl FnOnce(&Path) -> anyhow::Result<T>,
    toml: &str,
    fragments: &[&str],
) {
    let (_dir, path) = write_named_toml(filename, toml);
    let error = load(&path).expect_err("loader accepted invalid TOML");
    assert_mentions(&format!("{error:#}"), fragments);
}

#[track_caller]
pub(crate) fn assert_mentions(message: &str, fragments: &[&str]) {
    for fragment in fragments {
        assert!(
            message.contains(fragment),
            "missing {fragment:?}: {message}"
        );
    }
}

/// Some rules can report additional findings; only enforce a count when specified.
#[track_caller]
pub(crate) fn assert_messages(
    diagnostics: &[Diagnostic],
    count: Option<usize>,
    fragments: &[&str],
) {
    if let Some(count) = count {
        assert_eq!(diagnostics.len(), count, "{diagnostics:?}");
    }
    for fragment in fragments {
        assert!(
            diagnostics.iter().any(|d| d.message.contains(fragment)),
            "missing {fragment:?}: {diagnostics:?}"
        );
    }
}

/// Check the zero-or-one conflict contract; return the conflict for domain-specific
/// checks on declaration locations, identity and sensitive values.
#[track_caller]
pub(crate) fn assert_conflict(
    case: impl Debug,
    diagnostics: &[Diagnostic],
    expected: bool,
    code: DiagnosticCode,
) -> Option<&Diagnostic> {
    assert_eq!(
        diagnostics.len(),
        usize::from(expected),
        "{case:?}: {diagnostics:?}"
    );
    let diagnostic = diagnostics.first()?;
    assert_eq!(diagnostic.code, code, "{case:?}");
    assert_eq!(diagnostic.severity, Severity::Error, "{case:?}");
    Some(diagnostic)
}
