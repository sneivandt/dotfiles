//! Privileged system-file convergence driven by `conf/system-files.toml`.

mod install;
mod merge_ini;
mod merge_pam;
mod merge_toml;

use install::ATOMIC_INSTALL;
#[cfg(all(unix, test))]
use install::prepare_in_private_stage;
pub(crate) use install::privileged_prepare;
use install::read_fragment;
#[cfg(all(unix, test))]
use install::verified_fragment;
use merge_ini::contains_ini;
#[cfg(test)]
use merge_ini::ini_line_key;
use merge_ini::ini_settings;
#[cfg(any(unix, test))]
use merge_ini::merge_ini;
use merge_pam::merge_pam;
#[cfg(test)]
use merge_pam::pam_rule_identity;
#[cfg(all(unix, test))]
use merge_pam::same_pam_rule;
use merge_pam::validate_pam_rules;
use merge_toml::contains_toml;
#[cfg(any(unix, test))]
use merge_toml::merge_toml;
use merge_toml::parse_toml;

use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context as _, anyhow};

use crate::domains::system::config::system_files::{MergeStrategy, SystemFile};
use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::infra::exec::{CommandSpec, Executor, OutputLog};

/// Converges one system file from a tracked source fragment.
pub(crate) struct SystemFileResource {
    entry: SystemFile,
    target_root: PathBuf,
    executor: Arc<dyn Executor>,
}

enum CurrentFile {
    Missing,
    NonRegular,
    Present(String),
}

impl std::fmt::Debug for SystemFileResource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemFileResource")
            .field("target", &self.entry.target)
            .field("source", &self.entry.source_path())
            .field("merge", &self.entry.merge)
            .finish_non_exhaustive()
    }
}

impl SystemFileResource {
    /// Create a resource from one configured system file.
    #[must_use]
    pub(crate) fn new(entry: SystemFile, executor: Arc<dyn Executor>) -> Self {
        Self {
            entry,
            target_root: PathBuf::from("/etc"),
            executor,
        }
    }

    fn fragment(&self) -> ResourceResult<String> {
        let source = self.entry.source_path();
        read_fragment(&source)
            .and_then(|bytes| String::from_utf8(bytes).map_err(Into::into))
            .with_context(|| format!("reading {}", source.display()))
            .map_err(Into::into)
    }

    pub(in crate::domains::system) fn installation_state(&self) -> ResourceResult<ResourceState> {
        match self.current_state()? {
            // A later declaration may reference this dedicated file. Never publish
            // such a reference when its prerequisite belongs to the administrator.
            ResourceState::Invalid { reason } if self.entry.merge == MergeStrategy::CreateOnly => {
                Err(crate::engine::resource::ResourceError::conflicting_state(
                    self.description(),
                    "a missing or identical dedicated file",
                    reason,
                ))
            }
            // Read protected destinations only inside the scoped installer.
            ResourceState::Unknown { reason } => Ok(ResourceState::Incorrect { current: reason }),
            state @ (ResourceState::Correct
            | ResourceState::Missing
            | ResourceState::Incorrect { .. }
            | ResourceState::Invalid { .. }) => Ok(state),
        }
    }

    fn checked_target(&self) -> anyhow::Result<PathBuf> {
        let root = dunce::canonicalize(&self.target_root)?;
        let target = crate::infra::fs::resolve_target_entry(&self.entry.target)?;
        anyhow::ensure!(
            target != root && target.starts_with(&root),
            "system file target {} resolves outside {} to {}",
            self.entry.target.display(),
            self.target_root.display(),
            target.display()
        );
        Ok(target)
    }

