//! Task: write /etc/wsl.conf with the desired network settings.

use anyhow::Result;

use crate::platform::Platform;
use crate::tasks::{Context, Domain, ExecutionPolicy, Task, TaskPhase, TaskResult};

/// The single setting this task enforces.
const DESIRED_KEY: &str = "generateResolvConf = true";
const DESIRED_SECTION: &str = "[network]";

/// Desired content for /etc/wsl.conf.
const DESIRED_CONTENT: &str = "[network]\ngenerateResolvConf = true\n";
/// The single key name this task enforces inside `[network]`.
const DESIRED_KEY_NAME: &str = "generateResolvConf";

/// Write /etc/wsl.conf with `generateResolvConf = true` under `[network]`.
///
/// Only runs inside Windows Subsystem for Linux (WSL).  Writing /etc requires
/// elevated privileges when not running as root, so the task first attempts a
/// direct write and falls back to staging the file to a temp path and copying
/// into place via sudo.
#[derive(Debug)]
pub struct InstallWslConf;

impl Task for InstallWslConf {
    fn name(&self) -> &'static str {
        "Install wsl.conf"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Provision
    }

    fn domain(&self) -> Domain {
        Domain::System
    }

    fn execution_policies(&self) -> &[ExecutionPolicy] {
        const POLICIES: &[ExecutionPolicy] = &[
            ExecutionPolicy::PlatformSupported("WSL", Platform::is_wsl),
            ExecutionPolicy::RequiresElevation,
        ];
        POLICIES
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.is_wsl()
    }

    fn needs_elevation(&self, _ctx: &Context) -> bool {
        !is_correct("/etc/wsl.conf")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let target = "/etc/wsl.conf";

        // Already correct — skip.
        if is_correct(target) {
            ctx.log.debug("already configured, no action needed");
            return Ok(TaskResult::Ok);
        }

        if ctx.dry_run {
            ctx.log
                .dry_run(&format!("would update {DESIRED_KEY} in {target}"));
            return Ok(TaskResult::DryRun);
        }

        let desired_content = desired_content_for_path(target)?;

        ctx.log.info(&format!("updating {DESIRED_KEY} in {target}"));

        // Try a direct write first (works when running as root).  If that
        // fails with a permission error, fall back to staging via a temp file
        // and copying into place with sudo.  The temp path is unique per
        // process (PID-stamped) so concurrent runs do not race on the same
        // file and stale content from a previous failed run cannot interfere.
        match std::fs::write(target, &desired_content) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                ctx.log.info("direct write failed, falling back to sudo");
                let tmp = sudo_fallback_tmp_path();
                std::fs::write(&tmp, &desired_content)
                    .map_err(|e| anyhow::anyhow!("failed to write temp file {tmp}: {e}"))?;

                let result = ctx.executor.run("sudo", &["cp", &tmp, target]);
                drop(std::fs::remove_file(&tmp));
                result?;
            }
            Err(e) => return Err(anyhow::anyhow!("failed to write {target}: {e}")),
        }

        Ok(TaskResult::Ok)
    }
}

/// Returns the process-unique temp path used by the sudo fallback.
///
/// Using a PID-stamped name prevents two concurrent runs from racing on the
/// same file and prevents a stale file from a previous failed run from being
/// copied unexpectedly.
fn sudo_fallback_tmp_path() -> String {
    format!("/tmp/dotfiles-wsl-{}.conf", std::process::id())
}

/// Returns the desired /etc/wsl.conf content for a target path.
fn desired_content_for_path(path: &str) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(existing) => Ok(merge_desired_content(&existing)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(DESIRED_CONTENT.to_string()),
        Err(err) => Err(anyhow::anyhow!("failed to read {path}: {err}")),
    }
}

/// Merges `generateResolvConf = true` into the `[network]` section.
#[must_use]
fn merge_desired_content(existing: &str) -> String {
    if existing.is_empty() {
        return DESIRED_CONTENT.to_string();
    }

    let mut merged = String::new();
    let mut in_network = false;
    let mut saw_network = false;
    let mut saw_desired_key = false;

    for line in existing.lines() {
        let trimmed = line.trim();

        if is_section_header(trimmed) {
            if in_network && !saw_desired_key {
                merged.push_str(DESIRED_KEY);
                merged.push('\n');
            }

            in_network = trimmed == DESIRED_SECTION;
            if in_network {
                saw_network = true;
            }
            saw_desired_key = false;

            merged.push_str(line);
            merged.push('\n');
            continue;
        }

        if in_network && is_desired_key_line(trimmed) {
            merged.push_str(DESIRED_KEY);
            merged.push('\n');
            saw_desired_key = true;
            continue;
        }

        merged.push_str(line);
        merged.push('\n');
    }

    if in_network && !saw_desired_key {
        merged.push_str(DESIRED_KEY);
        merged.push('\n');
    }

    if saw_network {
        return merged;
    }

    if !merged.ends_with('\n') {
        merged.push('\n');
    }
    if !merged.ends_with("\n\n") {
        merged.push('\n');
    }
    merged.push_str(DESIRED_SECTION);
    merged.push('\n');
    merged.push_str(DESIRED_KEY);
    merged.push('\n');
    merged
}

