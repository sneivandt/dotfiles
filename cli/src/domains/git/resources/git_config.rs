//! Git configuration resource.
use anyhow::{Context as _, Result};
use std::path::PathBuf;

/// Stage the complete user configuration privately before editing it with libgit2.
fn stage_config(
    parent: &std::path::Path,
    contents: impl AsRef<[u8]>,
) -> Result<crate::infra::fs::TempGuard> {
    use std::io::Write as _;

    let (stage, mut file) = crate::infra::fs::TempGuard::create_unique_file_with_mode(
        parent,
        ".dotfiles-git",
        "config",
        0o600,
    )?;
    file.write_all(contents.as_ref())?;
    Ok(stage)
}

fn open_file(path: &std::path::Path) -> Result<git2::Config> {
    // libgit2's relative-include resolver recognizes '/' separators, including
    // on Windows. Native backslashes can otherwise make an include appear absent.
    #[cfg(windows)]
    {
        let normalized = path
            .to_str()
            .context("Git config path is not valid UTF-8")?
            .replace('\\', "/");
        Ok(git2::Config::open(std::path::Path::new(&normalized))?)
    }
    #[cfg(not(windows))]
    Ok(git2::Config::open(path)?)
}

use crate::engine::{
    AccessClaim, IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState,
};

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
                open_file(path).with_context(|| format!("opening git config {}", path.display()))
            },
        )
    }

    fn values_from_config(&self, config: &git2::Config) -> Result<Vec<Option<String>>> {
        (|| -> Result<Vec<Option<String>>> {
            let mut entries = config.multivar(&self.key, None)?;
            let mut values = Vec::new();
            while let Some(entry) = entries.next() {
                let entry = entry?;
                if entry.include_depth() != 0 {
                    continue;
                }
                values.push(if entry.has_value() {
                    Some(entry.value()?.to_string())
                } else {
                    None
                });
            }
            Ok(values)
        })()
        .with_context(|| format!("reading git config {}", self.key))
    }

    /// Check resource state against a pre-opened config snapshot.
    ///
    /// This enables unit testing without touching the real global git config.
    fn state_from_config(&self, config: &git2::Config) -> ResourceResult<ResourceState> {
        let values = self.values_from_config(config)?;
        if values.is_empty() {
            return Ok(if self.desired.is_some() {
                ResourceState::Missing
            } else {
                ResourceState::Correct
            });
        }
        Ok(
            if self.desired.as_ref().is_some_and(|desired| {
                values
                    .iter()
                    .all(|value| value.as_deref() == Some(desired.as_str()))
                    && config.get_entry(&self.key).is_ok_and(|entry| {
                        entry.has_value() && entry.value().is_ok_and(|value| value == desired)
                    })
            }) {
                ResourceState::Correct
            } else {
                ResourceState::Incorrect {
                    current: values
                        .iter()
                        .map(|value| value.as_deref().unwrap_or("<implicit true>"))
                        .collect::<Vec<_>>()
                        .join(", "),
                }
            },
        )
    }

    /// Apply config change to a mutable config handle.
    ///
    /// This enables unit testing without touching the real global git config.
    fn apply_to_config(&self, config: &mut git2::Config) -> Result<ResourceChange> {
        let values = self.values_from_config(config)?;
        // libgit2's regex mutation can dereference a null implicit value.
        if values.len() > 1 && values.iter().any(Option::is_none) {
            anyhow::bail!(
                "cannot safely replace repeated implicit values for git config {}; give each entry an explicit value first",
                self.key
            );
        }
        match &self.desired {
            Some(desired) => {
                if values.len() <= 1 {
                    config.set_str(&self.key, desired)
                } else {
                    config.set_multivar(&self.key, ".*", desired)
                }
                .with_context(|| format!("setting {} = {desired}", self.key))?;
            }
            None => {
                if values.len() == 1 {
                    config.remove(&self.key)
                } else {
                    config.remove_multivar(&self.key, ".*")
                }
                .with_context(|| format!("removing git config {}", self.key))?;
            }
        }
        Ok(ResourceChange::Applied)
    }

    fn has_includes(&self, config: &git2::Config) -> Result<bool> {
        let mut entries = config.multivar(&self.key, None)?;
        while let Some(entry) = entries.next() {
            if entry?.include_depth() != 0 {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn apply_with_includes(&self, config: &git2::Config) -> Result<ResourceChange> {
        let configured = self
            .config_path
            .clone()
            .map_or_else(git2::Config::find_global, Ok)?;
        let path = crate::infra::fs::canonicalize(&configured)?;
        let before = std::fs::read(&path)?;
        let parent = path.parent().context("global Git config has no parent")?;
        let stage = stage_config(parent, &before)?;
        {
            let mut staged = open_file(stage.path())?;
            let values = self.values_from_config(config)?;
            if values.iter().any(Option::is_none) {
                anyhow::bail!(
                    "cannot safely replace repeated implicit values for git config {}; give each entry an explicit value first",
                    self.key
                );
            }
            if !values.is_empty() {
                staged.remove_multivar(&self.key, ".*")?;
            }
        }
        let mut replacement = std::fs::read(stage.path())?;
        if let Some(desired) = &self.desired {
            // Let libgit2 validate and quote the key/value, then append its stanza
            // after every include so scalar Git precedence selects the desired value.
            std::fs::write(stage.path(), [])?;
            {
                let mut stanza = open_file(stage.path())?;
                stanza.set_str(&self.key, desired)?;
            }
            replacement.push(b'\n');
            replacement.extend(std::fs::read(stage.path())?);
        }
        crate::infra::fs::write_atomic_checked(&path, replacement, || {
            anyhow::ensure!(
                crate::infra::fs::canonicalize(&configured)? == path
                    && std::fs::read(&path)? == before,
                "global Git config changed during update; retry"
            );
            Ok(())
        })?;
        Ok(ResourceChange::Applied)
    }
}

impl Resource for GitConfigResource {
    fn access_claims(&self) -> Vec<AccessClaim> {
        // Global configuration may include multiple files; keep one conservative
        // domain-wide key, including explicit paths that could alias those files.
        vec![AccessClaim::exclusive("git-config", "configuration")]
    }

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
        if self.has_includes(&config)? {
            return Ok(self.apply_with_includes(&config)?);
        }
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

    #[cfg(unix)]
    #[test]
    fn git_config_is_private_before_staging_any_user_content() {
        use std::os::unix::fs::PermissionsExt as _;

        struct PrivateContent<'a>(&'a std::path::Path);
        impl AsRef<[u8]> for PrivateContent<'_> {
            fn as_ref(&self) -> &[u8] {
                let entries = std::fs::read_dir(self.0).unwrap().collect::<Vec<_>>();
                assert_eq!(entries.len(), 1, "exactly one staging file must exist");
                let metadata = entries[0].as_ref().unwrap().metadata().unwrap();
                assert_eq!(metadata.permissions().mode() & 0o077, 0);
                assert_eq!(metadata.len(), 0, "privacy must precede the first write");
                b"[fixture]\n private = synthetic-value\n"
            }
        }

        let directory = tempfile::tempdir().unwrap();
        let staged = stage_config(directory.path(), PrivateContent(directory.path())).unwrap();
        assert_eq!(
            std::fs::read(staged.path()).unwrap(),
            b"[fixture]\n private = synthetic-value\n"
        );
        drop(staged);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn implicit_local_value_with_includes_fails_without_rewriting_either_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        let included = dir.path().join("included");
        let contents = "[core]\n autocrlf\n[include]\n path = included\n";
        std::fs::write(&path, contents).unwrap();
        std::fs::write(&included, "[core]\n autocrlf = true\n").unwrap();
        let resource = GitConfigResource::with_config_path(
            "core.autocrlf".into(),
            "false".into(),
            path.clone(),
        );
        assert!(format!("{:#}", resource.apply().unwrap_err()).contains("implicit values"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
        assert_eq!(
            std::fs::read_to_string(&included).unwrap(),
            "[core]\n autocrlf = true\n"
        );
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            2,
            "failed staging must leave no temporary config"
        );
    }

    #[test]
    fn included_values_are_preserved_and_global_overrides_converge() {
        for local in [
            "",
            "[core]\n autocrlf = true\n",
            "[core]\n autocrlf = false\n",
        ] {
            for include_first in [false, true] {
                let dir = tempfile::Builder::new()
                    .prefix("git settings ")
                    .tempdir()
                    .unwrap();
                let path = dir.path().join("config");
                let included = dir.path().join("included");
                let content = "[core]\n autocrlf = true\n editor = user-editor\n";
                std::fs::write(&included, content).unwrap();
                let include = "[include]\n path = included\n";
                std::fs::write(
                    &path,
                    if include_first {
                        format!("{include}{local}")
                    } else {
                        format!("{local}{include}")
                    },
                )
                .unwrap();
                let resource = GitConfigResource::with_config_path(
                    "core.autocrlf".into(),
                    "false".into(),
                    path.clone(),
                );
                assert_eq!(
                    resource
                        .open_config()
                        .unwrap()
                        .get_string("core.editor")
                        .unwrap(),
                    "user-editor",
                    "relative includes must load from the configuration directory"
                );
                assert_eq!(
                    resource.current_state().unwrap() == ResourceState::Correct,
                    include_first && local.contains("false"),
                    "a later include must not override a managed scalar value"
                );
                resource.apply().unwrap();
                assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
                let after = std::fs::read(&path).unwrap();
                assert_eq!(
                    open_file(&path)
                        .unwrap()
                        .get_string("core.autocrlf")
                        .unwrap(),
                    "false"
                );
                assert_eq!(std::fs::read_to_string(&included).unwrap(), content);
                assert_eq!(std::fs::read(&path).unwrap(), after);
                let absent = GitConfigResource::absent("core.autocrlf".into())
                    .using_config_path(path.clone());
                absent.apply().unwrap();
                assert_eq!(absent.current_state().unwrap(), ResourceState::Correct);
                assert_eq!(
                    open_file(&path)
                        .unwrap()
                        .get_string("core.autocrlf")
                        .unwrap(),
                    "true"
                );
                assert_eq!(std::fs::read_to_string(&included).unwrap(), content);
            }
        }
    }

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
        for (key, section, variable, values, desired) in [
            (
                "core.autocrlf",
                "core",
                "autocrlf",
                ["false", "true"],
                Some("false"),
            ),
            (
                "core.autocrlf",
                "core",
                "autocrlf",
                ["true", "false"],
                Some("false"),
            ),
            (
                "credential.helper",
                "credential",
                "helper",
                ["old-helper", "manager"],
                Some("manager"),
            ),
            (
                "credential.helper",
                "credential",
                "helper",
                ["manager", "old-helper"],
                Some("manager"),
            ),
            ("core.autocrlf", "core", "autocrlf", ["false", "true"], None),
        ] {
            let dir = tempfile::tempdir_in(".").unwrap();
            let path = dir.path().join("config");
            std::fs::write(
                &path,
                format!("[{section}]\n\t{variable} = {}\n\t{variable} = {}\n[core]\n\teditor = fixture-editor\n", values[0], values[1]),
            )
            .unwrap();
            let resource = desired
                .map_or_else(
                    || GitConfigResource::absent(key.into()),
                    |value| GitConfigResource::new(key.into(), value.into()),
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
                let mut entries = observed.multivar(key, None).unwrap();
                let mut actual_values = Vec::new();
                while let Some(entry) = entries.next() {
                    actual_values.push(entry.unwrap().value().unwrap().to_string());
                }
                assert!(!actual_values.is_empty());
                assert!(
                    actual_values.iter().all(|current| current == value),
                    "{actual_values:?}"
                );
            } else {
                assert_eq!(
                    observed.get_string(key).unwrap_err().code(),
                    git2::ErrorCode::NotFound
                );
            }
        }
    }

    #[test]
    fn implicit_and_non_utf8_values_are_not_silently_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        let resource = GitConfigResource::with_config_path(
            "core.autocrlf".into(),
            "false".into(),
            path.clone(),
        );
        std::fs::write(&path, "[core]\n\tautocrlf\n").unwrap();
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Incorrect { .. }
        ));
        resource.apply().unwrap();
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);

        std::fs::write(&path, b"[core]\n\tautocrlf = \xff\n\tautocrlf = false\n").unwrap();
        let error = resource.current_state().unwrap_err();
        assert!(format!("{error:#}").contains("reading git config core.autocrlf"));
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
    fn repeated_implicit_values_fail_safely_without_entering_native_regex_mutation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        let content = "[core]\n\tautocrlf\n\tautocrlf = false\n";
        std::fs::write(&path, content).unwrap();
        for resource in [
            GitConfigResource::new("core.autocrlf".into(), "false".into()),
            GitConfigResource::absent("core.autocrlf".into()),
        ] {
            let resource = resource.using_config_path(path.clone());
            assert!(matches!(
                resource.current_state().unwrap(),
                ResourceState::Incorrect { .. }
            ));
            assert!(
                format!("{:#}", resource.apply().unwrap_err()).contains("repeated implicit values")
            );
            assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
        }
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
