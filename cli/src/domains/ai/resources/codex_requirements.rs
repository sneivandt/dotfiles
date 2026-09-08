//! System Codex requirements resource.

use std::fs;
use std::io::{ErrorKind, Write as _};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context as _, Result, anyhow};

use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::infra::exec::{CommandSpec, Executor, OutputLog};

const SYSTEM_TARGET: &str = "/etc/codex/requirements.toml";

/// Merges dotfiles-managed values into the administrator-enforced Codex policy.
pub struct CodexRequirementsResource {
    desired: toml::Table,
    target: PathBuf,
    temp_dir: PathBuf,
    executor: Arc<dyn Executor>,
    #[cfg(unix)]
    expected_uid: u32,
    #[cfg(unix)]
    expected_gid: u32,
}

impl std::fmt::Debug for CodexRequirementsResource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodexRequirementsResource")
            .field("target", &self.target)
            .field("temp_dir", &self.temp_dir)
            .finish_non_exhaustive()
    }
}

impl CodexRequirementsResource {
    /// Create the system requirements resource.
    #[must_use]
    pub fn system(executor: Arc<dyn Executor>) -> Self {
        Self::new(
            desired_requirements(),
            SYSTEM_TARGET,
            std::env::temp_dir(),
            executor,
        )
    }

    /// Create a resource targeting explicit paths.
    #[must_use]
    pub fn new(
        desired: toml::Table,
        target: impl Into<PathBuf>,
        temp_dir: impl Into<PathBuf>,
        executor: Arc<dyn Executor>,
    ) -> Self {
        Self {
            desired,
            target: target.into(),
            temp_dir: temp_dir.into(),
            executor,
            #[cfg(unix)]
            expected_uid: 0,
            #[cfg(unix)]
            expected_gid: 0,
        }
    }

    #[cfg(all(test, unix))]
    const fn with_expected_identity(mut self, uid: u32, gid: u32) -> Self {
        self.expected_uid = uid;
        self.expected_gid = gid;
        self
    }

    fn read_document(&self) -> Result<Option<(toml::Table, fs::Metadata)>> {
        let metadata = match fs::symlink_metadata(&self.target) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(anyhow!(error)
                    .context(format!("reading metadata for {}", self.target.display())));
            }
        };
        if !metadata.is_file() {
            return Err(anyhow!("{} is not a regular file", self.target.display()));
        }

        let contents = match fs::read_to_string(&self.target) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::PermissionDenied => {
                self.read_document_elevated()?
            }
            Err(error) => {
                return Err(anyhow!(error).context(format!("reading {}", self.target.display())));
            }
        };
        let document = if contents.trim().is_empty() {
            toml::Table::new()
        } else {
            toml::from_str(&contents)
                .with_context(|| format!("parsing {}", self.target.display()))?
        };
        Ok(Some((document, metadata)))
    }

    fn read_document_elevated(&self) -> Result<String> {
        self.executor
            .execute(
                CommandSpec::new("sudo")
                    .arg("cat")
                    .arg("--")
                    .arg(&self.target)
                    .output_log(OutputLog::Omit),
            )
            .with_context(|| format!("reading {}", self.target.display()))
            .map(|result| result.stdout)
    }

    #[cfg(unix)]
    fn metadata_is_correct(&self, metadata: &fs::Metadata) -> bool {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        metadata.uid() == self.expected_uid
            && metadata.gid() == self.expected_gid
            && metadata.permissions().mode() & 0o7777 == 0o644
    }

    #[cfg(not(unix))]
    #[allow(
        clippy::unused_self,
        reason = "the Unix implementation validates owner and mode from resource state"
    )]
    const fn metadata_is_correct(&self, _metadata: &fs::Metadata) -> bool {
        true
    }

    fn merged_document(&self) -> Result<toml::Table> {
        let mut document = self
            .read_document()?
            .map_or_else(toml::Table::new, |(document, _)| document);
        merge_table(&mut document, &self.desired);
        Ok(document)
    }
}

