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
    desired: Option<String>,
    config_path: Option<PathBuf>,
}

impl GitConfigResource {
    /// Create a new git config resource.
    #[must_use]
    pub const fn new(key: String, desired_value: String) -> Self {
        Self {
            key,
            desired: Some(desired_value),
            config_path: None,
        }
    }

    /// Create a resource that removes a global Git setting.
    #[must_use]
    pub const fn absent(key: String) -> Self {
        Self {
            key,
            desired: None,
            config_path: None,
        }
    }

    /// Create a resource backed by one explicit config file.
    #[must_use]
    pub const fn with_config_path(
        key: String,
        desired_value: String,
        config_path: PathBuf,
    ) -> Self {
        Self {
            key,
            desired: Some(desired_value),
            config_path: Some(config_path),
        }
    }

    /// Scope either setting or removal to an explicit file.
    #[must_use]
    pub(crate) fn using_config_path(mut self, path: PathBuf) -> Self {
        self.config_path = Some(path);
        self
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
    fn state_from_config(&self, config: &git2::Config) -> ResourceResult<ResourceState> {
        let current = config.get_string(&self.key);
        match (self.desired.as_deref(), current) {
            (Some(desired), Ok(current)) if current == desired => Ok(ResourceState::Correct),
            (_, Ok(current)) => Ok(ResourceState::Incorrect { current }),
            (desired, Err(error)) if error.code() == git2::ErrorCode::NotFound => {
                Ok(if desired.is_some() {
                    ResourceState::Missing
                } else {
                    ResourceState::Correct
                })
            }
            (_, Err(error)) => Err(anyhow::Error::from(error)
                .context(format!("reading git config {}", self.key))
                .into()),
        }
    }

    /// Apply config change to a mutable config handle.
    ///
    /// This enables unit testing without touching the real global git config.
    fn apply_to_config(&self, config: &mut git2::Config) -> Result<ResourceChange> {
        match &self.desired {
            Some(desired) => config
                .set_multivar(&self.key, ".*", desired)
                .with_context(|| format!("setting {} = {desired}", self.key))?,
            None => config
                .remove_multivar(&self.key, ".*")
                .with_context(|| format!("removing git config {}", self.key))?,
        }
        Ok(ResourceChange::Applied)
    }
}

impl Resource for GitConfigResource {
    fn description(&self) -> String {
        self.desired.as_ref().map_or_else(
            || format!("{} is unset", self.key),
            |desired| format!("{} = {desired}", self.key),
        )
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        let mut config = self
            .open_config()
            .with_context(|| format!("setting git config {}", self.key))?;
        Ok(self.apply_to_config(&mut config)?)
    }
}

impl IntrinsicState for GitConfigResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        let config = self
            .open_config()
            .with_context(|| format!("reading git config {}", self.key))?;
        self.state_from_config(&config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_failure_names_the_key() {
        let dir = tempfile::tempdir().unwrap();
        // A directory can never be opened as a config file.
        let resource = GitConfigResource::with_config_path(
            "core.autocrlf".to_string(),
            "false".to_string(),
            dir.path().to_path_buf(),
        );

        let error = resource
            .apply()
            .expect_err("opening a directory as a config file must fail");

        let rendered = format!("{:#}", anyhow::Error::new(error));
        assert!(
            rendered.contains("core.autocrlf"),
            "error must name the failing key, got: {rendered}"
        );
    }

    #[test]
    fn description_format() {
        let resource = GitConfigResource::new("core.autocrlf".to_string(), "false".to_string());
        assert_eq!(resource.description(), "core.autocrlf = false");

        let absent = GitConfigResource::absent("core.autocrlf".to_string());
        assert_eq!(absent.description(), "core.autocrlf is unset");
    }

    // ------------------------------------------------------------------
    // state_from_config
    // ------------------------------------------------------------------

    #[test]
    fn configured_value_state_matches_current_config() {
        for (label, current, expected) in [
            ("matching value", Some("false"), ResourceState::Correct),
            ("missing key", None, ResourceState::Missing),
            (
                "different value",
                Some("true"),
                ResourceState::Incorrect {
                    current: "true".to_string(),
                },
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mut config = git2::Config::open(&dir.path().join("config")).unwrap();
            if let Some(value) = current {
                config.set_str("core.autocrlf", value).unwrap();
            }
            let resource = GitConfigResource::new("core.autocrlf".to_string(), "false".to_string());
            assert_eq!(
                resource.state_from_config(&config).unwrap(),
                expected,
                "{label}"
            );
        }
    }

    #[test]
    fn absent_state_is_correct_only_when_key_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        let mut config = git2::Config::open(&path).unwrap();
        let resource = GitConfigResource::absent("core.autocrlf".to_string());

        assert_eq!(
            resource.state_from_config(&config).unwrap(),
            ResourceState::Correct
        );

        config.set_str("core.autocrlf", "false").unwrap();
        assert_eq!(
            resource.state_from_config(&config).unwrap(),
            ResourceState::Incorrect {
                current: "false".to_string()
            }
        );
    }

    // ------------------------------------------------------------------
    // apply_to_config
    // ------------------------------------------------------------------

    #[test]
    fn repeated_git_config_values_converge_for_setting_and_removal() {
        for desired in [Some("false"), None] {
            let dir = tempfile::tempdir_in(".").unwrap();
            let path = dir.path().join("config");
            std::fs::write(
                &path,
                "[core]\n\tautocrlf = false\n\tautocrlf = true\n\teditor = fixture-editor\n",
            )
            .unwrap();
            let resource = desired
                .map_or_else(
                    || GitConfigResource::absent("core.autocrlf".into()),
                    |value| GitConfigResource::new("core.autocrlf".into(), value.into()),
                )
                .using_config_path(path.clone());

            assert!(
                matches!(
                    resource.current_state().unwrap(),
                    ResourceState::Incorrect { .. }
                ),
                "duplicate settings are not yet in the desired state {desired:?}"
            );
            assert_eq!(
                resource.apply().unwrap(),
                ResourceChange::Applied,
                "must converge even when Git has repeated values for {desired:?}"
            );
            assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);

            let observed = git2::Config::open(&path).unwrap();
            assert_eq!(
                observed.get_string("core.editor").unwrap(),
                "fixture-editor"
            );
            if let Some(value) = desired {
                let mut entries = observed.multivar("core.autocrlf", None).unwrap();
                let mut values = Vec::new();
                while let Some(entry) = entries.next() {
                    values.push(entry.unwrap().value().unwrap().to_string());
                }
                assert!(!values.is_empty());
                assert!(values.iter().all(|current| current == value), "{values:?}");
            } else {
                assert_eq!(
                    observed.get_string("core.autocrlf").unwrap_err().code(),
                    git2::ErrorCode::NotFound
                );
            }
        }
    }

