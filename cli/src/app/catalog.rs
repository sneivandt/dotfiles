//! Task registration: the complete set of install and uninstall tasks.
//!
//! The application layer is the only layer allowed to name tasks across
//! domains, so all cross-domain wiring lives here: each domain task is
//! constructed with a handle to its configuration slice (from the shared
//! [`ConfigStore`]), and cross-domain ordering constraints are applied by
//! wrapping tasks in [`TaskWithExtraDeps`].

use std::any::TypeId;

use clap::CommandFactory as _;

use crate::app::cli::Cli;
use crate::app::config::store::ConfigStore;
use crate::app::preserve::MaterializeExcludedSymlinks;
use crate::app::reload::ReloadConfig;
use crate::domains::ai::apm::{InstallApmPackages, UpdateApmPackages};
use crate::domains::ai::copilot_settings::ConfigureCopilot;
use crate::domains::dotfiles::path::ConfigurePath;
use crate::domains::dotfiles::wrapper::{InstallWrapper, UninstallWrapper};
use crate::domains::editors::vscode_extensions::InstallVsCodeExtensions;
use crate::domains::files::chmod::ApplyFilePermissions;
use crate::domains::files::symlinks::{InstallSymlinks, UninstallSymlinks};
use crate::domains::git::git_config::ConfigureGit;
use crate::domains::git::hooks::{InstallGitHooks, UninstallGitHooks};
use crate::domains::overlay::scripts::ReportOverlayScriptSnapshot;
use crate::domains::packages::install::{InstallAurPackages, InstallPackages, InstallParu};
use crate::domains::repository::sparse_checkout::ConfigureSparseCheckout;
use crate::domains::repository::update::UpdateRepository;
use crate::domains::shell::completions::GenerateCompletions;
use crate::domains::shell::login_shell::ConfigureShell;
use crate::domains::system::developer_mode::EnableDeveloperMode;
use crate::domains::system::registry::ApplyRegistry;
use crate::domains::system::systemd_units::ConfigureSystemd;
use crate::domains::system::wsl_conf::InstallWslConf;
use crate::engine::update_signal::UpdateSignal;
use crate::engine::{Task, TaskId, TaskWithExtraDeps};

/// The `TaskId` of a static task type.
const fn id<T: 'static>() -> TaskId {
    TaskId::Type(TypeId::of::<T>())
}

/// Wrap a task, adding cross-domain dependency edges declared by the app.
fn with_deps(inner: impl Task, extra: &[TaskId]) -> Box<dyn Task> {
    TaskWithExtraDeps::boxed(Box::new(inner), extra)
}

/// Generate the zsh completion script for the CLI.
///
/// Owned by the app because it depends on the CLI argument definition; the
/// resulting content is injected into the shell-completions task so that domain
/// stays free of any CLI dependency.
#[must_use]
pub fn generate_zsh_completions() -> String {
    let mut buf = Vec::new();
    let mut cmd = Cli::command();
    clap_complete::generate(clap_complete::Shell::Zsh, &mut cmd, "dotfiles", &mut buf);
    String::from_utf8(buf).unwrap_or_default()
}

/// The complete set of tasks run by the uninstall command.
#[must_use]
pub fn all_uninstall_tasks(store: &ConfigStore) -> Vec<Box<dyn Task>> {
    vec![
        Box::new(UninstallSymlinks::new(store.symlinks.clone())),
        Box::new(UninstallGitHooks::new()),
        Box::new(UninstallWrapper),
    ]
}