impl Resource for CodexRequirementsResource {
    fn description(&self) -> String {
        format!("Codex requirements {}", self.target.display())
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        if matches!(self.current_state()?, ResourceState::Correct) {
            return Ok(ResourceChange::AlreadyCorrect);
        }

        let merged = self.merged_document()?;
        let mut serialized = toml::to_string_pretty(&merged)
            .with_context(|| format!("serializing {}", self.target.display()))?;
        if !serialized.ends_with('\n') {
            serialized.push('\n');
        }

        let (temporary, mut file) = crate::infra::fs::TempGuard::create_unique_file_with_mode(
            &self.temp_dir,
            ".dotfiles-codex-requirements",
            "tmp",
            0o600,
        )
        .context("creating temporary Codex requirements file")?;
        file.write_all(serialized.as_bytes())
            .context("writing temporary Codex requirements file")?;
        file.sync_all()
            .context("syncing temporary Codex requirements file")?;
        drop(file);

        self.executor
            .execute(
                CommandSpec::new("sudo")
                    .arg("install")
                    .arg("-D")
                    .arg("--owner=root")
                    .arg("--group=root")
                    .arg("--mode=0644")
                    .arg("--")
                    .arg(temporary.path())
                    .arg(&self.target),
            )
            .with_context(|| format!("installing {}", self.target.display()))?;

        Ok(ResourceChange::Applied)
    }
}

impl IntrinsicState for CodexRequirementsResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        let metadata = match fs::symlink_metadata(&self.target) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(ResourceState::Missing);
            }
            Err(error) => {
                return Err(anyhow!(error)
                    .context(format!("reading metadata for {}", self.target.display()))
                    .into());
            }
        };
        if !metadata.is_file() {
            return Ok(ResourceState::Invalid {
                reason: format!("{} is not a regular file", self.target.display()),
            });
        }
        let metadata_matches = self.metadata_is_correct(&metadata);
        let contents = match fs::read_to_string(&self.target) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::PermissionDenied && !metadata_matches => {
                return Ok(ResourceState::Incorrect {
                    current: "ownership or mode differs".to_string(),
                });
            }
            Err(error) => {
                return Err(anyhow!(error)
                    .context(format!("reading {}", self.target.display()))
                    .into());
            }
        };
        let document = if contents.trim().is_empty() {
            toml::Table::new()
        } else {
            match toml::from_str(&contents) {
                Ok(document) => document,
                Err(error) => {
                    return Ok(ResourceState::Invalid {
                        reason: format!("invalid TOML in {}: {error}", self.target.display()),
                    });
                }
            }
        };

        let content_matches = contains_table(&document, &self.desired);
        Ok(if content_matches && metadata_matches {
            ResourceState::Correct
        } else {
            let mut differences = Vec::new();
            if !content_matches {
                differences.push("managed values differ");
            }
            if !metadata_matches {
                differences.push("ownership or mode differs");
            }
            ResourceState::Incorrect {
                current: differences.join("; "),
            }
        })
    }
}

fn merge_table(current: &mut toml::Table, desired: &toml::Table) {
    for (key, desired_value) in desired {
        match (current.get_mut(key), desired_value) {
            (Some(toml::Value::Table(current_table)), toml::Value::Table(desired_table)) => {
                merge_table(current_table, desired_table);
            }
            _ => {
                current.insert(key.clone(), desired_value.clone());
            }
        }
    }
}

fn contains_table(current: &toml::Table, desired: &toml::Table) -> bool {
    desired.iter().all(
        |(key, desired_value)| match (current.get(key), desired_value) {
            (Some(toml::Value::Table(current_table)), toml::Value::Table(desired_table)) => {
                contains_table(current_table, desired_table)
            }
            (Some(current_value), desired_value) => current_value == desired_value,
            (None, _) => false,
        },
    )
}

