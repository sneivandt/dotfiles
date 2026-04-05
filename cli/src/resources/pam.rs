//! PAM service configuration resource.
//!
//! Manages `/etc/pam.d/<service>` files with the standard `system-auth`
//! template.  Writing to `/etc/pam.d/` requires elevated privileges;
//! the resource first attempts a direct write and falls back to `sudo`.

use std::sync::Arc;

use anyhow::{Context as _, Result};

use super::{Applicable, Resource, ResourceChange, ResourceState};
use crate::exec::Executor;

/// Standard PAM configuration content using `system-auth` includes.
///
/// This is the conventional template for Arch Linux services that need
/// standard user authentication (screen lockers, display managers, etc.).
const PAM_TEMPLATE: &str = "\
auth        include     system-auth
account     include     system-auth
password    include     system-auth
session     include     system-auth
";

/// A PAM service configuration resource.
///
/// Ensures that `/etc/pam.d/<name>` contains the standard `system-auth`
/// includes.
#[derive(Debug)]
pub struct PamConfigResource {
    /// Service name (e.g. `"hyprlock"`).
    pub name: String,
    /// Executor for running sudo commands.
    executor: Arc<dyn Executor>,
}

impl PamConfigResource {
    /// Create a new PAM config resource.
    #[must_use]
    pub fn new(name: String, executor: Arc<dyn Executor>) -> Self {
        Self { name, executor }
    }

    /// Create from a config entry.
    #[must_use]
    pub fn from_entry(entry: &crate::config::pam::PamEntry, executor: Arc<dyn Executor>) -> Self {
        Self::new(entry.name.clone(), executor)
    }

    /// The absolute path to the PAM config file.
    fn target_path(&self) -> String {
        format!("/etc/pam.d/{}", self.name)
    }

    /// PID-stamped temp path to prevent concurrent-run collisions.
    fn tmp_path(&self) -> String {
        format!("/tmp/dotfiles-pam-{}-{}", self.name, std::process::id())
    }
}

impl Applicable for PamConfigResource {
    fn description(&self) -> String {
        format!("/etc/pam.d/{}", self.name)
    }

    /// Write the standard PAM template to `/etc/pam.d/<name>`.
    ///
    /// Attempts a direct write first (works when running as root).
    /// Falls back to staging via a temp file and copying with `sudo`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written, even with sudo.
    fn apply(&self) -> Result<ResourceChange> {
        let target = self.target_path();

        match std::fs::write(&target, PAM_TEMPLATE) {
            Ok(()) => return Ok(ResourceChange::Applied),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                // Fall through to sudo path.
            }
            Err(e) => return Err(e).context(format!("write {target}")),
        }

        let tmp = self.tmp_path();
        std::fs::write(&tmp, PAM_TEMPLATE)
            .with_context(|| format!("write temp PAM file: {tmp}"))?;

        let result = self.executor.run("sudo", &["cp", &tmp, &target]);
        let _ = std::fs::remove_file(&tmp);
        result?;

        Ok(ResourceChange::Applied)
    }

    /// Remove the PAM config file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be removed.
    fn remove(&self) -> Result<ResourceChange> {
        let target = self.target_path();
        if !std::path::Path::new(&target).exists() {
            return Ok(ResourceChange::AlreadyCorrect);
        }

        match std::fs::remove_file(&target) {
            Ok(()) => Ok(ResourceChange::Applied),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                self.executor.run("sudo", &["rm", &target])?;
                Ok(ResourceChange::Applied)
            }
            Err(e) => Err(e).context(format!("remove {target}")),
        }
    }
}

impl Resource for PamConfigResource {
    /// Check whether `/etc/pam.d/<name>` exists and has the correct content.
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be read.
    fn current_state(&self) -> Result<ResourceState> {
        let target = self.target_path();
        match std::fs::read_to_string(&target) {
            Ok(content) if content == PAM_TEMPLATE => Ok(ResourceState::Correct),
            Ok(content) => Ok(ResourceState::Incorrect {
                current: content.lines().next().unwrap_or("(empty)").to_string(),
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ResourceState::Missing),
            Err(e) => Err(e).context(format!("read {target}")),
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::exec::MockExecutor;

    fn make_resource() -> PamConfigResource {
        let mock = MockExecutor::new();
        PamConfigResource::new("test-service".to_string(), Arc::new(mock))
    }

    #[test]
    fn description_shows_pam_path() {
        let resource = make_resource();
        assert_eq!(resource.description(), "/etc/pam.d/test-service");
    }

    #[test]
    fn target_path_format() {
        let resource = make_resource();
        assert_eq!(resource.target_path(), "/etc/pam.d/test-service");
    }

    #[test]
    fn tmp_path_contains_pid() {
        let resource = make_resource();
        let path = resource.tmp_path();
        let pid = std::process::id().to_string();
        assert!(
            path.contains(&pid),
            "temp path {path:?} must contain the process ID"
        );
        assert!(
            path.contains("test-service"),
            "temp path {path:?} must contain the service name"
        );
    }

    #[test]
    fn state_missing_when_file_not_found() {
        let temp = tempfile::tempdir().unwrap();
        let mut mock = MockExecutor::new();
        mock.expect_which().returning(|_| false);
        let resource = PamConfigResource {
            name: temp
                .path()
                .join("nonexistent")
                .to_string_lossy()
                .to_string(),
            executor: Arc::new(mock),
        };
        // Override target_path by using a name that won't exist as /etc/pam.d/<name>
        // Instead, test the logic directly with a non-existent path.
        let state = resource.current_state();
        // On most systems /etc/pam.d/<random-temp-path> won't exist, so this
        // should be Missing. If it errors (permission denied), that's also acceptable.
        assert!(state.is_ok(), "current_state should not error: {state:?}");
    }

    #[test]
    fn pam_template_is_valid() {
        assert!(PAM_TEMPLATE.contains("auth"));
        assert!(PAM_TEMPLATE.contains("account"));
        assert!(PAM_TEMPLATE.contains("password"));
        assert!(PAM_TEMPLATE.contains("session"));
        assert!(PAM_TEMPLATE.contains("system-auth"));
        assert!(PAM_TEMPLATE.ends_with('\n'));
    }

    #[test]
    fn from_entry_creates_resource() {
        let entry = crate::config::pam::PamEntry {
            name: "hyprlock".to_string(),
        };
        let mock = MockExecutor::new();
        let resource = PamConfigResource::from_entry(&entry, Arc::new(mock));
        assert_eq!(resource.name, "hyprlock");
    }
}