    #[test]
    fn malformed_config_is_rejected_without_rewriting_user_content() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let path = dir.path().join("config");
        let content = "[core\n  autocrlf = true\n";
        std::fs::write(&path, content).unwrap();
        let resource = GitConfigResource::with_config_path(
            "core.autocrlf".into(),
            "false".into(),
            path.clone(),
        );

        for error in [
            resource.current_state().unwrap_err(),
            resource.apply().unwrap_err(),
        ] {
            assert!(format!("{error:#}").contains("core.autocrlf"), "{error:#}");
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
    }

    #[test]
    fn apply_removes_obsolete_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        let mut config = git2::Config::open(&path).unwrap();
        config.set_str("core.autocrlf", "false").unwrap();
        config.set_str("core.editor", "fixture-editor").unwrap();
        let resource = GitConfigResource::absent("core.autocrlf".to_string());

        assert_eq!(
            resource.apply_to_config(&mut config).unwrap(),
            ResourceChange::Applied
        );
        assert_eq!(
            config.get_string("core.autocrlf").unwrap_err().code(),
            git2::ErrorCode::NotFound,
        );
        assert_eq!(config.get_string("core.editor").unwrap(), "fixture-editor");
        assert_eq!(
            resource.state_from_config(&config).unwrap(),
            ResourceState::Correct
        );
    }

    #[test]
    fn explicit_config_path_is_used_for_state_and_apply() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        let mut config = git2::Config::open(&path).unwrap();
        config.set_str("core.editor", "fixture-editor").unwrap();
        let resource = GitConfigResource::with_config_path(
            "core.autocrlf".to_string(),
            "false".to_string(),
            path.clone(),
        );

        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);

        let observed = git2::Config::open(&path).unwrap();
        assert_eq!(observed.get_string("core.autocrlf").unwrap(), "false");
        assert_eq!(
            observed.get_string("core.editor").unwrap(),
            "fixture-editor"
        );
    }
}
