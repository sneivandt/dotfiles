//! Task registration: the complete set of install and uninstall phases.

use crate::engine::update_signal::UpdateSignal;
use crate::phases::Task;

/// The complete set of tasks run by the uninstall command.
#[must_use]
pub fn all_uninstall_tasks() -> Vec<Box<dyn Task>> {
    vec![
        Box::new(crate::phases::apply::symlinks::UninstallSymlinks),
        Box::new(crate::phases::repository::hooks::UninstallGitHooks::new()),
        Box::new(crate::phases::bootstrap::wrapper::UninstallWrapper),
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
        Box::new(crate::phases::bootstrap::developer_mode::EnableDeveloperMode),
        Box::new(crate::phases::repository::sparse_checkout::ConfigureSparseCheckout::new()),
        Box::new(crate::phases::repository::update::UpdateRepository::new(
            repo_updated.clone(),
        )),
        Box::new(crate::phases::apply::git_config::ConfigureGit),
        Box::new(crate::phases::repository::hooks::InstallGitHooks::new()),
        Box::new(crate::phases::repository::completions::GenerateCompletions),
        Box::new(crate::phases::apply::packages::InstallPackages),
        Box::new(crate::phases::apply::packages::InstallParu),
        Box::new(crate::phases::apply::packages::InstallAurPackages),
        Box::new(crate::phases::apply::symlinks::InstallSymlinks),
        Box::new(crate::phases::apply::chmod::ApplyFilePermissions),
        Box::new(crate::phases::apply::shell::ConfigureShell),
        Box::new(crate::phases::apply::systemd_units::ConfigureSystemd),
        Box::new(crate::phases::apply::registry::ApplyRegistry),
        Box::new(crate::phases::apply::vscode_extensions::InstallVsCodeExtensions),
        Box::new(crate::phases::apply::copilot_plugins::InstallCopilotPlugins),
        Box::new(crate::phases::apply::wsl_conf::InstallWslConf),
        Box::new(crate::phases::apply::overlay_scripts::LoadOverlayScripts),
        Box::new(crate::phases::bootstrap::wrapper::InstallWrapper),
        Box::new(crate::phases::bootstrap::path::ConfigurePath),
        Box::new(crate::phases::repository::reload_config::ReloadConfig::new(
            repo_updated,
        )),
    ]
}
