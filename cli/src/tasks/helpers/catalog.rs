//! Task registration: the complete set of install and uninstall tasks.

use crate::engine::update_signal::UpdateSignal;
use crate::tasks::Task;

/// The complete set of tasks run by the uninstall command.
#[must_use]
pub fn all_uninstall_tasks() -> Vec<Box<dyn Task>> {
    vec![
        Box::new(crate::tasks::configure::symlinks::UninstallSymlinks),
        Box::new(crate::tasks::bootstrap::hooks::UninstallGitHooks::new()),
        Box::new(crate::tasks::bootstrap::wrapper::UninstallWrapper),
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
        Box::new(crate::tasks::bootstrap::self_update::UpdateBinary),
        Box::new(crate::tasks::bootstrap::developer_mode::EnableDeveloperMode),
        Box::new(crate::tasks::bootstrap::sparse_checkout::ConfigureSparseCheckout::new()),
        Box::new(crate::tasks::bootstrap::update::UpdateRepository::new(
            repo_updated.clone(),
        )),
        Box::new(crate::tasks::configure::git_config::ConfigureGit),
        Box::new(crate::tasks::bootstrap::hooks::InstallGitHooks::new()),
        Box::new(crate::tasks::configure::packages::InstallPackages),
        Box::new(crate::tasks::configure::packages::InstallParu),
        Box::new(crate::tasks::configure::packages::InstallAurPackages),
        Box::new(crate::tasks::configure::symlinks::InstallSymlinks),
        Box::new(crate::tasks::configure::chmod::ApplyFilePermissions),
        Box::new(crate::tasks::configure::shell::ConfigureShell),
        Box::new(crate::tasks::configure::systemd_units::ConfigureSystemd),
        Box::new(crate::tasks::configure::registry::ApplyRegistry),
        Box::new(crate::tasks::configure::vscode_extensions::InstallVsCodeExtensions),
        Box::new(crate::tasks::configure::copilot_plugins::InstallCopilotPlugins),
        Box::new(crate::tasks::configure::wsl_conf::InstallWslConf),
        Box::new(crate::tasks::bootstrap::wrapper::InstallWrapper),
        Box::new(crate::tasks::bootstrap::path::ConfigurePath),
        Box::new(crate::tasks::bootstrap::reload_config::ReloadConfig::new(
            repo_updated,
        )),
    ]
}
