//! Git configuration resource.
use anyhow::{Context as _, Result};
use std::path::PathBuf;

use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};

/// A git config entry resource that can be checked and applied.
///
/// Uses the `git2` crate to read and write global git configuration natively,
/// without shelling out to `git config`.
#[derive(Debug)]
pub struct GitConfigResource {
    /// Config key (e.g., "core.autocrlf").
    pub key: String,
    /// Desired value (e.g., "false").
    pub desired_value: String,
    config_path: Option<PathBuf>,
}

impl GitConfigResource {
    /// Create a new git config resource.
    #[must_use]
    pub const fn new(key: String, desired_value: String) -> Self {
        Self {
            key,
            desired_value,
            config_path: None,
        }
    }

    /// Create a resource backed by one explicit config file.
    #[must_use]
    pub(crate) const fn with_config_path(
        key: String,
        desired_value: String,
        config_path: PathBuf,
    ) -> Self {
        Self {
            key,
            desired_value,
            config_path: Some(config_path),
        }
    }

    fn open_config(&self) -> Result<git2::Config> {
        self.config_path.as_deref().map_or_else(
            || {
                let config = git2::Config::open_default().context("opening git config")?;
                config
                    .open_level(git2::ConfigLevel::Global)
                    .context("opening global git config")
            },
            |path| {
                git2::Config::open(path)
                    .with_context(|| format!("opening git config {}", path.display()))
            },
        )
    }

    /// Check resource state against a pre-opened config snapshot.
    ///
    /// This enables unit testing without touching the real global git config.
    fn state_from_config(&self, config: &git2::Config) -> Result<ResourceState> {
        match config.get_string(&self.key) {
            Ok(ref current) if current == &self.desired_value => Ok(ResourceState::Correct),
            Ok(current) => Ok(ResourceState::Incorrect { current }),
            Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(ResourceState::Missing),
            Err(e) => {
                Err(anyhow::Error::from(e).context(format!("reading git config {}", self.key)))
            }
        }
    }

    /// Apply config change to a mutable config handle.
    ///
    /// This enables unit testing without touching the real global git config.
    fn apply_to_config(&self, config: &mut git2::Config) -> Result<ResourceChange> {
        config
            .set_str(&self.key, &self.desired_value)
            .with_context(|| format!("setting {} = {}", self.key, self.desired_value))?;
        Ok(ResourceChange::Applied)
    }
}

impl Resource for GitConfigResource {
    fn description(&self) -> String {
        format!("{} = {}", self.key, self.desired_value)
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        let mut config = self.open_config()?;
        self.apply_to_config(&mut config).map_err(Into::into)
    }
}

impl IntrinsicState for GitConfigResource {
    fn current_state(&self) -> Result<ResourceState> {
        let config = self.open_config()?;
        self.state_from_config(&config)
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;

    #[test]
    fn description_format() {
        let resource = GitConfigResource::new("core.autocrlf".to_string(), "false".to_string());
        assert_eq!(resource.description(), "core.autocrlf = false");
    }

    // ------------------------------------------------------------------
    // state_from_config
    // ------------------------------------------------------------------

    #[test]
    fn state_correct_when_value_matches() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        let mut config = git2::Config::open(&path).unwrap();
        config.set_str("core.autocrlf", "false").unwrap();

        let resource = GitConfigResource::new("core.autocrlf".to_string(), "false".to_string());
        assert_eq!(
            resource.state_from_config(&config).unwrap(),
            ResourceState::Correct
        );
    }

    #[test]
    fn state_missing_when_key_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        let config = git2::Config::open(&path).unwrap();

        let resource = GitConfigResource::new("core.autocrlf".to_string(), "false".to_string());
        assert_eq!(
            resource.state_from_config(&config).unwrap(),
            ResourceState::Missing
        );
    }

    #[test]
    fn state_incorrect_when_value_differs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        let mut config = git2::Config::open(&path).unwrap();
        config.set_str("core.autocrlf", "true").unwrap();

        let resource = GitConfigResource::new("core.autocrlf".to_string(), "false".to_string());
        let state = resource.state_from_config(&config).unwrap();
        assert!(
            matches!(state, ResourceState::Incorrect { ref current } if current == "true"),
            "expected Incorrect(true), got {state:?}"
        );
    }

    // ------------------------------------------------------------------
    // apply_to_config
    // ------------------------------------------------------------------

    #[test]
    fn state_checks_global_level_only() {
        // Simulate a local config shadowing a global value. When a local
        // config has a different value, current_state should still report
        // the global-level value since apply() writes to global only.
        let dir = tempfile::tempdir().unwrap();
        let global_path = dir.path().join("global");
        let local_path = dir.path().join("local");

        // Set global to the desired value
        let mut global_cfg = git2::Config::open(&global_path).unwrap();
        global_cfg.set_str("core.autocrlf", "false").unwrap();

        // Set local to a different value (this would shadow global in a
        // merged config)
        let mut local_cfg = git2::Config::open(&local_path).unwrap();
        local_cfg.set_str("core.autocrlf", "true").unwrap();

        // state_from_config with only the global config should see "false"
        let resource = GitConfigResource::new("core.autocrlf".to_string(), "false".to_string());
        assert_eq!(
            resource.state_from_config(&global_cfg).unwrap(),
            ResourceState::Correct,
            "checking only global level should report Correct when global matches"
        );
    }

    // ------------------------------------------------------------------
    // apply_to_config
    // ------------------------------------------------------------------

    #[test]
    fn apply_sets_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        let mut config = git2::Config::open(&path).unwrap();

        let resource = GitConfigResource::new("core.autocrlf".to_string(), "false".to_string());
        assert_eq!(
            resource.apply_to_config(&mut config).unwrap(),
            ResourceChange::Applied
        );

        let val = config.get_string("core.autocrlf").unwrap();
        assert_eq!(val, "false");
    }

    #[test]
    fn explicit_config_path_is_used_for_state_and_apply() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        let resource = GitConfigResource::with_config_path(
            "core.autocrlf".to_string(),
            "false".to_string(),
            path.clone(),
        );

        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);

        let config = git2::Config::open(&path).unwrap();
        assert_eq!(config.get_string("core.autocrlf").unwrap(), "false");
    }
}
