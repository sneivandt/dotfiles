//! Validation tasks for the `check` command.
//!
//! These tasks verify configuration integrity and run linters on shell and
//! `PowerShell` scripts.  They are used by [`crate::app::commands::check::run`] but
//! live in the `tasks` module so they follow the same `Task` trait pattern
//! as all other tasks and are independently testable.
use anyhow::{Context as _, Result};

use crate::app::config::Config;
use crate::engine::{Context, Task, TaskResult, task_metadata};
use crate::infra::ConfigHandle;
use crate::infra::exec::CommandSpec;

use super::discovery::{
    discover_apm_plugin_dirs, discover_linter_inputs, discover_powershell_scripts,
    discover_shell_scripts,
};
use super::linters::{
    build_psscriptanalyzer_command, build_shellcheck_args, log_exec_output, run_linter,
};
use crate::infra::logging::OutputExt as _;

/// Fail the check command when config validation emits warnings.
#[derive(Debug)]
pub struct ValidateConfigWarnings {
    config: ConfigHandle<Config>,
}

impl ValidateConfigWarnings {
    /// Create the task with a handle to the aggregate configuration.
    #[must_use]
    pub const fn new(config: ConfigHandle<Config>) -> Self {
        Self { config }
    }
}

impl Task for ValidateConfigWarnings {
    task_metadata! {
        name: "Validate config warnings",
        selector: "config-warnings",
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let diagnostics = self.config.read().validate(ctx.platform());
        if diagnostics.is_empty() {
            ctx.log().info("no configuration diagnostics found");
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

        anyhow::bail!(
            "test failed: {} configuration diagnostic(s) found",
            diagnostics.len()
        );
    }
}

/// Validate that all symlink and file-permission source paths exist on disk.
#[derive(Debug)]
pub struct ValidateSymlinkSources {
    config: ConfigHandle<Config>,
}

impl ValidateSymlinkSources {
    /// Create the task with a handle to the aggregate configuration.
    #[must_use]
    pub const fn new(config: ConfigHandle<Config>) -> Self {
        Self { config }
    }
}

impl Task for ValidateSymlinkSources {
    task_metadata! {
        name: "Validate symlink sources",
        selector: "symlink-sources",
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        let config = self.config.read();
        !config.validation_symlinks.is_empty() || !config.validation_chmod.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let config = self.config.read();
        let symlinks = config.validation_symlinks.clone();
        let chmod = config.validation_chmod.clone();

        let repo_root = config.root.clone();
        let overlay_root = config.overlay.clone();
        drop(config);

        let mut missing = 0u32;

        for symlink in &symlinks {
            let symlinks_dir =
                crate::domains::files::config::symlinks::resolve_symlinks_dir(symlink, &repo_root);
            let source = symlinks_dir.join(&symlink.source);
            if symlink.source.contains('*') {
                crate::domains::files::config::symlinks::expand_glob_patterns(
                    std::slice::from_ref(symlink),
                    &repo_root,
                )
                .with_context(|| format!("validating symlink source glob {}", symlink.source))?;
            } else if !source.exists() {
                ctx.log()
                    .error(format!("symlink source missing: {}", source.display()));
                missing = missing.saturating_add(1);
            }
        }

        let main_sources = repo_root.join("symlinks");
        let overlay_sources = overlay_root.map(|root| root.join("symlinks"));
        for entry in &chmod {
            let main_source = main_sources.join(&entry.path);
            let overlay_source_exists = overlay_sources
                .as_ref()
                .is_some_and(|root| root.join(&entry.path).exists());
            if !main_source.exists() && !overlay_source_exists {
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
        ctx.log().info(format!(
            "all {} configured sources exist",
            symlinks.len().saturating_add(chmod.len())
        ));
        Ok(TaskResult::CheckPassed)
    }
}

/// Validate that required configuration files exist.
#[derive(Debug)]
pub struct ValidateConfigFiles;

impl Task for ValidateConfigFiles {
    task_metadata! {
        name: "Validate config files",
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
        ctx.log().info(format!(
            "all {} required config files present",
            required.len()
        ));
        Ok(TaskResult::CheckPassed)
    }
}

/// Validate local APM plugin package shape with APM's own pack dry-run.
#[derive(Debug)]
pub struct ValidateApmPlugins;

impl Task for ValidateApmPlugins {
    task_metadata! {
        name: "Validate APM plugins",
        selector: "apm-plugins",
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.executor().which("apm")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
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
            .info(format!("validated {} local APM plugins", plugins.len()));
        Ok(TaskResult::CheckPassed)
    }
}

/// Run shellcheck on all shell scripts in the repository.
#[derive(Debug)]
pub struct RunShellcheck;

impl Task for RunShellcheck {
    task_metadata! {
        name: "Shellcheck",
        selector: "shellcheck",
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.executor().which("shellcheck")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let scripts = discover_linter_inputs(
            ctx.root(),
            &["dotfiles.sh", "install.sh"],
            &["symlinks", "hooks", ".github"],
            discover_shell_scripts,
        );
        run_linter(
            ctx,
            "shellcheck",
            "shellcheck",
            "shell scripts",
            &scripts,
            build_shellcheck_args,
        )
    }
}

/// Run `PSScriptAnalyzer` on `PowerShell` scripts.
#[derive(Debug)]
pub struct RunPSScriptAnalyzer;

impl Task for RunPSScriptAnalyzer {
    task_metadata! {
        name: "PSScriptAnalyzer",
        selector: "psscriptanalyzer",
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.executor().which("pwsh")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let ps_files = discover_linter_inputs(
            ctx.root(),
            &["dotfiles.ps1"],
            &["symlinks", "hooks"],
            discover_powershell_scripts,
        );
        run_linter(
            ctx,
            "pwsh",
            "PSScriptAnalyzer",
            "PowerShell scripts",
            &ps_files,
            |paths| {
                vec![
                    "-NoProfile".to_owned(),
                    "-Command".to_owned(),
                    build_psscriptanalyzer_command(paths),
                ]
            },
        )
    }
}
