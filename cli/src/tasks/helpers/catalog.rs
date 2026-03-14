//! Task registration: the complete set of install and uninstall tasks.

use crate::engine::update_signal::UpdateSignal;
use crate::tasks::Task;

/// The complete set of tasks run by the uninstall command.
#[must_use]
pub fn all_uninstall_tasks() -> Vec<Box<dyn Task>> {
    vec![
        Box::new(crate::tasks::symlinks::UninstallSymlinks),
        Box::new(crate::tasks::hooks::UninstallGitHooks::new()),
        Box::new(crate::tasks::wrapper::UninstallWrapper),
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
        Box::new(crate::tasks::self_update::UpdateBinary),
        Box::new(crate::tasks::developer_mode::EnableDeveloperMode),
        Box::new(crate::tasks::sparse_checkout::ConfigureSparseCheckout::new()),
        Box::new(crate::tasks::update::UpdateRepository::new(
            repo_updated.clone(),
        )),
        Box::new(crate::tasks::git_config::ConfigureGit),
        Box::new(crate::tasks::hooks::InstallGitHooks::new()),
        Box::new(crate::tasks::packages::InstallPackages),
        Box::new(crate::tasks::packages::InstallParu),
        Box::new(crate::tasks::packages::InstallAurPackages),
        Box::new(crate::tasks::symlinks::InstallSymlinks),
        Box::new(crate::tasks::chmod::ApplyFilePermissions),
        Box::new(crate::tasks::shell::ConfigureShell),
        Box::new(crate::tasks::systemd_units::ConfigureSystemd),
        Box::new(crate::tasks::registry::ApplyRegistry),
        Box::new(crate::tasks::vscode_extensions::InstallVsCodeExtensions),
        Box::new(crate::tasks::copilot_plugins::InstallCopilotPlugins),
        Box::new(crate::tasks::wsl_conf::InstallWslConf),
        Box::new(crate::tasks::wrapper::InstallWrapper),
        Box::new(crate::tasks::path::ConfigurePath),
        Box::new(crate::tasks::reload_config::ReloadConfig::new(repo_updated)),
    ]
}
