//! Task registration: the complete set of install and uninstall tasks.

use crate::engine::update_signal::UpdateSignal;
use crate::tasks::Task;

/// The complete set of tasks run by the uninstall command.
#[must_use]
pub fn all_uninstall_tasks() -> Vec<Box<dyn Task>> {
    vec![
        Box::new(crate::tasks::files::symlinks::UninstallSymlinks),
        Box::new(crate::tasks::git::hooks::UninstallGitHooks::new()),
        Box::new(crate::tasks::core::wrapper::UninstallWrapper),
    ]
}

/// The complete set of tasks run by the install command.
///
/// Order within the list is arbitrary — the scheduler derives execution order
/// from each task's [`Task::dependencies`] declaration.
#[must_use]
pub fn all_install_tasks() -> Vec<Box<dyn Task>> {
    let repo_updated = UpdateSignal::new();
    vec![
        Box::new(crate::tasks::system::developer_mode::EnableDeveloperMode),
        Box::new(crate::tasks::repository::sparse_checkout::ConfigureSparseCheckout::new()),
        Box::new(crate::tasks::repository::update::UpdateRepository::new(
            repo_updated.clone(),
        )),
        Box::new(crate::tasks::git::git_config::ConfigureGit),
        Box::new(crate::tasks::ai::copilot_settings::ConfigureCopilot),
        Box::new(crate::tasks::git::hooks::InstallGitHooks::new()),
        Box::new(crate::tasks::shell::completions::GenerateCompletions),
        Box::new(crate::tasks::packages::InstallPackages),
        Box::new(crate::tasks::packages::InstallParu),
        Box::new(crate::tasks::packages::InstallAurPackages),
        Box::new(crate::tasks::files::symlinks::InstallSymlinks),
        Box::new(crate::tasks::files::chmod::ApplyFilePermissions),
        Box::new(crate::tasks::shell::login_shell::ConfigureShell),
        Box::new(crate::tasks::system::systemd_units::ConfigureSystemd),
        Box::new(crate::tasks::system::registry::ApplyRegistry),
        Box::new(crate::tasks::editors::InstallVsCodeExtensions),
        Box::new(crate::tasks::ai::apm::InstallApmPackages),
        Box::new(crate::tasks::ai::apm::UpdateApmPackages),
        Box::new(crate::tasks::system::pam::ConfigurePam),
        Box::new(crate::tasks::system::wsl_conf::InstallWslConf),
        Box::new(crate::tasks::overlay::LoadOverlayScripts),
        Box::new(crate::tasks::core::wrapper::InstallWrapper),
        Box::new(crate::tasks::core::path::ConfigurePath),
        Box::new(crate::tasks::repository::reload_config::ReloadConfig::new(
            repo_updated,
        )),
    ]
}

#[cfg(test)]
mod tests {
    use super::{all_install_tasks, all_uninstall_tasks};
    use crate::tasks::Domain;
    use crate::tasks::core::self_update::UpdateBinary;
    use crate::tasks::validation::{
        RunPSScriptAnalyzer, RunShellcheck, ValidateConfigFiles, ValidateConfigWarnings,
        ValidateManifestSync, ValidateSymlinkSources,
    };

    /// Every production task must declare an explicit [`Domain`]; only test and
    /// mock tasks are allowed to keep the [`Domain::General`] default.  This
    /// guards against new tasks being registered without a domain, which would
    /// silently drop them into the catch-all summary group.
    #[test]
    fn all_registered_tasks_declare_a_domain() {
        let mut tasks = all_install_tasks();
        tasks.extend(all_uninstall_tasks());
        // Tasks run outside the catalog (inline self-update, the test command).
        tasks.push(Box::new(UpdateBinary));
        tasks.push(Box::new(ValidateConfigWarnings));
        tasks.push(Box::new(ValidateSymlinkSources));
        tasks.push(Box::new(ValidateConfigFiles));
        tasks.push(Box::new(ValidateManifestSync));
        tasks.push(Box::new(RunShellcheck));
        tasks.push(Box::new(RunPSScriptAnalyzer));

        for task in &tasks {
            assert_ne!(
                task.domain(),
                Domain::General,
                "task `{}` is missing an explicit domain",
                task.name()
            );
        }
    }
}