/// The complete set of tasks run by the install command.
///
/// Order within the list is arbitrary — the scheduler derives execution order
/// from each task's [`Task::dependencies`] declaration merged with the
/// app-level dependency edges applied here.
#[must_use]
pub fn all_install_tasks(store: ConfigStore) -> Vec<Box<dyn Task>> {
    let repo_updated = UpdateSignal::new();
    let completions = generate_zsh_completions();

    vec![
        Box::new(EnableDeveloperMode),
        Box::new(MaterializeExcludedSymlinks::new(
            store.all_symlinks.clone(),
            store.manifest.clone(),
        )),
        with_deps(
            ConfigureSparseCheckout::new(store.manifest.clone()),
            &[id::<MaterializeExcludedSymlinks>()],
        ),
        Box::new(UpdateRepository::new(repo_updated.clone())),
        Box::new(ConfigureGit::new(store.git_settings.clone())),
        Box::new(ConfigureCopilot::new(store.copilot_settings.clone())),
        with_deps(InstallGitHooks::new(), &[id::<UpdateRepository>()]),
        with_deps(
            GenerateCompletions::new(completions),
            &[id::<UpdateRepository>()],
        ),
        Box::new(InstallPackages::new(store.packages.clone())),
        Box::new(InstallParu),
        Box::new(InstallAurPackages::new(store.packages.clone())),
        Box::new(InstallSymlinks::new(store.symlinks.clone())),
        Box::new(ApplyFilePermissions::new(store.chmod.clone())),
        with_deps(ConfigureShell, &[id::<InstallPackages>()]),
        with_deps(
            ConfigureSystemd::new(store.units.clone()),
            &[
                id::<InstallPackages>(),
                id::<InstallAurPackages>(),
                id::<InstallSymlinks>(),
            ],
        ),
        Box::new(ApplyRegistry::new(store.registry.clone())),
        with_deps(
            InstallVsCodeExtensions::new(store.vscode_extensions.clone()),
            &[id::<InstallPackages>(), id::<InstallAurPackages>()],
        ),
        with_deps(
            InstallApmPackages,
            &[
                id::<InstallPackages>(),
                id::<InstallAurPackages>(),
                id::<InstallSymlinks>(),
            ],
        ),
        Box::new(UpdateApmPackages),
        Box::new(InstallWslConf),
        with_deps(
            ReportOverlayScriptSnapshot::new(store.scripts.clone()),
            &[id::<ReloadConfig>()],
        ),
        Box::new(InstallWrapper),
        Box::new(ConfigurePath),
        Box::new(ReloadConfig::new(repo_updated, store)),
    ]
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::test_helpers::empty_config;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn test_params() -> ConfigStore {
        ConfigStore::from_config(empty_config(PathBuf::from("/tmp")))
    }

    fn test_store() -> ConfigStore {
        ConfigStore::from_config(empty_config(PathBuf::from("/tmp")))
    }

    #[test]
    fn all_install_tasks_have_the_expected_membership() {
        let tasks = all_install_tasks(test_params());
        let actual = tasks
            .iter()
            .map(|task| task.task_id())
            .collect::<HashSet<_>>();
        let expected = [
            id::<EnableDeveloperMode>(),
            id::<MaterializeExcludedSymlinks>(),
            id::<ConfigureSparseCheckout>(),
            id::<UpdateRepository>(),
            id::<ConfigureGit>(),
            id::<ConfigureCopilot>(),
            id::<InstallGitHooks>(),
            id::<GenerateCompletions>(),
            id::<InstallPackages>(),
            id::<InstallParu>(),
            id::<InstallAurPackages>(),
            id::<InstallSymlinks>(),
            id::<ApplyFilePermissions>(),
            id::<ConfigureShell>(),
            id::<ConfigureSystemd>(),
            id::<ApplyRegistry>(),
            id::<InstallVsCodeExtensions>(),
            id::<InstallApmPackages>(),
            id::<UpdateApmPackages>(),
            id::<InstallWslConf>(),
            id::<ReportOverlayScriptSnapshot>(),
            id::<InstallWrapper>(),
            id::<ConfigurePath>(),
            id::<ReloadConfig>(),
        ]
        .into_iter()
        .collect::<HashSet<_>>();

        assert_eq!(actual, expected, "install task registration changed");
        assert_eq!(
            tasks.len(),
            actual.len(),
            "install task registration contains duplicate task IDs"
        );
    }

    #[test]
    fn all_uninstall_tasks_have_the_expected_membership() {
        let store = test_store();
        let tasks = all_uninstall_tasks(&store);
        let actual = tasks
            .iter()
            .map(|task| task.task_id())
            .collect::<HashSet<_>>();
        let expected = [
            id::<UninstallSymlinks>(),
            id::<UninstallGitHooks>(),
            id::<UninstallWrapper>(),
        ]
        .into_iter()
        .collect::<HashSet<_>>();

        assert_eq!(actual, expected, "uninstall task registration changed");
        assert_eq!(
            tasks.len(),
            actual.len(),
            "uninstall task registration contains duplicate task IDs"
        );
    }

    #[test]
    fn install_tasks_have_resolvable_dependencies() {
        use std::collections::HashSet;
        let tasks = all_install_tasks(test_params());
        let ids: Vec<TaskId> = tasks.iter().map(|t| t.task_id()).collect();
        let unique: HashSet<TaskId> = ids.iter().copied().collect();
        assert_eq!(ids.len(), unique.len(), "duplicate task TaskIds found");
        let present: HashSet<TaskId> = tasks.iter().map(|t| t.task_id()).collect();
        for task in &tasks {
            for dep in task.dependencies() {
                assert!(
                    present.contains(dep),
                    "task '{}' depends on a TaskId not in the task list",
                    task.name()
                );
            }
        }
    }

    #[test]
    fn install_tasks_have_no_cycles() {
        let tasks = all_install_tasks(test_params());
        let task_refs: Vec<&dyn Task> = tasks.iter().map(Box::as_ref).collect();
        assert!(
            crate::engine::graph::ResolvedTaskGraph::resolve(&task_refs).is_ok(),
            "install task graph should be a valid DAG"
        );
    }

    #[test]
    fn cross_domain_dependencies_are_applied() {
        let tasks = all_install_tasks(test_params());
        let find = |name: &str| {
            tasks
                .iter()
                .find(|t| t.name() == name)
                .expect("task present")
        };
        assert!(
            find("Configure sparse checkout")
                .dependencies()
                .contains(&id::<MaterializeExcludedSymlinks>()),
            "sparse checkout must preserve excluded managed symlinks first"
        );
        assert!(
            find("Configure systemd units")
                .dependencies()
                .contains(&id::<InstallSymlinks>()),
            "systemd must depend on symlinks (app-injected)"
        );
        assert!(
            find("Install shell completions")
                .dependencies()
                .contains(&id::<UpdateRepository>()),
            "completions must depend on repository update (app-injected)"
        );
        assert!(
            find("Report overlay scripts")
                .dependencies()
                .contains(&id::<ReloadConfig>()),
            "overlay script report must depend on reload (app-injected)"
        );
        assert!(
            find("Install Git hooks")
                .dependencies()
                .contains(&id::<UpdateRepository>()),
            "git hooks must depend on repository update (app-injected)"
        );
    }
}
