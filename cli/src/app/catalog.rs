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
use crate::app::reload::ReloadConfig;
use crate::domains::ai::apm::{InstallApmPackages, UpdateApmPackages};
use crate::domains::ai::tasks::copilot_settings::ConfigureCopilot;
use crate::domains::dotfiles::tasks::path::ConfigurePath;
use crate::domains::dotfiles::tasks::wrapper::{InstallWrapper, UninstallWrapper};
use crate::domains::editors::tasks::InstallVsCodeExtensions;
use crate::domains::files::tasks::chmod::ApplyFilePermissions;
use crate::domains::files::tasks::symlinks::{InstallSymlinks, UninstallSymlinks};
use crate::domains::git::tasks::git_config::ConfigureGit;
use crate::domains::git::tasks::hooks::{InstallGitHooks, UninstallGitHooks};
use crate::domains::overlay::tasks::ReportOverlayScriptSnapshot;
use crate::domains::packages::tasks::{InstallAurPackages, InstallPackages, InstallParu};
use crate::domains::repository::tasks::sparse_checkout::ConfigureSparseCheckout;
use crate::domains::repository::tasks::update::UpdateRepository;
use crate::domains::shell::tasks::completions::GenerateCompletions;
use crate::domains::shell::tasks::login_shell::ConfigureShell;
use crate::domains::system::tasks::developer_mode::EnableDeveloperMode;
use crate::domains::system::tasks::pam::ConfigurePam;
use crate::domains::system::tasks::registry::ApplyRegistry;
use crate::domains::system::tasks::systemd_units::ConfigureSystemd;
use crate::domains::system::tasks::wsl_conf::InstallWslConf;
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
        Box::new(ConfigureSparseCheckout::new(store.manifest.clone())),
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
        with_deps(
            ConfigurePam::new(store.pam_services.clone()),
            &[id::<InstallPackages>(), id::<InstallAurPackages>()],
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
    use crate::app::validation::{
        RunPSScriptAnalyzer, RunShellcheck, ValidateConfigFiles, ValidateConfigWarnings,
        ValidateManifestSync, ValidateSymlinkSources,
    };
    use crate::engine::Domain;
    use crate::test_helpers::empty_config;
    use std::path::PathBuf;

    fn test_params() -> ConfigStore {
        ConfigStore::from_config(empty_config(PathBuf::from("/tmp")))
    }

    fn test_store() -> ConfigStore {
        ConfigStore::from_config(empty_config(PathBuf::from("/tmp")))
    }

    /// Every production task must declare an explicit [`Domain`]; only test and
    /// mock tasks are allowed to keep the [`Domain::General`] default.
    #[test]
    fn all_registered_tasks_declare_a_domain() {
        let store = test_store();
        let mut tasks = all_install_tasks(test_params());
        tasks.extend(all_uninstall_tasks(&store));
        // Tasks run outside the catalog (the test command).
        tasks.push(Box::new(ValidateConfigWarnings::new(
            store.aggregate.clone(),
        )));
        tasks.push(Box::new(ValidateSymlinkSources::new(
            store.aggregate.clone(),
        )));
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

    /// Guard against forgetting to register a new task.
    #[test]
    fn all_install_tasks_count() {
        let tasks = all_install_tasks(test_params());
        assert_eq!(
            tasks.len(),
            24,
            "expected 24 install tasks — did you add a new task without updating \
             all_install_tasks()? Update the registration list and this test."
        );
    }

    #[test]
    fn all_uninstall_tasks_count() {
        let store = test_store();
        let tasks = all_uninstall_tasks(&store);
        assert_eq!(
            tasks.len(),
            3,
            "expected 3 uninstall tasks — update all_uninstall_tasks() and this test."
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
            find("Report overlay script snapshot")
                .dependencies()
                .contains(&id::<ReloadConfig>()),
            "overlay script snapshot report must depend on reload (app-injected)"
        );
        assert!(
            find("Install Git hooks")
                .dependencies()
                .contains(&id::<UpdateRepository>()),
            "git hooks must depend on repository update (app-injected)"
        );
    }
}
