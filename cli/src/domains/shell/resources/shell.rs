//! Login shell configuration resource.
use std::sync::Arc;

use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::infra::env::Env;
use crate::infra::exec::{CommandSpec, ExecResult, Executor};

/// A resource for configuring the current account's default login shell.
#[derive(Debug)]
pub struct DefaultShellResource {
    target_shell: String,
    executor: Arc<dyn Executor>,
    env: Arc<dyn Env>,
    running_as_root: bool,
}

impl DefaultShellResource {
    /// Create a new default shell resource.
    #[must_use]
    pub fn new(target_shell: String, executor: Arc<dyn Executor>, env: Arc<dyn Env>) -> Self {
        Self {
            target_shell,
            executor,
            env,
            running_as_root: process_is_root(),
        }
    }

    #[cfg(test)]
    #[must_use]
    const fn with_root(mut self, running_as_root: bool) -> Self {
        self.running_as_root = running_as_root;
        self
    }

    fn target_user(&self) -> ResourceResult<String> {
        let sudo_user = self
            .running_as_root
            .then(|| self.env.var("SUDO_USER"))
            .flatten()
            .filter(|user| !user.is_empty() && user != "root");
        sudo_user
            .or_else(|| self.env.var("USER").filter(|user| !user.is_empty()))
            .or_else(|| self.env.var("LOGNAME").filter(|user| !user.is_empty()))
            .ok_or_else(|| anyhow::anyhow!("USER and LOGNAME are not set").into())
    }

    fn account_shell(&self, user: &str) -> ResourceResult<ResourceState> {
        let result = self.executor.execute(
            CommandSpec::new("getent")
                .args(&["passwd", user])
                .unchecked(),
        )?;
        if !result.success {
            return Ok(ResourceState::Unknown {
                reason: format!(
                    "could not read the {user} account: {}",
                    failure_details(&result)
                ),
            });
        }

        let Some(shell) = result
            .stdout
            .lines()
            .find_map(|line| passwd_shell(line, user))
        else {
            return Ok(ResourceState::Unknown {
                reason: format!("getent returned no passwd entry for {user}"),
            });
        };
        let current_name = std::path::Path::new(shell)
            .file_name()
            .and_then(|name| name.to_str());
        if current_name == Some(&self.target_shell) {
            Ok(ResourceState::Correct)
        } else {
            Ok(ResourceState::Incorrect {
                current: shell.to_string(),
            })
        }
    }

    fn non_interactive_sudo_available(&self) -> ResourceResult<bool> {
        if !self.executor.which("sudo") {
            return Ok(false);
        }
        let result = self
            .executor
            .execute(CommandSpec::new("sudo").args(&["-n", "true"]).unchecked())?;
        Ok(result.success)
    }
}

#[cfg(unix)]
fn process_is_root() -> bool {
    nix::unistd::Uid::effective().is_root()
}

#[cfg(not(unix))]
const fn process_is_root() -> bool {
    false
}

fn passwd_shell<'a>(line: &'a str, expected_user: &str) -> Option<&'a str> {
    let fields = line.split(':').collect::<Vec<_>>();
    (fields.len() == 7 && fields.first().copied() == Some(expected_user))
        .then(|| fields.get(6).copied())
        .flatten()
        .filter(|shell| !shell.is_empty())
}

fn failure_details(result: &ExecResult) -> String {
    let status = result.code.map_or_else(
        || "terminated by signal".to_string(),
        |code| format!("exit {code}"),
    );
    let stdout = result.stdout.trim();
    let stderr = result.stderr.trim();
    format!(
        "{status}; stdout: {}; stderr: {}",
        if stdout.is_empty() { "<empty>" } else { stdout },
        if stderr.is_empty() { "<empty>" } else { stderr }
    )
}

impl Resource for DefaultShellResource {
    fn description(&self) -> String {
        format!("default shell → {}", self.target_shell)
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        let shell_path = self.executor.which_path(&self.target_shell)?;
        let shell_str = shell_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("non-UTF-8 shell path: {}", shell_path.display()))?;
        let user = self.target_user()?;

        if self.running_as_root {
            self.executor.execute(CommandSpec::new("usermod").args(&[
                "-s",
                shell_str,
                user.as_str(),
            ]))?;
        } else if self.non_interactive_sudo_available()? {
            self.executor.execute(CommandSpec::new("sudo").args(&[
                "-n",
                "usermod",
                "-s",
                shell_str,
                user.as_str(),
            ]))?;
        } else {
            // Preserve the normal installed-system path when passwordless or
            // cached sudo is unavailable: chsh authenticates through the TTY.
            self.executor
                .execute(CommandSpec::new("chsh").args(&["-s", shell_str]))?;
        }
        Ok(ResourceChange::Applied)
    }
}

