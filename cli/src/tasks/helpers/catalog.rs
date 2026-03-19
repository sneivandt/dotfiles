//! Task registration: the complete set of install and uninstall tasks.

use crate::engine::update_signal::UpdateSignal;
use crate::tasks::Task;

/// The complete set of tasks run by the uninstall command.
#[must_use]
pub fn all_uninstall_tasks() -> Vec<Box<dyn Task>> {
    vec![
        Box::new(crate::tasks::apply::symlinks::UninstallSymlinks),
        Box::new(crate::tasks::repository::hooks::UninstallGitHooks::new()),
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
        Box::new(crate::tasks::bootstrap::developer_mode::EnableDeveloperMode),
        Box::new(crate::tasks::repository::sparse_checkout::ConfigureSparseCheckout::new()),
        Box::new(crate::tasks::repository::update::UpdateRepository::new(
            repo_updated.clone(),
        )),
        Box::new(crate::tasks::apply::git_config::ConfigureGit),
        Box::new(crate::tasks::repository::hooks::InstallGitHooks::new()),
        Box::new(crate::tasks::repository::completions::GenerateCompletions),
        Box::new(crate::tasks::apply::packages::InstallPackages),
        Box::new(crate::tasks::apply::packages::InstallParu),
        Box::new(crate::tasks::apply::packages::InstallAurPackages),
        Box::new(crate::tasks::apply::symlinks::InstallSymlinks),
        Box::new(crate::tasks::apply::chmod::ApplyFilePermissions),
        Box::new(crate::tasks::apply::shell::ConfigureShell),
        Box::new(crate::tasks::apply::systemd_units::ConfigureSystemd),
        Box::new(crate::tasks::apply::registry::ApplyRegistry),
        Box::new(crate::tasks::apply::vscode_extensions::InstallVsCodeExtensions),
        Box::new(crate::tasks::apply::copilot_plugins::InstallCopilotPlugins),
        Box::new(crate::tasks::apply::wsl_conf::InstallWslConf),
        Box::new(crate::tasks::apply::overlay_scripts::LoadOverlayScripts),
        Box::new(crate::tasks::bootstrap::wrapper::InstallWrapper),
        Box::new(crate::tasks::bootstrap::path::ConfigurePath),
        Box::new(crate::tasks::repository::reload_config::ReloadConfig::new(
            repo_updated,
        )),
    ]
}
