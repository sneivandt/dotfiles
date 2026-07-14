//! Shared test fixtures for APM task tests.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::engine::Context;
use crate::runtime::exec::{ExecResult, MockExecutor};
use crate::runtime::platform::Platform;
use crate::test_helpers::{empty_config, make_context};

use super::fragments::{discover_fragment_files, merge_fragments};
use super::manifest::{manifest_fingerprint, write_manifest_marker};

/// Default APM fragment shared across APM test suites.
pub const DEFAULT_FRAGMENT: &str =
    "name: base\nversion: 1.0.0\ndependencies:\n  apm:\n    - example/plugin\n";

/// Build a successful [`ExecResult`] with the given stdout.
pub fn ok_result(stdout: &str) -> ExecResult {
    ExecResult {
        stdout: stdout.to_string(),
        stderr: String::new(),
        success: true,
        code: Some(0),
    }
}

/// Write `content` into `<home>/.apm/config/<filename>`.
pub fn write_home_fragment(home: &Path, filename: &str, content: &str) {
    let fragment_dir = home.join(".apm").join("config");
    std::fs::create_dir_all(&fragment_dir).expect("create fragment dir");
    std::fs::write(fragment_dir.join(filename), content).expect("write manifest fragment");
}

/// Write the default fragment into `<home>/.apm/config/base.yml`.
pub fn write_default_home_fragment(home: &Path) {
    write_home_fragment(home, "base.yml", DEFAULT_FRAGMENT);
}

/// Write the default fragment, merged manifest, and bare lockfile under `home`.
pub fn write_current_manifest_and_lock(home: &Path) {
    write_default_home_fragment(home);
    let fragments = discover_fragment_files(home).expect("discover fragments");
    let merged = merge_fragments(&fragments).expect("merge fragments");
    std::fs::write(home.join(".apm").join("apm.yml"), merged).expect("write manifest");
    std::fs::write(home.join(".apm").join("apm.lock.yaml"), "lock\n").expect("write lock");
}

/// Write the default fragment, merged manifest, lockfile, and success marker
/// under `home`.
pub fn write_current_manifest_lock_and_marker(home: &Path) {
    write_current_manifest_and_lock(home);
    let manifest =
        std::fs::read_to_string(home.join(".apm").join("apm.yml")).expect("read manifest");
    write_manifest_marker(
        &home.join(".apm").join(".dotfiles-manifest.sha256"),
        &manifest_fingerprint(&manifest),
    )
    .expect("write marker");
}

/// Create `<home>/.copilot/data.db` so the copilot-app target and autopilot
/// fixup are enabled.  Returns the path to the created file.
pub fn write_copilot_app_db(home: &Path) -> PathBuf {
    let copilot_dir = home.join(".copilot");
    std::fs::create_dir_all(&copilot_dir).expect("create .copilot dir");
    let db_path = copilot_dir.join("data.db");
    std::fs::write(&db_path, b"db").expect("write data.db");
    db_path
}

/// Build a [`Context`] rooted at `home` with the given platform and executor.
pub fn make_context_with_home(home: &Path, platform: Platform, executor: MockExecutor) -> Context {
    make_context(
        empty_config(home.to_path_buf()),
        platform,
        Arc::new(executor),
    )
    .with_home(home.to_path_buf())
}