impl IntrinsicState for DefaultShellResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        let user = self.target_user()?;
        self.account_shell(&user)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::env::MapEnv;
    use crate::infra::exec::{ExecError, MockExecutor};
    use std::path::PathBuf;

    fn env_for(user: &str) -> Arc<dyn Env> {
        MapEnv::new().with("USER", user).into_handle()
    }

    fn passwd(user: &str, shell: &str) -> String {
        format!("{user}:x:1000:1000::/home/{user}:{shell}\n")
    }

    #[test]
    fn description_includes_shell_name() {
        let executor: Arc<dyn Executor> = Arc::new(crate::infra::exec::ProcessExecutor::system());
        let resource = DefaultShellResource::new("zsh".to_string(), executor, env_for("stuart"));
        assert_eq!(resource.description(), "default shell → zsh");
    }

    #[test]
    fn current_state_reads_passwd_database_instead_of_shell_environment() {
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .withf(|spec| {
                spec.program() == "getent"
                    && spec.arguments() == ["passwd", "stuart"]
                    && !spec.is_checked()
            })
            .returning(|_| Ok(ExecResult::success(passwd("stuart", "/usr/bin/zsh"))));
        let env = MapEnv::new()
            .with("USER", "stuart")
            .with("SHELL", "/bin/bash")
            .into_handle();
        let resource = DefaultShellResource::new("zsh".to_string(), Arc::new(mock), env);

        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn non_root_runuser_context_ignores_an_inherited_sudo_user() {
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .withf(|spec| spec.arguments() == ["passwd", "new-user"])
            .returning(|_| Ok(ExecResult::success(passwd("new-user", "/usr/bin/zsh"))));
        let env = MapEnv::new()
            .with("USER", "new-user")
            .with("SUDO_USER", "installer-operator")
            .into_handle();
        let resource =
            DefaultShellResource::new("zsh".to_string(), Arc::new(mock), env).with_root(false);

        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn current_state_treats_bin_and_usr_bin_shells_as_equivalent() {
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .returning(|_| Ok(ExecResult::success(passwd("stuart", "/bin/zsh"))));
        let resource =
            DefaultShellResource::new("zsh".to_string(), Arc::new(mock), env_for("stuart"));

        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn current_state_reports_different_account_shell() {
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .returning(|_| Ok(ExecResult::success(passwd("stuart", "/bin/bash"))));
        let resource =
            DefaultShellResource::new("zsh".to_string(), Arc::new(mock), env_for("stuart"));

        assert_eq!(
            resource.current_state().unwrap(),
            ResourceState::Incorrect {
                current: "/bin/bash".to_string()
            }
        );
    }

    #[test]
    fn root_install_context_uses_usermod_for_target_user() {
        let mut mock = MockExecutor::new();
        mock.expect_which_path()
            .once()
            .returning(|program| Ok(PathBuf::from(format!("/usr/bin/{program}"))));
        mock.expect_execute()
            .once()
            .withf(|spec| {
                spec.program() == "usermod"
                    && spec.arguments() == ["-s", "/usr/bin/zsh", "stuart"]
                    && spec.is_checked()
            })
            .returning(|_| Ok(ExecResult::success("")));
        let env = MapEnv::new()
            .with("USER", "root")
            .with("SUDO_USER", "stuart")
            .into_handle();
        let resource =
            DefaultShellResource::new("zsh".to_string(), Arc::new(mock), env).with_root(true);

        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }

    #[test]
    fn non_interactive_install_uses_passwordless_sudo_usermod() {
        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        mock.expect_which_path()
            .once()
            .returning(|_| Ok(PathBuf::from("/usr/bin/zsh")));
        mock.expect_which()
            .once()
            .withf(|program| program == "sudo")
            .returning(|_| true);
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .withf(|spec| {
                spec.program() == "sudo" && spec.arguments() == ["-n", "true"] && !spec.is_checked()
            })
            .returning(|_| Ok(ExecResult::success("")));
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .withf(|spec| {
                spec.program() == "sudo"
                    && spec.arguments() == ["-n", "usermod", "-s", "/usr/bin/zsh", "stuart"]
                    && spec.is_checked()
            })
            .returning(|_| Ok(ExecResult::success("")));
        let resource =
            DefaultShellResource::new("zsh".to_string(), Arc::new(mock), env_for("stuart"))
                .with_root(false);

        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }

    #[test]
    fn interactive_context_falls_back_to_chsh() {
        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        mock.expect_which_path()
            .once()
            .returning(|_| Ok(PathBuf::from("/usr/bin/zsh")));
        mock.expect_which().once().returning(|_| true);
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .returning(|_| Ok(ExecResult::failure("", "password required", Some(1))));
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .withf(|spec| {
                spec.program() == "chsh"
                    && spec.arguments() == ["-s", "/usr/bin/zsh"]
                    && spec.is_checked()
            })
            .returning(|_| Ok(ExecResult::success("")));
        let resource =
            DefaultShellResource::new("zsh".to_string(), Arc::new(mock), env_for("stuart"))
                .with_root(false);

        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }

    #[test]
    fn apply_propagates_account_mutation_failure() {
        let mut mock = MockExecutor::new();
        mock.expect_which_path()
            .once()
            .returning(|_| Ok(PathBuf::from("/usr/bin/zsh")));
        mock.expect_execute().once().returning(|_| {
            Err(ExecError::spawn(
                "usermod -s /usr/bin/zsh stuart",
                std::io::Error::other("account database unavailable"),
            ))
        });
        let resource =
            DefaultShellResource::new("zsh".to_string(), Arc::new(mock), env_for("stuart"))
                .with_root(true);

        assert!(resource.apply().is_err());
    }
}
