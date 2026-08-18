//! Application-owned configuration reload after a repository update.
//!
//! Reloading re-parses every TOML file and re-composes the aggregate
//! [`Config`](crate::app::config::Config), then swaps each reloadable per-domain
//! handle in the shared [`ConfigStore`]. Because composing the aggregate
//! configuration is an application concern (it spans every domain), this task
//! lives in the `app` layer rather than in any single domain.

use anyhow::{Context as _, Result};

use crate::app::config::store::ConfigStore;
use crate::app::config::{Config, profiles};
use crate::engine::{
    Context, Operation, OperationState, Task, TaskResult, TaskStats, UpdateSignal,
    process_operation, task_metadata,
};
use crate::infra::logging::OutputExt as _;

/// Re-parse all configuration files after `UpdateRepository` has pulled the
/// latest changes and swap the freshly-loaded values into the shared store.
///
/// Tasks read configuration through handles owned by the [`ConfigStore`];
/// swapping the reloadable handles here makes new values visible downstream.
/// Dynamic overlay script tasks are rebuilt from the refreshed script handle
/// after this task's dependency closure completes.
#[derive(Debug)]
pub struct ReloadConfig {
    /// Shared flag set by the repository-update task when new commits were
    /// fetched.  When `false`, the reload is a no-op.
    repo_updated: UpdateSignal,
    /// Shared configuration store whose handles are swapped on reload.
    store: ConfigStore,
}

impl ReloadConfig {
    /// Create a new task, sharing `repo_updated` with the repository-update
    /// task and the [`ConfigStore`] with every configuration-reading task.
    #[must_use]
    pub const fn new(repo_updated: UpdateSignal, store: ConfigStore) -> Self {
        Self {
            repo_updated,
            store,
        }
    }

    fn reload(&self, ctx: &Context) -> Result<TaskResult> {
        // The selected profile name is fixed for this process, but its category
        // definition may have changed in the fetched profiles.toml.
        let new_config = {
            let old = self.store.aggregate.read();
            let profile =
                profiles::resolve(&old.profile.name, &old.root.join("conf"), ctx.platform())
                    .context("re-resolving profile after repository update")?;
            Config::load(&old.root, &profile, ctx.platform(), old.overlay.as_deref())
                .context("reloading configuration after repository update")?
        };

        ctx.debug_fmt(|| {
            format!(
                "{} packages, {} symlinks after reload",
                new_config.packages.len(),
                new_config.symlinks.len()
            )
        });

        // Validate before publishing, mirroring startup: `load_config` in the
        // command runner reports diagnostics for the configuration present when
        // the process started. Without this, configuration pulled in by
        // `UpdateRepository` would reach downstream tasks unreported, so
        // whether a problem is surfaced would depend on whether the offending
        // commit landed before or during the run.
        let diagnostics = new_config.validate(ctx.platform());

        self.store.reload(new_config);

        // Diagnostics are advisory here for the same reason they are at
        // startup: the run continues and the user is told what is wrong.
        // Escalating to a hard failure only on the reload path would make the
        // outcome depend on run timing rather than on configuration content.
        crate::app::validation::display_diagnostics(&diagnostics, ctx.log());

        if diagnostics.is_empty() {
            ctx.log().info("configuration reloaded");
        } else {
            ctx.log().info(format!(
                "configuration reloaded with {} diagnostic(s)",
                diagnostics.len()
            ));
        }
        Ok(TaskResult::Ok)
    }
}

struct ReloadConfigOperation<'a> {
    task: &'a ReloadConfig,
}

impl Operation for ReloadConfigOperation<'_> {
    type Plan = ();

    fn current_state(&self, _ctx: &Context) -> Result<OperationState<Self::Plan>> {
        if self.task.repo_updated.was_updated() {
            Ok(OperationState::needs_run("repository changed", ()))
        } else {
            Ok(OperationState::not_applicable(
                "repository was already current",
            ))
        }
    }

    fn preview(&self, _ctx: &Context, _plan: &Self::Plan) -> Result<TaskResult> {
        Ok(TaskStats::changed().finish())
    }

    fn apply(&self, ctx: &Context, _plan: &Self::Plan) -> Result<TaskResult> {
        self.task.reload(ctx)
    }
}

