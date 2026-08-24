//! PAM configuration for GNOME Keyring login integration.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, anyhow, bail};

use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::infra::exec::{CommandSpec, Executor};

const PAM_GNOME_KEYRING: &str = "pam_gnome_keyring.so";

#[derive(Debug, Clone, Copy)]
struct PamRule {
    facility: &'static str,
    rendered: &'static str,
}

const LOGIN_RULES: [PamRule; 2] = [
    PamRule {
        facility: "auth",
        rendered: "auth       optional     pam_gnome_keyring.so",
    },
    PamRule {
        facility: "session",
        rendered: "session    optional     pam_gnome_keyring.so auto_start",
    },
];

const PASSWD_RULES: [PamRule; 1] = [PamRule {
    facility: "password",
    rendered: "password   optional     pam_gnome_keyring.so",
}];

/// PAM service file receiving GNOME Keyring integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PamKeyringService {
    /// Console login authentication and session setup.
    Login,
    /// Password changes, used to keep the login keyring password synchronized.
    Passwd,
}

impl PamKeyringService {
    const fn name(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Passwd => "passwd",
        }
    }

    const fn rules(self) -> &'static [PamRule] {
        match self {
            Self::Login => &LOGIN_RULES,
            Self::Passwd => &PASSWD_RULES,
        }
    }
}

/// Converges the GNOME Keyring directives in one existing PAM service file.
pub struct PamKeyringResource {
    service: PamKeyringService,
    target: PathBuf,
    temp_dir: PathBuf,
    executor: Arc<dyn Executor>,
}

impl std::fmt::Debug for PamKeyringResource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PamKeyringResource")
            .field("service", &self.service)
            .field("target", &self.target)
            .field("temp_dir", &self.temp_dir)
            .finish_non_exhaustive()
    }
}

impl PamKeyringResource {
    /// Create a resource targeting explicit paths.
    #[must_use]
    pub fn new(
        service: PamKeyringService,
        target: impl Into<PathBuf>,
        executor: Arc<dyn Executor>,
        temp_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            service,
            target: target.into(),
            temp_dir: temp_dir.into(),
            executor,
        }
    }

    fn current_content(&self) -> ResourceResult<String> {
        let metadata = fs::symlink_metadata(&self.target)
            .with_context(|| format!("reading metadata for {}", self.target.display()))?;
        if !metadata.is_file() {
            return Err(anyhow!("{} is not a regular file", self.target.display()).into());
        }
        fs::read_to_string(&self.target)
            .with_context(|| format!("reading {}", self.target.display()))
            .map_err(Into::into)
    }

    fn desired_content(&self, current: &str) -> ResourceResult<String> {
        merge_rules(current, self.service.rules()).map_err(Into::into)
    }
}

