//! Validation tasks for the `check` command.
//!
//! These tasks verify desired-state configuration integrity, declared sources,
//! and local APM plugin packages. They live in the app-owned validation module
//! and use the same [`Task`] contract as domain tasks.
use anyhow::{Context as _, Result};

use crate::app::config::Config;
use crate::engine::{Context, Task, TaskResult, task_metadata};
use crate::infra::ConfigHandle;
use crate::infra::exec::{CommandSpec, ExecResult};
use crate::infra::logging::{Log, OutputExt as _};

use super::discovery::discover_apm_plugin_dirs;

fn log_exec_output(log: &dyn Log, result: &ExecResult) {
    for line in result.stdout.lines().chain(result.stderr.lines()) {
        log.error(line);
    }
}

/// Fail the check command when config validation emits warnings.
#[derive(Debug)]
pub(crate) struct ValidateConfigWarnings {
    config: ConfigHandle<Config>,
}

impl ValidateConfigWarnings {
    /// Create the task with a handle to the aggregate configuration.
    #[must_use]
    pub(crate) const fn new(config: ConfigHandle<Config>) -> Self {
        Self { config }
    }
}

impl Task for ValidateConfigWarnings {
    task_metadata! {
        name: "Configuration diagnostics",
        selector: "config-diagnostics",
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let config = self.config.get();
        let mut diagnostics = config.validate(ctx.platform());
        diagnostics.extend(
            crate::domains::files::config::symlinks::config_home_diagnostic(
                &config.symlinks,
                ctx.home(),
                ctx.env().as_ref(),
            ),
        );
        if diagnostics.is_empty() {
            ctx.log().debug("no configuration issues found");
            return Ok(TaskResult::CheckPassed);
        }

        for d in &diagnostics {
            ctx.log().error(format!(
                "[{}] {} [{}] ({}): {}",
                d.severity.label(),
                d.source,
                d.item,
                d.code,
                d.message
            ));
        }

        let count = diagnostics.len();
        let unit = if count == 1 { "issue" } else { "issues" };
        anyhow::bail!("configuration check failed: {count} {unit} found");
    }
}

/// Validate that all symlink and file-permission source paths exist on disk.
#[derive(Debug)]
pub(crate) struct ValidateSymlinkSources {
    config: ConfigHandle<Config>,
}

impl ValidateSymlinkSources {
    /// Create the task with a handle to the aggregate configuration.
    #[must_use]
    pub(crate) const fn new(config: ConfigHandle<Config>) -> Self {
        Self { config }
    }
}

impl Task for ValidateSymlinkSources {
    task_metadata! {
        name: "Symlink sources",
        selector: "symlink-sources",
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        let config = self.config.get();
        !config.validation_symlinks.is_empty() || !config.validation_chmod.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let config = self.config.get();
        let symlinks = &config.validation_symlinks;
        let chmod = &config.validation_chmod;
        let repo_root = &config.root;

        let mut missing = 0u32;

        let expanded =
            crate::domains::files::config::symlinks::expand_glob_patterns(symlinks, repo_root)?;
        for symlink in &expanded {
            use crate::domains::files::config::symlinks::{InspectedSource, inspect_source};
            let symlinks_dir =
                crate::domains::files::config::symlinks::resolve_symlinks_dir(symlink, repo_root);
            let source = symlinks_dir.join(&symlink.source);
            match inspect_source(&source, Some(&symlinks_dir))? {
                InspectedSource::Available(_) => {}
                InspectedSource::Missing(_) => {
                    ctx.log()
                        .error(format!("symlink source missing: {}", source.display()));
                    missing = missing.saturating_add(1);
                }
                InspectedSource::Outside(reason) => anyhow::bail!(reason),
            }
        }

        let main_sources = repo_root.join("symlinks");
        let overlay_sources = config.overlay.as_ref().map(|root| root.join("symlinks"));
        for entry in chmod {
            let target = crate::domains::files::config::chmod::target_path(&entry.path);
            let sources: Vec<_> = expanded
                .iter()
                .filter_map(|link| {
                    let link_target = link.target.clone().unwrap_or_else(|| {
                        crate::domains::files::config::symlinks::default_target(&link.source)
                    });
                    let suffix = target.strip_prefix(&link_target).ok()?;
                    let root = crate::domains::files::config::symlinks::resolve_symlinks_dir(
                        link, repo_root,
                    );
                    let source = root.join(&link.source);
                    Some(if suffix.as_os_str().is_empty() {
                        source
                    } else {
                        source.join(suffix)
                    })
                })
                .collect();
            if !sources.is_empty() {
                for source in sources {
                    if !source.try_exists()? {
                        ctx.log().error(format!(
                            "file-permission source missing: {}",
                            source.display()
                        ));
                        missing = missing.saturating_add(1);
                    }
                }
                continue;
            }
            let spelling = target.to_string_lossy();
            let relative = spelling.strip_prefix('.').unwrap_or(&spelling);
            let main_source = main_sources.join(relative);
            let overlay_source_exists = overlay_sources
                .as_ref()
                .map(|root| root.join(relative).try_exists())
                .transpose()?
                .unwrap_or(false);
            if !main_source.try_exists()?
                && !overlay_source_exists
                && !ctx.home().join(&target).try_exists()?
            {
                ctx.log().error(format!(
                    "file-permission source missing: {}",
                    main_source.display()
                ));
                missing = missing.saturating_add(1);
            }
        }

        if missing > 0 {
            anyhow::bail!("{missing} configured source(s) missing");
        }
        ctx.log().debug(format!(
            "all {} configured sources exist",
            symlinks.len().saturating_add(chmod.len())
        ));
        Ok(TaskResult::CheckPassed)
    }
}