impl Task for ReloadConfig {
    task_metadata! {
        name: "Reload configuration",
        visibility: crate::engine::TaskVisibility::Internal,
        deps: [crate::domains::repository::update::UpdateRepository],
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        process_operation(ctx, &ReloadConfigOperation { task: self })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::config::profiles::Profile;
    use crate::engine::UpdateSignal;
    use crate::infra::config::category_matcher::Category;
    use crate::infra::logging::MsgKind;
    use crate::test_helpers::{empty_config, make_linux_context, recording_log};
    use std::path::{Path, PathBuf};

    /// Config files `Config::load` expects to find under `conf/`.
    const CONF_FILES: &[&str] = &[
        "symlinks.toml",
        "packages.toml",
        "manifest.toml",
        "chmod.toml",
        "systemd-units.toml",
        "vscode-extensions.toml",
        "git-config.toml",
        "registry.toml",
        "agent-settings.toml",
    ];

    /// Create a repository root with an empty but complete `conf/` directory.
    fn make_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let conf = dir.path().join("conf");
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::write(
            conf.join("profiles.toml"),
            "[base]\ninclude = []\nexclude = [\"desktop\"]\n",
        )
        .unwrap();
        for file in CONF_FILES {
            std::fs::write(conf.join(file), "").unwrap();
        }
        dir
    }

    fn write_conf(root: &Path, file: &str, content: &str) {
        std::fs::write(root.join("conf").join(file), content).unwrap();
    }

    /// A store whose aggregate carries the `base` profile so a reload
    /// re-loads with matching category selection.
    fn base_store(root: &Path) -> ConfigStore {
        let mut config = empty_config(root.to_path_buf());
        config.profile = Profile {
            name: "base".to_string(),
            active_categories: vec![Category::Base],
            excluded_categories: vec![],
        };
        ConfigStore::from_config(config)
    }

    fn updated_task(store: ConfigStore) -> ReloadConfig {
        let repo_updated = UpdateSignal::new();
        repo_updated.mark_updated();
        ReloadConfig::new(repo_updated, store)
    }

    fn make_task(root: PathBuf, signal: UpdateSignal) -> ReloadConfig {
        let store = ConfigStore::from_config(empty_config(root));
        ReloadConfig::new(signal, store)
    }

    #[test]
    fn run_is_not_applicable_when_repo_not_updated() {
        let ctx = make_linux_context(empty_config(PathBuf::from("/tmp")));
        let task = make_task(PathBuf::from("/tmp"), UpdateSignal::new());
        assert!(task.assess(&ctx).is_applicable());
        assert!(matches!(
            task.run(&ctx).unwrap(),
            TaskResult::NotApplicable(reason) if reason == "repository was already current"
        ));
    }

    #[test]
    fn assessment_does_not_freeze_repository_update_signal() {
        let ctx = make_linux_context(empty_config(PathBuf::from("/tmp")));
        let signal = UpdateSignal::new();
        let task = make_task(PathBuf::from("/tmp"), signal.clone());
        let assessment = task.assess(&ctx);
        signal.mark_updated();
        assert!(assessment.is_applicable());
        assert!(task.repo_updated.was_updated());
    }