/// Returns true if /etc/wsl.conf already contains the desired setting.
fn is_correct(path: &str) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    is_correct_content(&content)
}

#[must_use]
fn is_correct_content(content: &str) -> bool {
    let mut in_network = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if is_section_header(trimmed) {
            in_network = trimmed == DESIRED_SECTION;
        } else if in_network && is_desired_key_with_true_value(trimmed) {
            return true;
        }
    }
    false
}

#[must_use]
fn is_section_header(trimmed: &str) -> bool {
    trimmed.starts_with('[') && trimmed.ends_with(']')
}

#[must_use]
fn is_desired_key_line(trimmed: &str) -> bool {
    trimmed
        .split_once('=')
        .is_some_and(|(key, _)| key.trim() == DESIRED_KEY_NAME)
}

#[must_use]
fn is_desired_key_with_true_value(trimmed: &str) -> bool {
    trimmed
        .split_once('=')
        .is_some_and(|(key, value)| key.trim() == DESIRED_KEY_NAME && value.trim() == "true")
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::tasks::test_helpers::{ContextBuilder, empty_config};
    use std::path::PathBuf;

    #[test]
    fn should_run_false_on_windows() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = ContextBuilder::new(config)
            .os(crate::platform::Os::Windows)
            .which(true)
            .build();
        assert!(!InstallWslConf.should_run(&ctx));
    }

    #[test]
    fn should_run_false_on_linux_non_wsl() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = ContextBuilder::new(config)
            .os(crate::platform::Os::Linux)
            .which(true)
            .build();
        assert!(!InstallWslConf.should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_wsl() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = ContextBuilder::new(config)
            .os(crate::platform::Os::Linux)
            .wsl(true)
            .which(true)
            .build();
        assert!(InstallWslConf.should_run(&ctx));
    }

    #[test]
    fn is_correct_detects_setting_in_network_section() {
        assert!(is_correct_content("[network]\ngenerateResolvConf = true\n"));
    }

    #[test]
    fn is_correct_ignores_setting_outside_network_section() {
        assert!(!is_correct_content("[boot]\ngenerateResolvConf = true\n"));
    }

    #[test]
    fn is_correct_false_when_missing() {
        assert!(!is_correct_content("[network]\n"));
    }

    #[test]
    fn is_correct_false_when_true_value_is_outside_network_section() {
        assert!(!is_correct_content(
            "[network]\ngenerateResolvConf = false\n\n[boot]\ngenerateResolvConf = true\n"
        ));
    }

    #[test]
    fn merge_preserves_unrelated_sections_and_adds_network_key() {
        let existing = "[boot]\nsystemd=true\n\n[user]\ndefault=sneivandt\n\n[network]\nhostname=devbox\n\n[automount]\nenabled=true\n";
        let expected = "[boot]\nsystemd=true\n\n[user]\ndefault=sneivandt\n\n[network]\nhostname=devbox\n\ngenerateResolvConf = true\n[automount]\nenabled=true\n";
        assert_eq!(merge_desired_content(existing), expected);
    }

    #[test]
    fn merge_preserves_network_sibling_keys() {
        let existing = "[network]\nhostname=devbox\n";
        let expected = "[network]\nhostname=devbox\ngenerateResolvConf = true\n";
        assert_eq!(merge_desired_content(existing), expected);
    }

    #[test]
    fn merge_updates_existing_network_key() {
        let existing = "[network]\ngenerateResolvConf = false\nhostname=devbox\n";
        let expected = "[network]\ngenerateResolvConf = true\nhostname=devbox\n";
        assert_eq!(merge_desired_content(existing), expected);
    }

    #[test]
    fn merge_appends_missing_network_section() {
        let existing = "[boot]\nsystemd=true\n";
        let expected = "[boot]\nsystemd=true\n\n[network]\ngenerateResolvConf = true\n";
        assert_eq!(merge_desired_content(existing), expected);
    }

    #[test]
    fn merge_empty_file_becomes_minimal_desired_content() {
        assert_eq!(merge_desired_content(""), DESIRED_CONTENT);
    }

    #[test]
    fn desired_content_for_missing_file_is_minimal() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("missing-wsl.conf");
        assert_eq!(
            desired_content_for_path(&missing.to_string_lossy()).unwrap(),
            DESIRED_CONTENT
        );
    }

    #[test]
    fn sudo_fallback_tmp_path_contains_pid() {
        let path = sudo_fallback_tmp_path();
        let pid = std::process::id().to_string();
        assert!(
            path.contains(&pid),
            "temp path {path:?} must contain the process ID to prevent concurrent-run collisions"
        );
    }
}
