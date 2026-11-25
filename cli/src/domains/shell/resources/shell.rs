//! Login shell configuration resource.
use std::sync::Arc;

use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::infra::env::Env;
use crate::infra::exec::{CommandSpec, ExecResult, Executor};

pub(crate) const SYSTEM_ZSH: &str = "/usr/bin/zsh";

#[derive(Debug)]
enum RegisteredShells {
    File(std::path::PathBuf),
    #[cfg(test)]
    Fixture(&'static str),
}

impl RegisteredShells {
    fn read(&self) -> anyhow::Result<String> {
        use anyhow::Context as _;

        match self {
            Self::File(path) => std::fs::read_to_string(path)
                .with_context(|| format!("reading allowed login shells from {}", path.display())),
            #[cfg(test)]
            Self::Fixture(text) => Ok((*text).to_owned()),
        }
    }
}

/// A resource for configuring the current account's default login shell.
#[derive(Debug)]
pub(crate) struct DefaultShellResource {
    target_shell: String,
    executor: Arc<dyn Executor>,
    env: Arc<dyn Env>,
    running_as_root: bool,
    allow_prompt: bool,
    allowed_shells: RegisteredShells,
}

impl DefaultShellResource {
    /// Create a new default shell resource.
    #[must_use]
    pub(crate) fn new(
        target_shell: String,
        executor: Arc<dyn Executor>,
        env: Arc<dyn Env>,
    ) -> Self {
        Self {
            target_shell,
            executor,
            env,
            #[cfg(unix)]
            running_as_root: nix::unistd::Uid::effective().is_root(),
            #[cfg(not(unix))]
            running_as_root: false,
            allow_prompt: false,
            #[cfg(not(test))]
            allowed_shells: RegisteredShells::File("/etc/shells".into()),
            #[cfg(test)]
            allowed_shells: RegisteredShells::Fixture("/bin/sh\n/usr/bin/zsh\n/bin/zsh\n"),
        }
    }

    /// Apply the command's resolved policy for authentication prompts.
    #[must_use]
    pub(crate) const fn with_interaction(mut self, allow_prompt: bool) -> Self {
        self.allow_prompt = allow_prompt;
        self
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
        let user = sudo_user
            .or_else(|| self.env.var("USER").filter(|user| !user.is_empty()))
            .or_else(|| self.env.var("LOGNAME").filter(|user| !user.is_empty()))
            .ok_or_else(|| anyhow::anyhow!("USER and LOGNAME are not set"))?;
        if user.starts_with('-') || user.contains([':', '\0', '\n', '\r']) {
            return Err(anyhow::anyhow!("invalid target account name: {user:?}").into());
        }
        Ok(user)
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
        let system_alias = format!("/bin/{}", self.target_shell);
        if (shell == self.system_shell() || shell == system_alias)
            && self.executor.which(shell)
            && shell_is_listed(&self.allowed_shells.read()?, shell)
        {
            Ok(ResourceState::Correct)
        } else {
            Ok(ResourceState::Incorrect {
                current: shell.to_string(),
            })
        }
    }

    fn system_shell(&self) -> String {
        format!("/usr/bin/{}", self.target_shell)
    }

    fn registered_system_shell(&self) -> anyhow::Result<std::path::PathBuf> {
        let allowed = self.allowed_shells.read()?;
        let preferred = self.system_shell();
        let alias = format!("/bin/{}", self.target_shell);
        let mut last_error = None;
        for candidate in [&preferred, &alias] {
            if shell_is_listed(&allowed, candidate) {
                match self.executor.which_path(candidate) {
                    Ok(path) => return Ok(path),
                    Err(error) => last_error = Some(error),
                }
            }
        }
        if let Some(error) = last_error {
            return Err(error);
        }
        anyhow::bail!("{preferred} is not registered in /etc/shells (also checked {alias})")
    }

    fn non_interactive_sudo_available(&self, shell: &str, user: &str) -> ResourceResult<bool> {
        if !self.executor.which("sudo") {
            return Ok(false);
        }
        let result = self.executor.execute(
            CommandSpec::new("sudo")
                .args(&["-n", "-l", "--", "usermod", "-s", shell, user])
                .unchecked(),
        )?;
        Ok(result.success)
    }
}

fn shell_is_listed(allowed: &str, shell: &str) -> bool {
    allowed.lines().any(|line| line.trim() == shell)
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
        let shell_path = self.registered_system_shell()?;
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
        } else if self.non_interactive_sudo_available(shell_str, &user)? {
            self.executor.execute(CommandSpec::new("sudo").args(&[
                "-n",
                "usermod",
                "-s",
                shell_str,
                user.as_str(),
            ]))?;
        } else if self.allow_prompt {
            // Preserve the normal installed-system path when passwordless or
            // cached sudo is unavailable: chsh authenticates through the TTY.
            self.executor
                .execute(CommandSpec::new("chsh").args(&["-s", shell_str]))?;
        } else {
            return Ok(ResourceChange::unusable(
                "changing the login shell requires authentication; run interactively or configure passwordless sudo for usermod",
            ));
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
#[path = "tests.rs"]
mod tests;