    #[test]
    fn run_reloads_config_when_repo_updated() {
        let dir = make_repo();
        let ctx = make_linux_context(empty_config(dir.path().to_path_buf()));
        let task = updated_task(base_store(dir.path()));
        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Ok));
    }

    #[test]
    fn run_publishes_new_values_into_the_store() {
        let dir = make_repo();
        write_conf(
            dir.path(),
            "packages.toml",
            "[base]\npackages = [\"git\"]\n",
        );

        let ctx = make_linux_context(empty_config(dir.path().to_path_buf()));
        let store = base_store(dir.path());
        assert!(
            store.packages.read().is_empty(),
            "store should start with no packages"
        );

        let result = updated_task(store.clone()).run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Ok));

        let packages = store.packages.read();
        assert_eq!(
            packages.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            vec!["git"],
            "reload should publish the newly-loaded packages to the shared store"
        );
    }

    #[test]
    fn run_re_resolves_the_selected_profile_from_updated_definitions() {
        let dir = make_repo();
        write_conf(
            dir.path(),
            "profiles.toml",
            "[base]\ninclude = [\"desktop\"]\nexclude = []\n",
        );
        write_conf(
            dir.path(),
            "packages.toml",
            "[desktop]\npackages = [\"desktop-package\"]\n",
        );

        let ctx = make_linux_context(empty_config(dir.path().to_path_buf()));
        let store = base_store(dir.path());
        assert!(
            !store
                .aggregate
                .read()
                .profile
                .active_categories
                .contains(&Category::Desktop)
        );

        let result = updated_task(store.clone()).run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Ok));

        assert!(
            store
                .aggregate
                .read()
                .profile
                .active_categories
                .contains(&Category::Desktop),
            "reload should use the fetched profile definition, not stale resolved categories"
        );
        assert_eq!(store.packages.read()[0].name, "desktop-package");
    }

    #[test]
    fn run_reports_diagnostics_for_reloaded_config() {
        // Regression: reload called `Config::load` but never `Config::validate`,
        // so configuration pulled in by `UpdateRepository` reached downstream
        // tasks with no diagnostics reported. Whether a problem was surfaced
        // depended on whether the offending commit landed before the run
        // started rather than on the configuration content.
        let dir = make_repo();
        write_conf(
            dir.path(),
            "symlinks.toml",
            "[base]\nsymlinks = [\"definitely-not-present\"]\n",
        );

        let (log, handle) = recording_log();
        let ctx = make_linux_context(empty_config(dir.path().to_path_buf())).with_log(handle);

        let result = updated_task(base_store(dir.path())).run(&ctx).unwrap();

        assert!(
            matches!(result, TaskResult::Ok),
            "diagnostics are advisory at startup, so reload must not fail either"
        );
        assert!(
            log.has_message(MsgKind::Warn, "symlink.source-missing"),
            "reload should surface the diagnostic; emitted: {}",
            log.all_text()
        );
        assert!(
            log.has_message(MsgKind::Info, "1 diagnostic(s)"),
            "reload should report the diagnostic count; emitted: {}",
            log.all_text()
        );
    }

    #[test]
    fn run_reports_no_diagnostics_for_clean_config() {
        let dir = make_repo();
        let (log, handle) = recording_log();
        let ctx = make_linux_context(empty_config(dir.path().to_path_buf())).with_log(handle);

        let result = updated_task(base_store(dir.path())).run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Ok));

        assert!(
            log.messages_of(MsgKind::Warn).is_empty(),
            "clean config should not warn; emitted: {}",
            log.all_text()
        );
        assert!(
            log.has_message(MsgKind::Info, "configuration reloaded"),
            "reload should confirm success; emitted: {}",
            log.all_text()
        );
    }

    #[test]
    fn run_fails_when_reloaded_config_cannot_be_parsed() {
        let dir = make_repo();
        write_conf(
            dir.path(),
            "packages.toml",
            "[base]\npackages = [{ name = \"git\", our = true }]\n",
        );

        let ctx = make_linux_context(empty_config(dir.path().to_path_buf()));
        let error = updated_task(base_store(dir.path()))
            .run(&ctx)
            .expect_err("an unparseable reload must fail rather than keep stale config");

        let message = format!("{error:#}");
        assert!(
            message.contains("reloading configuration"),
            "error should be attributed to the reload, got: {message}"
        );
        assert!(
            message.contains("our"),
            "error should name the unknown key, got: {message}"
        );
    }

    #[test]
    fn dry_run_does_not_publish_new_values() {
        let dir = make_repo();
        write_conf(
            dir.path(),
            "packages.toml",
            "[base]\npackages = [\"git\"]\n",
        );

        let ctx = make_linux_context(empty_config(dir.path().to_path_buf())).with_dry_run(true);
        let store = base_store(dir.path());

        let result = updated_task(store.clone()).run(&ctx).unwrap();

        assert!(!matches!(result, TaskResult::NotApplicable(_)));
        assert!(
            store.packages.read().is_empty(),
            "preview must not mutate the shared store"
        );
    }
}
