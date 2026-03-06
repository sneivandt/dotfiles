//! Task: write /etc/wsl.conf with the desired network settings.

use anyhow::Result;

use super::{Context, Task, TaskResult};

/// The single setting this task enforces.
const DESIRED_KEY: &str = "generateResolvConf = true";
const DESIRED_SECTION: &str = "[network]";

/// Desired content for /etc/wsl.conf.
const DESIRED_CONTENT: &str = "[network]\ngenerateResolvConf = true\n";

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

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.is_wsl()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let target = "/etc/wsl.conf";

        // Already correct — skip.
        if is_correct(target) {
            return Ok(TaskResult::Ok);
        }

        if ctx.dry_run {
            ctx.log
                .info(&format!("[dry-run] would write {DESIRED_KEY} to {target}"));
            return Ok(TaskResult::Ok);
        }

        ctx.log.info(&format!("writing {DESIRED_KEY} to {target}"));

        // Try a direct write first (works when running as root).  If that
        // fails with a permission error, fall back to staging via a temp file
        // and copying into place with sudo.
        match std::fs::write(target, DESIRED_CONTENT) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                ctx.log.info("direct write failed, falling back to sudo");
                let tmp = "/tmp/dotfiles-wsl.conf";
                std::fs::write(tmp, DESIRED_CONTENT)
                    .map_err(|e| anyhow::anyhow!("failed to write temp file {tmp}: {e}"))?;

                ctx.executor.run("sudo", &["cp", tmp, target])?;
                let _ = std::fs::remove_file(tmp);
            }
            Err(e) => return Err(anyhow::anyhow!("failed to write {target}: {e}")),
        }

        Ok(TaskResult::Ok)
    }
}

/// Returns true if /etc/wsl.conf already contains the desired setting.
fn is_correct(path: &str) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    let mut in_network = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_network = trimmed == DESIRED_SECTION;
        } else if in_network && trimmed == DESIRED_KEY {
            return true;
        }
    }
    false
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
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
        assert!(is_correct_from_str(
            "[network]\ngenerateResolvConf = true\n"
        ));
    }

    #[test]
    fn is_correct_ignores_setting_outside_network_section() {
        assert!(!is_correct_from_str("[boot]\ngenerateResolvConf = true\n"));
    }

    #[test]
    fn is_correct_false_when_missing() {
        assert!(!is_correct_from_str("[network]\n"));
    }

    /// Helper: write content to a temp file and run `is_correct` against it.
    fn is_correct_from_str(content: &str) -> bool {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), content).unwrap();
        is_correct(&tmp.path().to_string_lossy())
    }
}
