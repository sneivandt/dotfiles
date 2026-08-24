//! Task: configure PAM to unlock GNOME Keyring at console login.

use std::path::PathBuf;

use anyhow::Result;

use crate::domains::system::resources::pam_keyring::{PamKeyringResource, PamKeyringService};
use crate::engine::{
    Context, IntrinsicState, ProcessOpts, ResourceState, Task, TaskResult, process_resources,
    task_metadata,
};
use crate::infra::ConfigHandle;

/// Configure console-login keyring unlocking and password synchronization.
#[derive(Debug)]
pub struct ConfigurePamKeyring {
    enabled: ConfigHandle<bool>,
    login_path: PathBuf,
    passwd_path: PathBuf,
    temp_dir: PathBuf,
}

impl ConfigurePamKeyring {
    /// Create the system PAM task when GNOME Keyring belongs to the active profile.
    #[must_use]
    pub fn new(enabled: ConfigHandle<bool>) -> Self {
        Self {
            enabled,
            login_path: PathBuf::from("/etc/pam.d/login"),
            passwd_path: PathBuf::from("/etc/pam.d/passwd"),
            temp_dir: PathBuf::from("/tmp"),
        }
    }

    fn resources(&self, ctx: &Context) -> [PamKeyringResource; 2] {
        let executor = ctx.system().executor_arc();
        [
            PamKeyringResource::new(
                PamKeyringService::Login,
                &self.login_path,
                std::sync::Arc::clone(&executor),
                &self.temp_dir,
            ),
            PamKeyringResource::new(
                PamKeyringService::Passwd,
                &self.passwd_path,
                executor,
                &self.temp_dir,
            ),
        ]
    }

    #[cfg(test)]
    fn with_paths(
        mut self,
        login: &std::path::Path,
        passwd: &std::path::Path,
        temp_dir: &std::path::Path,
    ) -> Self {
        self.login_path = login.to_path_buf();
        self.passwd_path = passwd.to_path_buf();
        self.temp_dir = temp_dir.to_path_buf();
        self
    }
}

impl Task for ConfigurePamKeyring {
    task_metadata! {
        name: "GNOME Keyring PAM integration",
        selector: "pam-keyring",
    }

    fn should_run(&self, ctx: &Context) -> bool {
        *self.enabled.read() && ctx.platform().uses_pacman() && !ctx.system().is_ci()
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        !ctx.system().is_elevated()
            && self.resources(ctx).iter().any(|resource| {
                matches!(
                    resource.current_state(),
                    Ok(ResourceState::Incorrect { .. })
                )
            })
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        process_resources(
            ctx,
            self.resources(ctx),
            &ProcessOpts::strict("configure").sequential(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::platform::Os;
    use crate::test_helpers::{ContextBuilder, empty_config};
    use std::fs;
    use tempfile::TempDir;

    const LOGIN_BASE: &str = "\
#%PAM-1.0
auth include system-local-login
account include system-local-login
session include system-local-login
password include system-local-login
";
    const PASSWD_BASE: &str = "\
#%PAM-1.0
auth include system-auth
account include system-auth
password include system-auth
";

    fn arch_context() -> Context {
        ContextBuilder::new(empty_config(PathBuf::from("/tmp")))
            .os(Os::Linux)
            .arch(true)
            .build()
    }

    fn task(enabled: bool) -> ConfigurePamKeyring {
        ConfigurePamKeyring::new(ConfigHandle::new(enabled))
    }

    #[test]
    fn runs_only_for_enabled_arch_profile_outside_ci() {
        let arch = arch_context();
        let linux = ContextBuilder::new(empty_config(PathBuf::from("/tmp"))).build();
        let ci = ContextBuilder::new(empty_config(PathBuf::from("/tmp")))
            .arch(true)
            .ci(true)
            .build();

        assert!(task(true).should_run(&arch));
        assert!(!task(false).should_run(&arch));
        assert!(!task(true).should_run(&linux));
        assert!(!task(true).should_run(&ci));
    }

    #[test]
    #[cfg(not(windows))]
    fn elevation_is_requested_only_while_configuration_differs() {
        // Off Windows the process is never considered elevated, so this test
        // exercises the resource-state branch without depending on host state.
        let temp = TempDir::new().unwrap();
        let login = temp.path().join("login");
        let passwd = temp.path().join("passwd");
        fs::write(&login, LOGIN_BASE).unwrap();
        fs::write(&passwd, PASSWD_BASE).unwrap();
        let task = task(true).with_paths(&login, &passwd, temp.path());
        let ctx = arch_context();

        assert!(task.needs_elevation(&ctx));

        fs::write(
            &login,
            "\
#%PAM-1.0
auth include system-local-login
auth       optional     pam_gnome_keyring.so
account include system-local-login
session include system-local-login
session    optional     pam_gnome_keyring.so auto_start
password include system-local-login
",
        )
        .unwrap();
        fs::write(
            &passwd,
            "\
#%PAM-1.0
auth include system-auth
account include system-auth
password include system-auth
password   optional     pam_gnome_keyring.so
",
        )
        .unwrap();

        assert!(!task.needs_elevation(&ctx));
    }

    #[test]
    fn dry_run_reports_changes_without_modifying_pam_files() {
        let temp = TempDir::new().unwrap();
        let login = temp.path().join("login");
        let passwd = temp.path().join("passwd");
        fs::write(&login, LOGIN_BASE).unwrap();
        fs::write(&passwd, PASSWD_BASE).unwrap();
        let task = task(true).with_paths(&login, &passwd, temp.path());
        let ctx = arch_context().with_dry_run(true);

        let result = task.run(&ctx).unwrap();

        assert!(
            matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() == 2),
            "both PAM files should be planned: {result:?}"
        );
        assert_eq!(fs::read_to_string(login).unwrap(), LOGIN_BASE);
        assert_eq!(fs::read_to_string(passwd).unwrap(), PASSWD_BASE);
    }
}
