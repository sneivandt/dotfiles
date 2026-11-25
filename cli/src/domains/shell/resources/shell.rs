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
pub struct DefaultShellResource {
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
    pub fn new(target_shell: String, executor: Arc<dyn Executor>, env: Arc<dyn Env>) -> Self {
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
    pub const fn with_interaction(mut self, allow_prompt: bool) -> Self {
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
mod tests {
    use super::*;
    use crate::infra::env::MapEnv;
    use crate::infra::exec::{ExecError, MockExecutor};
    use std::path::PathBuf;

    #[test]
    fn root_cannot_set_an_unregistered_login_shell() {
        let mut resource = DefaultShellResource::new(
            "zsh".into(),
            Arc::new(MockExecutor::new()),
            env_for("stuart"),
        )
        .with_root(true);
        resource.allowed_shells =
            RegisteredShells::Fixture("/bin/bash\n# /usr/bin/zsh\n/usr/local/bin/zsh\n");
        let error = resource.apply().unwrap_err();
        assert!(
            error
                .to_string()
                .contains("is not registered in /etc/shells")
        );
    }

    #[test]
    fn repair_uses_the_registered_system_alias_and_converges_on_the_next_login() {
        let mut mock = MockExecutor::new();
        let mut sequence = mockall::Sequence::new();
        for (installed, mutate) in [(SYSTEM_ZSH, true), ("/bin/zsh", false)] {
            mock.expect_execute()
                .once()
                .in_sequence(&mut sequence)
                .withf(|spec| spec.program() == "getent" && spec.arguments() == ["passwd", "kiosk"])
                .returning(move |_| Ok(ExecResult::success(passwd("kiosk", installed))));
            mock.expect_which()
                .once()
                .in_sequence(&mut sequence)
                .withf(move |program| program == installed)
                .return_const(true);
            if mutate {
                mock.expect_which_path()
                    .once()
                    .in_sequence(&mut sequence)
                    .withf(|program| program == "/bin/zsh")
                    .returning(|program| Ok(PathBuf::from(program)));
                mock.expect_execute()
                    .once()
                    .in_sequence(&mut sequence)
                    .withf(|spec| {
                        spec.program() == "usermod"
                            && spec.is_checked()
                            && spec.arguments() == ["-s", "/bin/zsh", "kiosk"]
                    })
                    .returning(|_| Ok(ExecResult::success("")));
            }
        }
        let mut resource =
            DefaultShellResource::new("zsh".into(), Arc::new(mock), env_for("kiosk"))
                .with_root(true);
        resource.allowed_shells =
            RegisteredShells::Fixture("# site-managed shell list\n/bin/bash\n/bin/zsh\n");
        assert_eq!(
            resource.current_state().unwrap(),
            ResourceState::Incorrect {
                current: SYSTEM_ZSH.into()
            }
        );
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn missing_shell_registry_blocks_an_account_write_until_the_file_is_restored() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let path = fixture.path().join("shells");
        let mut mock = MockExecutor::new();
        mock.expect_which_path()
            .withf(|program| program == "/bin/zsh")
            .once()
            .returning(|program| Ok(PathBuf::from(program)));
        mock.expect_execute()
            .once()
            .withf(|spec| {
                spec.program() == "usermod"
                    && spec.arguments() == ["-s", "/bin/zsh", "kiosk"]
                    && spec.is_checked()
            })
            .returning(|_| Ok(ExecResult::success("")));
        let mut resource =
            DefaultShellResource::new("zsh".into(), Arc::new(mock), env_for("kiosk"))
                .with_root(true);
        resource.allowed_shells = RegisteredShells::File(path.clone());
        assert!(
            resource
                .apply()
                .unwrap_err()
                .to_string()
                .contains("reading allowed login shells")
        );
        assert!(!path.exists());
        let restored = "# restored administrator policy\n/bin/sh\n/bin/zsh\n";
        std::fs::write(&path, restored).unwrap();
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        assert_eq!(std::fs::read_to_string(path).unwrap(), restored);
    }

    #[test]
    fn non_interactive_authentication_never_invokes_chsh() {
        for sudo_present in [false, true] {
            let mut mock = MockExecutor::new();
            mock.expect_which_path()
                .once()
                .returning(|_| Ok(PathBuf::from("/usr/bin/zsh")));
            mock.expect_which()
                .once()
                .withf(|program| program == "sudo")
                .return_const(sudo_present);
            if sudo_present {
                mock.expect_execute()
                    .once()
                    .withf(|spec| {
                        spec.program() == "sudo"
                            && spec.arguments()
                                == ["-n", "-l", "--", "usermod", "-s", "/usr/bin/zsh", "stuart"]
                    })
                    .returning(|_| Ok(ExecResult::failure("", "password required", Some(1))));
            }
            let resource =
                DefaultShellResource::new("zsh".into(), Arc::new(mock), env_for("stuart"))
                    .with_root(false)
                    .with_interaction(false);
            assert!(
                matches!(resource.apply().unwrap(), ResourceChange::Skipped { kind: crate::engine::SkipKind::UnmetWork, reason } if reason.contains("authentication"))
            );
        }
    }

    fn env_for(user: &str) -> Arc<dyn Env> {
        MapEnv::new().with("USER", user).into_handle()
    }

    fn passwd(user: &str, shell: &str) -> String {
        format!("{user}:x:1000:1000::/home/{user}:{shell}\n")
    }

    fn expect_passwd(mock: &mut MockExecutor, user: &'static str, result: ExecResult) {
        mock.expect_execute()
            .once()
            .withf(move |spec| {
                spec.program() == "getent"
                    && spec.arguments() == ["passwd", user]
                    && !spec.is_checked()
                    && spec.working_dir().is_none()
            })
            .return_once(|_| Ok(result));
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
        mock.expect_which()
            .once()
            .withf(|program| program == "/usr/bin/zsh")
            .return_const(true);
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
        mock.expect_which()
            .once()
            .withf(|program| program == "/usr/bin/zsh")
            .return_const(true);
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
        mock.expect_which()
            .once()
            .withf(|program| program == "/bin/zsh")
            .return_const(true);
        expect_passwd(
            &mut mock,
            "stuart",
            ExecResult::success(passwd("stuart", "/bin/zsh")),
        );
        let resource =
            DefaultShellResource::new("zsh".to_string(), Arc::new(mock), env_for("stuart"));

        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn current_state_reports_different_account_shell() {
        let mut mock = MockExecutor::new();
        expect_passwd(
            &mut mock,
            "stuart",
            ExecResult::success(passwd("stuart", "/bin/bash")),
        );
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
    fn unavailable_account_shell_is_repaired_and_converges() {
        use crate::engine::{ProcessOpts, process_resources};
        use crate::test_helpers::{make_linux_context, task_batch};

        for dry_run in [true, false] {
            let mut mock = MockExecutor::new();
            let mut sequence = mockall::Sequence::new();
            mock.expect_execute()
                .once()
                .in_sequence(&mut sequence)
                .withf(|spec| {
                    spec.program() == "getent"
                        && spec.arguments() == ["passwd", "stuart"]
                        && !spec.is_checked()
                })
                .returning(|_| Ok(ExecResult::success(passwd("stuart", "/removed/bin/zsh"))));
            if !dry_run {
                mock.expect_which_path()
                    .once()
                    .withf(|program| program == SYSTEM_ZSH)
                    .returning(|_| Ok(PathBuf::from("/usr/bin/zsh")));
                mock.expect_which()
                    .once()
                    .withf(|program| program == "sudo")
                    .return_const(false);
                mock.expect_execute()
                    .once()
                    .in_sequence(&mut sequence)
                    .withf(|spec| {
                        spec.program() == "chsh"
                            && spec.arguments() == ["-s", "/usr/bin/zsh"]
                            && spec.is_checked()
                    })
                    .returning(|_| Ok(ExecResult::success("")));
                mock.expect_execute()
                    .once()
                    .in_sequence(&mut sequence)
                    .withf(|spec| {
                        spec.program() == "getent"
                            && spec.arguments() == ["passwd", "stuart"]
                            && !spec.is_checked()
                    })
                    .returning(|_| Ok(ExecResult::success(passwd("stuart", "/usr/bin/zsh"))));
                mock.expect_which()
                    .once()
                    .withf(|program| program == "/usr/bin/zsh")
                    .return_const(true);
            }
            let executor: Arc<dyn Executor> = Arc::new(mock);
            let ctx = make_linux_context(PathBuf::from("/repo"), None).with_dry_run(dry_run);
            let run = || {
                process_resources(
                    &ctx,
                    [DefaultShellResource::new(
                        "zsh".to_string(),
                        Arc::clone(&executor),
                        env_for("stuart"),
                    )
                    .with_root(false)
                    .with_interaction(true)],
                    &ProcessOpts::strict("configure"),
                )
                .unwrap()
            };

            assert_eq!(
                task_batch(&run()).changed_count(),
                1,
                "a removed or non-executable login shell needs repair; dry_run={dry_run}"
            );
            if !dry_run {
                assert_eq!(task_batch(&run()).already_ok_count(), 1);
            }
        }
    }

    #[test]
    fn account_lookup_rejects_unusable_records_without_claiming_missing_state() {
        for (case, result, reason) in [
            ("no record", ExecResult::success(""), "no passwd entry"),
            (
                "different account",
                ExecResult::success(passwd("another-user", "/bin/zsh")),
                "no passwd entry",
            ),
            (
                "missing field",
                ExecResult::success("stuart:x:1000:1000:/home/stuart:/bin/zsh"),
                "no passwd entry",
            ),
            (
                "empty shell",
                ExecResult::success(passwd("stuart", "")),
                "no passwd entry",
            ),
            (
                "lookup failure",
                ExecResult::failure(" partial output \n", " database unavailable \n", Some(2)),
                "exit 2; stdout: partial output; stderr: database unavailable",
            ),
            (
                "terminated lookup",
                ExecResult::failure("", "", None),
                "terminated by signal; stdout: <empty>; stderr: <empty>",
            ),
        ] {
            let mut mock = MockExecutor::new();
            expect_passwd(&mut mock, "stuart", result);
            let resource =
                DefaultShellResource::new("zsh".to_string(), Arc::new(mock), env_for("stuart"));

            let state = resource.current_state().unwrap();
            assert!(
                matches!(state, ResourceState::Unknown { reason: ref actual } if actual.contains(reason)),
                "{case}: {state:?}"
            );
        }
    }

    #[test]
    fn account_lookup_selects_target_user_after_unrelated_and_malformed_records() {
        let mut mock = MockExecutor::new();
        mock.expect_which()
            .once()
            .withf(|program| program == "/usr/bin/zsh")
            .return_const(true);
        expect_passwd(
            &mut mock,
            "stuart",
            ExecResult::success(format!(
                "malformed\n{}{}",
                passwd("another-user", "/bin/bash"),
                passwd("stuart", "/usr/bin/zsh")
            )),
        );
        let resource =
            DefaultShellResource::new("zsh".to_string(), Arc::new(mock), env_for("stuart"));

        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn account_selection_falls_back_without_consulting_the_host_environment() {
        for (case, root, user, logname, sudo_user, expected) in [
            ("root sudo user", true, "root", "login", "stuart", "stuart"),
            (
                "ignore sudo as user",
                false,
                "stuart",
                "login",
                "operator",
                "stuart",
            ),
            (
                "ignore root sudo user",
                true,
                "stuart",
                "login",
                "root",
                "stuart",
            ),
            (
                "ignore empty sudo user",
                true,
                "stuart",
                "login",
                "",
                "stuart",
            ),
            ("empty USER uses LOGNAME", false, "", "login", "", "login"),
        ] {
            let env = MapEnv::new()
                .with("USER", user)
                .with("LOGNAME", logname)
                .with("SUDO_USER", sudo_user)
                .into_handle();
            let resource =
                DefaultShellResource::new("zsh".to_string(), Arc::new(MockExecutor::new()), env)
                    .with_root(root);
            assert_eq!(resource.target_user().unwrap(), expected, "{case}");
        }
        let resource = DefaultShellResource::new(
            "zsh".to_string(),
            Arc::new(MockExecutor::new()),
            MapEnv::new()
                .with("USER", "")
                .with("LOGNAME", "")
                .into_handle(),
        )
        .with_root(false);
        let error = resource.current_state().unwrap_err().to_string();
        assert!(error.contains("USER and LOGNAME are not set"), "{error}");
    }

    #[test]
    fn root_install_context_uses_usermod_for_target_user() {
        let mut mock = MockExecutor::new();
        mock.expect_which_path()
            .once()
            .withf(|program| program == SYSTEM_ZSH)
            .returning(|program| Ok(PathBuf::from(program)));
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
    fn non_interactive_install_accepts_command_scoped_sudo_usermod() {
        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        mock.expect_which_path()
            .once()
            .withf(|program| program == SYSTEM_ZSH)
            .returning(|_| Ok(PathBuf::from("/usr/bin/zsh")));
        mock.expect_which()
            .once()
            .withf(|program| program == "sudo")
            .returning(|_| true);
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .withf(|spec| {
                spec.program() == "sudo"
                    && spec.arguments()
                        == ["-n", "-l", "--", "usermod", "-s", "/usr/bin/zsh", "stuart"]
                    && !spec.is_checked()
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
            .withf(|program| program == SYSTEM_ZSH)
            .returning(|_| Ok(PathBuf::from("/usr/bin/zsh")));
        mock.expect_which()
            .once()
            .withf(|program| program == "sudo")
            .returning(|_| true);
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .withf(|spec| {
                spec.program() == "sudo"
                    && spec.arguments()
                        == ["-n", "-l", "--", "usermod", "-s", "/usr/bin/zsh", "stuart"]
                    && !spec.is_checked()
            })
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
                .with_root(false)
                .with_interaction(true);

        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }

    #[test]
    fn missing_sudo_falls_back_to_chsh_without_a_probe() {
        let mut mock = MockExecutor::new();
        mock.expect_which_path()
            .once()
            .withf(|program| program == SYSTEM_ZSH)
            .returning(|_| Ok(PathBuf::from("/usr/bin/zsh")));
        mock.expect_which()
            .once()
            .withf(|program| program == "sudo")
            .return_const(false);
        mock.expect_execute()
            .once()
            .withf(|spec| {
                spec.program() == "chsh"
                    && spec.arguments() == ["-s", "/usr/bin/zsh"]
                    && spec.is_checked()
            })
            .returning(|_| Ok(ExecResult::success("")));
        let resource =
            DefaultShellResource::new("zsh".to_string(), Arc::new(mock), env_for("stuart"))
                .with_root(false)
                .with_interaction(true);

        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }

    #[test]
    fn apply_propagates_account_mutation_failure() {
        let mut mock = MockExecutor::new();
        mock.expect_which_path()
            .once()
            .withf(|program| program == SYSTEM_ZSH)
            .returning(|_| Ok(PathBuf::from("/usr/bin/zsh")));
        mock.expect_execute()
            .once()
            .withf(|spec| {
                spec.program() == "usermod"
                    && spec.arguments() == ["-s", "/usr/bin/zsh", "stuart"]
                    && spec.is_checked()
            })
            .returning(|_| {
                Err(ExecError::non_zero(
                    "usermod -s /usr/bin/zsh stuart",
                    ExecResult::failure("", "account database unavailable", Some(1)),
                ))
            });
        let resource =
            DefaultShellResource::new("zsh".to_string(), Arc::new(mock), env_for("stuart"))
                .with_root(true);

        let error = resource.apply().unwrap_err().to_string();
        assert!(error.contains("account database unavailable"), "{error}");
    }

    #[test]
    fn account_lookup_spawn_failure_is_not_reported_as_missing_state() {
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .withf(|spec| {
                spec.program() == "getent"
                    && spec.arguments() == ["passwd", "stuart"]
                    && !spec.is_checked()
                    && spec.working_dir().is_none()
            })
            .returning(|_| {
                Err(ExecError::spawn(
                    "getent",
                    std::io::Error::other("account lookup could not start"),
                ))
            });
        let resource =
            DefaultShellResource::new("zsh".to_string(), Arc::new(mock), env_for("stuart"))
                .with_root(false);

        let error = resource.current_state().unwrap_err().to_string();
        assert!(error.contains("account lookup could not start"), "{error}");
    }

    #[test]
    fn sudo_probe_spawn_failure_does_not_fall_back_to_interactive_chsh() {
        let mut mock = MockExecutor::new();
        mock.expect_which_path()
            .once()
            .withf(|program| program == SYSTEM_ZSH)
            .returning(|_| Ok(PathBuf::from("/usr/bin/zsh")));
        mock.expect_which()
            .once()
            .withf(|program| program == "sudo")
            .return_const(true);
        mock.expect_execute()
            .once()
            .withf(|spec| {
                spec.program() == "sudo"
                    && spec.arguments()
                        == ["-n", "-l", "--", "usermod", "-s", "/usr/bin/zsh", "stuart"]
                    && !spec.is_checked()
                    && spec.working_dir().is_none()
            })
            .returning(|_| {
                Err(ExecError::spawn(
                    "sudo",
                    std::io::Error::other("sudo probe could not start"),
                ))
            });
        let resource =
            DefaultShellResource::new("zsh".to_string(), Arc::new(mock), env_for("stuart"))
                .with_root(false);

        let error = resource.apply().unwrap_err().to_string();
        assert!(error.contains("sudo probe could not start"), "{error}");
    }

    #[test]
    fn target_user_rejects_option_like_and_malformed_account_names() {
        for bad in [
            "-f",
            "--help",
            "user:0",
            "user\nname",
            "user\rname",
            "user\0name",
        ] {
            let resource = DefaultShellResource::new(
                "zsh".into(),
                Arc::new(MockExecutor::new()),
                env_for(bad),
            )
            .with_root(true);
            let error = resource.current_state().unwrap_err().to_string();
            assert!(
                error.contains("invalid target account name"),
                "{bad:?}: {error}"
            );
        }
    }

    #[test]
    fn registered_shell_falls_back_to_listed_alias_when_preferred_path_is_missing() {
        let mut mock = MockExecutor::new();
        let mut seq = mockall::Sequence::new();
        mock.expect_which_path()
            .once()
            .in_sequence(&mut seq)
            .withf(|program| program == SYSTEM_ZSH)
            .returning(|_| Err(anyhow::anyhow!("/usr/bin/zsh not found")));
        mock.expect_which_path()
            .once()
            .in_sequence(&mut seq)
            .withf(|program| program == "/bin/zsh")
            .returning(|program| Ok(PathBuf::from(program)));
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .withf(|spec| {
                spec.program() == "usermod"
                    && spec.arguments() == ["-s", "/bin/zsh", "stuart"]
                    && spec.is_checked()
            })
            .returning(|_| Ok(ExecResult::success("")));
        let resource = DefaultShellResource::new("zsh".into(), Arc::new(mock), env_for("stuart"))
            .with_root(true);

        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }

    #[test]
    fn registered_shell_reports_lookup_error_when_all_listed_paths_are_missing() {
        let mut mock = MockExecutor::new();
        let mut seq = mockall::Sequence::new();
        mock.expect_which_path()
            .once()
            .in_sequence(&mut seq)
            .withf(|program| program == SYSTEM_ZSH)
            .returning(|_| Err(anyhow::anyhow!("/usr/bin/zsh not found")));
        mock.expect_which_path()
            .once()
            .in_sequence(&mut seq)
            .withf(|program| program == "/bin/zsh")
            .returning(|_| Err(anyhow::anyhow!("/bin/zsh not found")));
        let resource = DefaultShellResource::new("zsh".into(), Arc::new(mock), env_for("stuart"))
            .with_root(true);

        let error = resource.apply().unwrap_err().to_string();
        assert!(error.contains("/bin/zsh not found"), "{error}");
    }
}