    fn validate_installation_target(&self) -> anyhow::Result<()> {
        let target = &self.entry.target;
        // The private boundary is absolute, so this also excludes relative targets.
        anyhow::ensure!(
            target.starts_with(&self.target_root),
            "unsafe system file target"
        );
        anyhow::ensure!(target != &self.target_root, "unsafe system file target");
        anyhow::ensure!(
            !target.components().any(|component| matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )),
            "unsafe system file target"
        );
        Ok(())
    }

    fn assessment_with_target(
        &self,
        fragment: &str,
        resolved: anyhow::Result<PathBuf>,
    ) -> ResourceResult<(ResourceState, PathBuf)> {
        self.validate_installation_target()?;
        let target = match resolved {
            Ok(target) => target,
            Err(error) if permission_denied(&error) => {
                return Ok((
                    ResourceState::Unknown {
                        reason: "requires privileged assessment; destination was not read"
                            .to_string(),
                    },
                    self.entry.target.clone(),
                ));
            }
            Err(error) => return Err(error.into()),
        };
        let state = match Self::current_content(&target).map_err(anyhow::Error::from) {
            Ok(current) => self.state_for(&current, fragment),
            Err(error) if permission_denied(&error) => ResourceState::Unknown {
                reason: "requires privileged assessment; destination was not read".to_string(),
            },
            Err(error) => return Err(error.into()),
        };
        Ok((state, target))
    }

    fn current_content(target: &std::path::Path) -> ResourceResult<CurrentFile> {
        let metadata = match fs::symlink_metadata(target) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(CurrentFile::Missing),
            Err(error) => {
                return Err(anyhow!(error)
                    .context(format!("reading metadata for {}", target.display()))
                    .into());
            }
        };
        if !metadata.is_file() {
            return Ok(CurrentFile::NonRegular);
        }
        let content =
            fs::read_to_string(target).with_context(|| format!("reading {}", target.display()))?;
        Ok(CurrentFile::Present(content))
    }

    #[cfg(unix)]
    fn desired_content(&self, current: &str, fragment: &str) -> anyhow::Result<String> {
        match self.entry.merge {
            MergeStrategy::Toml => merge_toml(current, fragment),
            MergeStrategy::Ini => merge_ini(current, fragment),
            MergeStrategy::Pam => merge_pam(current, fragment),
            MergeStrategy::CreateOnly => Ok(fragment.to_owned()),
        }
    }

    fn content_is_correct(&self, current: &str, fragment: &str) -> anyhow::Result<bool> {
        match self.entry.merge {
            MergeStrategy::Toml => contains_toml(current, fragment),
            MergeStrategy::Ini => contains_ini(current, fragment),
            MergeStrategy::Pam => Ok(merge_pam(current, fragment)? == current),
            MergeStrategy::CreateOnly => Ok(current == fragment),
        }
    }

    fn state_for(&self, current: &CurrentFile, fragment: &str) -> ResourceState {
        let content = match current {
            CurrentFile::Missing if self.entry.merge == MergeStrategy::Pam => {
                return ResourceState::Invalid {
                    reason: format!(
                        "required PAM service file {} is missing",
                        self.entry.target.display()
                    ),
                };
            }
            CurrentFile::Missing => "",
            CurrentFile::NonRegular => {
                return ResourceState::Invalid {
                    reason: format!("{} is not a regular file", self.entry.target.display()),
                };
            }
            CurrentFile::Present(content) => content,
        };
        match self.content_is_correct(content, fragment) {
            Err(error) => ResourceState::Invalid {
                reason: error.to_string(),
            },
            Ok(_) if matches!(current, CurrentFile::Missing) => ResourceState::Missing,
            Ok(true) => ResourceState::Correct,
            Ok(false) if self.entry.merge == MergeStrategy::CreateOnly => ResourceState::Invalid {
                reason: format!(
                    "{} already exists with different content; review the existing file before retrying",
                    self.entry.target.display()
                ),
            },
            Ok(false) => ResourceState::Incorrect {
                current: "managed content differs".to_string(),
            },
        }
    }
}

impl Resource for SystemFileResource {
    fn description(&self) -> String {
        self.entry.target.display().to_string()
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        let fragment = self.fragment()?;
        if let Err(error) = validate_fragment(self.entry.merge, &fragment) {
            return Err(crate::engine::resource::ResourceError::conflicting_state(
                self.entry.target.display().to_string(),
                "a valid system file and fragment",
                error.to_string(),
            ));
        }
        let (state, target) = self.assessment_with_target(&fragment, self.checked_target())?;
        match state {
            ResourceState::Correct => return Ok(ResourceChange::AlreadyCorrect),
            ResourceState::Invalid { reason } => {
                return Err(crate::engine::resource::ResourceError::conflicting_state(
                    self.entry.target.display().to_string(),
                    "a valid system file and fragment",
                    reason,
                ));
            }
            ResourceState::Missing
            | ResourceState::Incorrect { .. }
            | ResourceState::Unknown { .. } => {}
        }
        let result = self
            .executor
            .execute(
                CommandSpec::new("sudo")
                    .args(&[
                        "-n",
                        "--",
                        "sh",
                        "-c",
                        ATOMIC_INSTALL,
                        "dotfiles-system-file",
                    ])
                    .arg(std::env::current_exe()?)
                    .arg(&target)
                    .arg(dunce::canonicalize(self.entry.source_path())?)
                    .arg(&self.target_root)
                    .arg(match self.entry.merge {
                        MergeStrategy::Toml => "toml",
                        MergeStrategy::Ini => "ini",
                        MergeStrategy::Pam => "pam",
                        MergeStrategy::CreateOnly => "create-only",
                    })
                    .arg(crate::infra::fs::content_hash(fragment.as_bytes()))
                    .redact_arguments()
                    .output_log(OutputLog::Omit),
            )
            .with_context(|| format!("installing {}", self.entry.target.display()))?;
        match result.stdout.trim() {
            "applied" => Ok(ResourceChange::Applied),
            "already-correct" => Ok(ResourceChange::AlreadyCorrect),
            _ => Err(anyhow!("invalid system file installation response").into()),
        }
    }
}

impl IntrinsicState for SystemFileResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        let fragment = self.fragment()?;
        if let Err(error) = validate_fragment(self.entry.merge, &fragment) {
            return Ok(ResourceState::Invalid {
                reason: error.to_string(),
            });
        }
        self.assessment_with_target(&fragment, self.checked_target())
            .map(|(state, _)| state)
    }
}

fn permission_denied(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io_error| io_error.kind() == ErrorKind::PermissionDenied)
    })
}

pub(crate) fn validate_fragment(merge: MergeStrategy, fragment: &str) -> anyhow::Result<()> {
    match merge {
        MergeStrategy::Toml => parse_toml(fragment).map(drop),
        MergeStrategy::Ini => ini_settings(fragment, true).map(drop),
        MergeStrategy::Pam => validate_pam_rules(fragment),
        MergeStrategy::CreateOnly => Ok(()),
    }
}

#[cfg(test)]
#[path = "tests/system_file.rs"]
mod tests;