fn desired_requirements() -> toml::Table {
    let origin_policy = toml::Table::from_iter([
        (
            "access".to_string(),
            toml::Value::String("allow".to_string()),
        ),
        (
            "auto_review".to_string(),
            toml::Value::String("deny".to_string()),
        ),
        (
            "persistent_approval".to_string(),
            toml::Value::Boolean(false),
        ),
        (
            "access_approval_lifetime".to_string(),
            toml::Value::String("turn".to_string()),
        ),
    ]);
    let browser_use = toml::Table::from_iter([
        (
            "disable_auto_review".to_string(),
            toml::Value::Boolean(true),
        ),
        (
            "allow_global_persistent_approval".to_string(),
            toml::Value::Boolean(false),
        ),
        (
            "default_origin_policy".to_string(),
            toml::Value::Table(origin_policy),
        ),
    ]);
    toml::Table::from_iter([("browser_use".to_string(), toml::Value::Table(browser_use))])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::exec::{ExecResult, MockExecutor};
    use std::path::Path;

    fn requirements() -> toml::Table {
        desired_requirements()
    }

    fn resource(
        target: &Path,
        temp_dir: &Path,
        executor: MockExecutor,
    ) -> CodexRequirementsResource {
        CodexRequirementsResource::new(requirements(), target, temp_dir, Arc::new(executor))
    }

    #[test]
    fn recursive_merge_preserves_unmanaged_requirements() {
        let mut current: toml::Table = toml::from_str(
            "allowed_web_search_modes = [\"cached\"]\n\
             [browser_use]\nunmanaged = \"keep\"\n\
             [browser_use.default_origin_policy]\nauto_review = \"deny\"\n",
        )
        .unwrap();

        merge_table(&mut current, &requirements());

        assert_eq!(
            current["allowed_web_search_modes"][0],
            toml::Value::String("cached".to_string())
        );
        assert_eq!(
            current["browser_use"]["unmanaged"],
            toml::Value::String("keep".to_string())
        );
        assert_eq!(
            current["browser_use"]["disable_auto_review"],
            toml::Value::Boolean(true)
        );
        assert_eq!(
            current["browser_use"]["default_origin_policy"]["auto_review"],
            toml::Value::String("deny".to_string())
        );
        assert!(contains_table(&current, &requirements()));
    }

    #[test]
    fn managed_policy_matches_codex_browser_requirements() {
        let desired = requirements();
        let browser = desired["browser_use"].as_table().unwrap();
        let origin = browser["default_origin_policy"].as_table().unwrap();

        assert_eq!(browser["disable_auto_review"].as_bool(), Some(true));
        assert_eq!(
            browser["allow_global_persistent_approval"].as_bool(),
            Some(false)
        );
        assert_eq!(origin["access"].as_str(), Some("allow"));
        assert_eq!(origin["auto_review"].as_str(), Some("deny"));
        assert_eq!(origin["persistent_approval"].as_bool(), Some(false));
        assert_eq!(origin["access_approval_lifetime"].as_str(), Some("turn"));
    }

    #[test]
    fn missing_file_has_missing_state() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("requirements.toml");

        assert_eq!(
            resource(&target, dir.path(), MockExecutor::new())
                .current_state()
                .unwrap(),
            ResourceState::Missing
        );
    }

    #[test]
    fn non_regular_target_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let state = resource(dir.path(), dir.path(), MockExecutor::new())
            .current_state()
            .unwrap();

        assert!(matches!(
            state,
            ResourceState::Invalid { reason } if reason.contains("is not a regular file")
        ));
    }

    #[cfg(unix)]
    #[test]
    fn correct_content_mode_and_identity_are_current() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("requirements.toml");
        fs::write(&target, toml::to_string(&requirements()).unwrap()).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
        let metadata = fs::metadata(&target).unwrap();
        let resource = resource(&target, dir.path(), MockExecutor::new())
            .with_expected_identity(metadata.uid(), metadata.gid());

        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[cfg(unix)]
    #[test]
    fn metadata_drift_is_incorrect() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("requirements.toml");
        fs::write(&target, toml::to_string(&requirements()).unwrap()).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        let metadata = fs::metadata(&target).unwrap();
        let resource = resource(&target, dir.path(), MockExecutor::new())
            .with_expected_identity(metadata.uid(), metadata.gid());

        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Incorrect { current }
                if current == "ownership or mode differs"
        ));
    }

    #[test]
    fn elevated_read_uses_sudo_cat_without_logging_content() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("requirements.toml");
        let expected_target = target.clone();
        let mut executor = MockExecutor::new();
        executor
            .expect_execute()
            .once()
            .withf(move |spec| {
                spec.program() == "sudo"
                    && spec.arguments().len() == 3
                    && spec.arguments().starts_with(&["cat".into(), "--".into()])
                    && spec
                        .arguments()
                        .last()
                        .is_some_and(|argument| argument == expected_target.as_os_str())
            })
            .returning(|_| Ok(ExecResult::success("unmanaged = true\n")));
        let resource = resource(&target, dir.path(), executor);

        assert_eq!(
            resource.read_document_elevated().unwrap(),
            "unmanaged = true\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn apply_stages_a_recursive_merge_uses_root_owned_mode_and_converges() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("etc/codex/requirements.toml");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(
            &target,
            "allowed_web_search_modes = [\"cached\"]\n\
             [browser_use]\ndisable_auto_review = false\n",
        )
        .unwrap();
        let staged = Arc::new(std::sync::Mutex::new(String::new()));
        let staged_for_command = Arc::clone(&staged);
        let expected_target = target.clone();
        let expected_target_for_match = expected_target.clone();
        let mut executor = MockExecutor::new();
        executor
            .expect_execute()
            .once()
            .withf(move |spec| {
                spec.program() == "sudo"
                    && spec.arguments().starts_with(&[
                        "install".into(),
                        "-D".into(),
                        "--owner=root".into(),
                        "--group=root".into(),
                        "--mode=0644".into(),
                        "--".into(),
                    ])
                    && spec
                        .arguments()
                        .last()
                        .is_some_and(|argument| argument == expected_target_for_match.as_os_str())
            })
            .returning(move |spec| {
                let source = spec.arguments().get(6).unwrap();
                *staged_for_command.lock().unwrap() = fs::read_to_string(source).unwrap();
                assert_eq!(
                    fs::metadata(source).unwrap().permissions().mode() & 0o777,
                    0o600
                );
                fs::copy(source, &expected_target).unwrap();
                fs::set_permissions(&expected_target, fs::Permissions::from_mode(0o644)).unwrap();
                Ok(ExecResult::success(""))
            });

        let metadata = fs::metadata(&target).unwrap();
        let resource = resource(&target, dir.path(), executor)
            .with_expected_identity(metadata.uid(), metadata.gid());

        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        let merged: toml::Table = toml::from_str(&staged.lock().unwrap()).unwrap();
        assert_eq!(
            merged["allowed_web_search_modes"][0],
            toml::Value::String("cached".to_string())
        );
        assert!(contains_table(&merged, &requirements()));
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
        assert_eq!(resource.apply().unwrap(), ResourceChange::AlreadyCorrect);
    }

    #[test]
    fn malformed_existing_toml_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("requirements.toml");
        fs::write(&target, "[broken").unwrap();

        let resource = resource(&target, dir.path(), MockExecutor::new());
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Invalid { reason } if reason.contains("invalid TOML")
        ));
        let error = resource.apply().unwrap_err();

        assert!(error.to_string().contains("parsing"));
        assert_eq!(fs::read_to_string(target).unwrap(), "[broken");
    }

    #[test]
    fn failed_install_preserves_original_and_cleans_up_temp_file() {
        use crate::infra::exec::ExecError;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("requirements.toml");
        let original = "[browser_use]\ndisable_auto_review = false\n";
        fs::write(&target, original).unwrap();
        let mut executor = MockExecutor::new();
        executor.expect_execute().once().returning(|_| {
            Err(ExecError::spawn(
                "sudo",
                std::io::Error::other("permission denied"),
            ))
        });
        let resource = resource(&target, dir.path(), executor);

        assert!(resource.apply().is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), original);
        assert!(fs::read_dir(dir.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".dotfiles-codex-requirements")
        }));
    }
}