/// Validate that required configuration files exist.
#[derive(Debug)]
pub(crate) struct ValidateConfigFiles;

impl Task for ValidateConfigFiles {
    task_metadata! {
        name: "Required configuration files",
        selector: "config-files",
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let root = ctx.root();
        let conf = root.join("conf");
        let required = crate::app::config::REQUIRED_CONFIG_FILES;

        let mut errors = 0u32;
        for config_file in required {
            let path = conf.join(config_file);
            if path.exists() {
                ctx.debug_fmt(|| format!("found conf/{config_file}"));
            } else {
                ctx.log()
                    .error(format!("missing config: conf/{config_file}"));
                errors = errors.saturating_add(1);
            }
        }

        let hooks_dir = root.join("hooks");
        if hooks_dir.exists() {
            ctx.log().debug("found hooks directory");
        } else {
            ctx.log().warn("hooks directory missing");
        }

        if errors > 0 {
            anyhow::bail!("{errors} required config file(s) missing");
        }
        ctx.log().debug(format!(
            "all {} required config files present",
            required.len()
        ));
        Ok(TaskResult::CheckPassed)
    }
}

/// Validate local APM plugin package shape with APM's own pack dry-run.
#[derive(Debug)]
pub(crate) struct ValidateApmPlugins;

impl Task for ValidateApmPlugins {
    task_metadata! {
        name: "APM plugins",
        selector: "apm-plugins",
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if !ctx.which("apm") {
            return Ok(TaskResult::unmet("apm not found in PATH"));
        }
        let plugins =
            discover_apm_plugin_dirs(&ctx.root().join("symlinks").join("apm").join("plugins"))?;
        if plugins.is_empty() {
            ctx.log().info("no local APM plugins found");
            return Ok(TaskResult::CheckPassed);
        }

        let mut failures = 0u32;
        for plugin in &plugins {
            ctx.debug_fmt(|| format!("validating APM plugin {}", plugin.display()));
            let result = ctx
                .executor()
                .execute(
                    CommandSpec::new("apm")
                        .args(&["pack", "--dry-run", "--verbose"])
                        .current_dir(plugin)
                        .unchecked(),
                )
                .with_context(|| format!("running apm pack validation in {}", plugin.display()))?;
            if result.success {
                continue;
            }

            ctx.log().error(format!(
                "APM plugin validation failed: {}",
                plugin.display()
            ));
            log_exec_output(ctx.log(), &result);
            failures = failures.saturating_add(1);
        }

        if failures > 0 {
            anyhow::bail!("{failures} APM plugin(s) failed validation");
        }

        ctx.log()
            .debug(format!("validated {} local APM plugins", plugins.len()));
        Ok(TaskResult::CheckPassed)
    }
}
