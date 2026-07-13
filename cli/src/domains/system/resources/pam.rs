//! PAM service file resource.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};

use crate::engine::resource::ResourceError;
use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::runtime::exec::Executor;

const PAM_DIR: &str = "/etc/pam.d";

/// A managed PAM service file under `/etc/pam.d`.
#[derive(Debug)]
pub struct PamServiceResource {
    /// PAM service name, e.g. `"hyprlock"`.
    pub name: String,
    /// Exact desired file content.
    pub content: String,
    target_dir: PathBuf,
    executor: Arc<dyn Executor>,
}

impl PamServiceResource {
    /// Create a new PAM service resource for `/etc/pam.d/<name>`.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        content: impl Into<String>,
        executor: Arc<dyn Executor>,
    ) -> Self {
        Self::with_target_dir(name, content, PathBuf::from(PAM_DIR), executor)
    }

    /// Create a new PAM service resource from a config entry.
    #[must_use]
    pub fn from_entry(
        entry: &crate::domains::system::config::pam::PamService,
        executor: Arc<dyn Executor>,
    ) -> Self {
        Self::new(entry.name.clone(), entry.content.clone(), executor)
    }

    /// Create a new PAM service resource with an explicit target directory.
    #[must_use]
    pub fn with_target_dir(
        name: impl Into<String>,
        content: impl Into<String>,
        target_dir: impl Into<PathBuf>,
        executor: Arc<dyn Executor>,
    ) -> Self {
        Self {
            name: name.into(),
            content: content.into(),
            target_dir: target_dir.into(),
            executor,
        }
    }

    fn target_path(&self) -> PathBuf {
        self.target_dir.join(&self.name)
    }

    fn tmp_path(&self) -> PathBuf {
        std::env::temp_dir().join(format!("dotfiles-pam-{}-{}", self.name, std::process::id()))
    }

    fn validate_name(&self) -> Result<()> {
        if self.name.trim().is_empty()
            || matches!(self.name.as_str(), "." | "..")
            || self.name.chars().any(|c| matches!(c, '/' | '\\' | '\0'))
        {
            return Err(ResourceError::not_supported(format!(
                "invalid PAM service name '{}'",
                self.name
            ))
            .into());
        }
        Ok(())
    }

    fn write_with_sudo(&self, target: &Path) -> ResourceResult<()> {
        let tmp = self.tmp_path();
        std::fs::write(&tmp, &self.content)
            .with_context(|| format!("write temp PAM file: {}", tmp.display()))?;
        let _cleanup = crate::runtime::fs::TempPath::new(tmp.clone());

        let tmp_arg = tmp.to_string_lossy();
        let target_arg = target.to_string_lossy();
        self.executor
            .run("sudo", &["install", "-m", "0644", &tmp_arg, &target_arg])?;
        Ok(())
    }
}

impl Resource for PamServiceResource {
    fn description(&self) -> String {
        self.target_path().display().to_string()
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        self.validate_name()?;
        let target = self.target_path();

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }

        match std::fs::write(&target, &self.content) {
            Ok(()) => Ok(ResourceChange::Applied),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                self.write_with_sudo(&target)?;
                Ok(ResourceChange::Applied)
            }
            Err(e) => Err(anyhow::Error::new(e)
                .context(format!("write {}", target.display()))
                .into()),
        }
    }
}

impl IntrinsicState for PamServiceResource {
    fn current_state(&self) -> Result<ResourceState> {
        if let Err(e) = self.validate_name() {
            return Ok(ResourceState::Invalid {
                reason: e.to_string(),
            });
        }

        let target = self.target_path();
        match std::fs::read_to_string(&target) {
            Ok(content) if content == self.content => Ok(ResourceState::Correct),
            Ok(content) => Ok(ResourceState::Incorrect {
                current: summarize_content(&content),
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ResourceState::Missing),
            Err(e) => Err(e).with_context(|| format!("read {}", target.display())),
        }
    }
}

fn summarize_content(content: &str) -> String {
    content
        .lines()
        .next()
        .map_or_else(|| "(empty)".to_string(), str::to_string)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::runtime::exec::{Executor, SystemExecutor};

    fn executor() -> Arc<dyn Executor> {
        Arc::new(SystemExecutor)
    }

    #[test]
    fn current_state_reports_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let resource = PamServiceResource::with_target_dir(
            "hyprlock",
            "auth include login\n",
            dir.path(),
            executor(),
        );

        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    }

    #[test]
    fn current_state_reports_correct_content() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hyprlock"), "auth include login\n").unwrap();
        let resource = PamServiceResource::with_target_dir(
            "hyprlock",
            "auth include login\n",
            dir.path(),
            executor(),
        );

        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn apply_writes_desired_content() {
        let dir = tempfile::tempdir().unwrap();
        let resource = PamServiceResource::with_target_dir(
            "hyprlock",
            "auth include login\n",
            dir.path(),
            executor(),
        );

        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("hyprlock")).unwrap(),
            "auth include login\n"
        );
    }

    #[test]
    fn invalid_service_name_is_invalid_state() {
        let dir = tempfile::tempdir().unwrap();
        let resource = PamServiceResource::with_target_dir(
            "../hyprlock",
            "auth include login\n",
            dir.path(),
            executor(),
        );

        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Invalid { .. }
        ));
    }
}
