//! Task registration: the complete set of install and uninstall tasks.

use crate::engine::update_signal::UpdateSignal;
use crate::tasks::Task;

/// The complete set of tasks run by the uninstall command.
#[must_use]
pub fn all_uninstall_tasks() -> Vec<Box<dyn Task>> {
    vec![
        Box::new(crate::tasks::user::symlinks::UninstallSymlinks),
        Box::new(crate::tasks::system::hooks::UninstallGitHooks::new()),
        Box::new(crate::tasks::system::wrapper::UninstallWrapper),
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
        Box::new(crate::tasks::system::self_update::UpdateBinary),
        Box::new(crate::tasks::system::developer_mode::EnableDeveloperMode),
        Box::new(crate::tasks::system::sparse_checkout::ConfigureSparseCheckout::new()),
        Box::new(crate::tasks::system::update::UpdateRepository::new(
            repo_updated.clone(),
        )),
        Box::new(crate::tasks::user::git_config::ConfigureGit),
        Box::new(crate::tasks::system::hooks::InstallGitHooks::new()),
        Box::new(crate::tasks::user::packages::InstallPackages),
        Box::new(crate::tasks::user::packages::InstallParu),
        Box::new(crate::tasks::user::packages::InstallAurPackages),
        Box::new(crate::tasks::user::symlinks::InstallSymlinks),
        Box::new(crate::tasks::user::chmod::ApplyFilePermissions),
        Box::new(crate::tasks::user::shell::ConfigureShell),
        Box::new(crate::tasks::user::systemd_units::ConfigureSystemd),
        Box::new(crate::tasks::user::registry::ApplyRegistry),
        Box::new(crate::tasks::user::vscode_extensions::InstallVsCodeExtensions),
        Box::new(crate::tasks::user::copilot_plugins::InstallCopilotPlugins),
        Box::new(crate::tasks::user::wsl_conf::InstallWslConf),
        Box::new(crate::tasks::system::wrapper::InstallWrapper),
        Box::new(crate::tasks::system::path::ConfigurePath),
        Box::new(crate::tasks::system::reload_config::ReloadConfig::new(
            repo_updated,
        )),
    ]
}