impl Resource for PamKeyringResource {
    fn description(&self) -> String {
        format!("GNOME Keyring PAM integration ({})", self.service.name())
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        let current = self.current_content()?;
        let desired = self.desired_content(&current)?;
        if desired == current {
            return Ok(ResourceChange::AlreadyCorrect);
        }

        let temporary = TemporaryPamFile::create(&self.temp_dir, self.service, &desired)?;
        self.executor
            .execute(
                CommandSpec::new("sudo")
                    .arg("install")
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

impl IntrinsicState for PamKeyringResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        let metadata = match fs::symlink_metadata(&self.target) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(ResourceState::Invalid {
                    reason: format!(
                        "required PAM service file {} is missing",
                        self.target.display()
                    ),
                });
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

        let current = fs::read_to_string(&self.target)
            .with_context(|| format!("reading {}", self.target.display()))?;
        let desired = match self.desired_content(&current) {
            Ok(desired) => desired,
            Err(error) => {
                return Ok(ResourceState::Invalid {
                    reason: error.to_string(),
                });
            }
        };

        Ok(if desired == current {
            ResourceState::Correct
        } else {
            ResourceState::Incorrect {
                current: "GNOME Keyring directives are missing, duplicated, or misplaced"
                    .to_string(),
            }
        })
    }
}

fn merge_rules(current: &str, rules: &[PamRule]) -> anyhow::Result<String> {
    let mut lines: Vec<String> = current.lines().map(str::to_string).collect();

    for rule in rules {
        lines.retain(|line| !is_keyring_rule(line, rule.facility));
        let Some(last_facility_line) = lines
            .iter()
            .rposition(|line| pam_facility(line) == Some(rule.facility))
        else {
            bail!(
                "PAM service has no {} stack; refusing to synthesize one",
                rule.facility
            );
        };
        lines.insert(
            last_facility_line.saturating_add(1),
            rule.rendered.to_string(),
        );
    }

    let mut merged = lines.join("\n");
    merged.push('\n');
    Ok(merged)
}

fn pam_tokens(line: &str) -> Option<Vec<&str>> {
    let code = line.split_once('#').map_or(line, |(code, _)| code).trim();
    if code.is_empty() {
        return None;
    }
    Some(code.split_whitespace().collect())
}

fn pam_facility(line: &str) -> Option<&str> {
    pam_tokens(line)?.first().copied()
}

fn is_keyring_rule(line: &str, facility: &str) -> bool {
    let Some(tokens) = pam_tokens(line) else {
        return false;
    };
    tokens.first().copied() == Some(facility) && tokens.contains(&PAM_GNOME_KEYRING)
}

struct TemporaryPamFile {
    path: PathBuf,
}

impl TemporaryPamFile {
    fn create(directory: &Path, service: PamKeyringService, content: &str) -> anyhow::Result<Self> {
        for attempt in 0..32 {
            let path = directory.join(format!(
                ".dotfiles-pam-{}-{}-{attempt}",
                service.name(),
                std::process::id()
            ));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    let temporary = Self { path };
                    file.write_all(content.as_bytes())
                        .with_context(|| format!("writing {}", temporary.path.display()))?;
                    file.sync_all()
                        .with_context(|| format!("syncing {}", temporary.path.display()))?;
                    return Ok(temporary);
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(anyhow!(error).context(format!(
                        "creating temporary PAM file in {}",
                        directory.display()
                    )));
                }
            }
        }
        bail!(
            "could not allocate a temporary PAM file in {}",
            directory.display()
        )
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryPamFile {
    fn drop(&mut self) {
        drop(fs::remove_file(&self.path));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::exec::{ExecError, ExecResult, MockExecutor};
    use std::ffi::{OsStr, OsString};
    use tempfile::TempDir;

    const LOGIN_BASE: &str = "\
#%PAM-1.0

auth       requisite    pam_nologin.so
auth       include      system-local-login
account    include      system-local-login
session    include      system-local-login
password   include      system-local-login
";

    const PASSWD_BASE: &str = "\
#%PAM-1.0
auth        include     system-auth
account     include     system-auth
password    include     system-auth
";

    fn resource(
        service: PamKeyringService,
        target: &Path,
        temp_dir: &Path,
        executor: MockExecutor,
    ) -> PamKeyringResource {
        PamKeyringResource::new(service, target, Arc::new(executor), temp_dir)
    }

    #[test]
    fn login_rules_are_placed_at_the_end_of_their_stacks() {
        let current = format!(
            "{LOGIN_BASE}auth optional pam_gnome_keyring.so old_option\n\
             session optional pam_gnome_keyring.so\n"
        );

        let merged = merge_rules(&current, &LOGIN_RULES).unwrap();

        assert_eq!(merged.matches(PAM_GNOME_KEYRING).count(), 2);
        assert!(merged.contains(
            "auth       include      system-local-login\n\
             auth       optional     pam_gnome_keyring.so\n\
             account"
        ));
        assert!(merged.contains(
            "session    include      system-local-login\n\
             session    optional     pam_gnome_keyring.so auto_start\n\
             password"
        ));
    }

    #[test]
    fn passwd_rule_preserves_unrelated_configuration() {
        let merged = merge_rules(PASSWD_BASE, &PASSWD_RULES).unwrap();

        assert!(merged.contains("account     include     system-auth"));
        assert!(merged.ends_with(
            "password    include     system-auth\n\
             password   optional     pam_gnome_keyring.so\n"
        ));
    }

    #[test]
    fn missing_service_file_is_invalid() {
        let temp = TempDir::new().unwrap();
        let target = temp.path().join("login");

        assert!(matches!(
            resource(
                PamKeyringService::Login,
                &target,
                temp.path(),
                MockExecutor::new()
            )
            .current_state()
            .unwrap(),
            ResourceState::Invalid { ref reason } if reason.contains("is missing")
        ));
    }

    #[test]
    fn service_without_required_stack_is_invalid() {
        let temp = TempDir::new().unwrap();
        let target = temp.path().join("login");
        fs::write(&target, "#%PAM-1.0\naccount include system-local-login\n").unwrap();

        assert!(matches!(
            resource(
                PamKeyringService::Login,
                &target,
                temp.path(),
                MockExecutor::new()
            )
            .current_state()
            .unwrap(),
            ResourceState::Invalid { ref reason } if reason.contains("no auth stack")
        ));
    }

    #[test]
    fn apply_installs_merged_content_and_converges() {
        let temp = TempDir::new().unwrap();
        let target = temp.path().join("login");
        fs::write(&target, LOGIN_BASE).unwrap();

        let expected_target = target.clone();
        let mut executor = MockExecutor::new();
        executor.expect_execute().once().returning(move |spec| {
            assert_eq!(spec.program(), OsStr::new("sudo"));
            let args = spec.arguments();
            assert_eq!(
                &args[..5],
                [
                    "install",
                    "--owner=root",
                    "--group=root",
                    "--mode=0644",
                    "--"
                ]
            );
            assert_eq!(
                args.get(6).map(OsString::as_os_str),
                Some(expected_target.as_os_str())
            );
            let source = args.get(5).expect("install source argument");
            fs::copy(source, &expected_target).unwrap();
            Ok(ExecResult::success(""))
        });
        let resource = resource(PamKeyringService::Login, &target, temp.path(), executor);

        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
        assert_eq!(resource.apply().unwrap(), ResourceChange::AlreadyCorrect);
    }

    #[test]
    fn failed_install_preserves_original_and_cleans_up_temp_file() {
        let temp = TempDir::new().unwrap();
        let target = temp.path().join("passwd");
        fs::write(&target, PASSWD_BASE).unwrap();
        let mut executor = MockExecutor::new();
        executor.expect_execute().once().returning(|_| {
            Err(ExecError::spawn(
                "sudo",
                std::io::Error::other("permission denied"),
            ))
        });
        let resource = resource(PamKeyringService::Passwd, &target, temp.path(), executor);

        assert!(resource.apply().is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), PASSWD_BASE);
        assert!(fs::read_dir(temp.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".dotfiles-pam-")
        }));
    }
}
