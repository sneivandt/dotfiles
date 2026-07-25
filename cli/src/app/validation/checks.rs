//! Validation tasks for the `test` command.
//!
//! These tasks verify configuration integrity and run linters on shell and
//! `PowerShell` scripts.  They are used by [`crate::app::commands::test::run`] but
//! live in the `tasks` module so they follow the same `Task` trait pattern
//! as all other tasks and are independently testable.
use anyhow::{Context as _, Result};

use crate::app::config::Config;
use crate::engine::{Context, Task, TaskResult, task_metadata};
use crate::infra::ConfigHandle;

use super::discovery::{
    discover_apm_plugin_dirs, discover_linter_inputs, discover_powershell_scripts,
    discover_shell_scripts,
};
use super::linters::{
    build_psscriptanalyzer_command, build_shellcheck_args, log_exec_output, run_linter,
};
use crate::infra::logging::OutputExt as _;

/// Fail the test command when config validation emits warnings.
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

/// Validate that all symlink source files exist on disk.
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
        !self.config.read().symlinks.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let config = self.config.read();
        let symlinks = config.symlinks.clone();

        let repo_root = config.root.clone();
        let mut missing = 0u32;

        for symlink in &symlinks {
            let symlinks_dir =
                crate::domains::files::config::symlinks::resolve_symlinks_dir(symlink, &repo_root);
            let source = symlinks_dir.join(&symlink.source);
            if !source.exists() {
                ctx.log()
                    .error(format!("symlink source missing: {}", source.display()));
                missing = missing.saturating_add(1);
            }
        }

        if missing > 0 {
            anyhow::bail!("{missing} symlink source(s) missing");
        }

        ctx.log()
            .info(format!("all {} symlink sources exist", symlinks.len()));
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
        let required = [
            "profiles.toml",
            "symlinks.toml",
            "packages.toml",
            "manifest.toml",
        ];

        let mut errors = 0u32;
        for config_file in &required {
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

/// Validate that `symlinks.toml` and `manifest.toml` have matching category
/// sections.
///
/// Every non-`[base]` section in `symlinks.toml` must appear in
/// `manifest.toml`, and every section in `manifest.toml` must appear in
/// `symlinks.toml`.  Drift between the two files causes silent sparse-checkout
/// misconfiguration.
#[derive(Debug)]
pub struct ValidateManifestSync;

impl Task for ValidateManifestSync {
    task_metadata! {
        name: "Validate manifest sync",
        selector: "manifest-sync",
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        use std::collections::{HashMap, HashSet};

        use toml::Value;

        use crate::infra::config::toml_loader;

        let conf = ctx.root().join("conf");
        let symlinks_path = conf.join("symlinks.toml");
        let manifest_path = conf.join("manifest.toml");

        let symlink_raw: HashMap<String, Value> =
            toml_loader::load_required_config(&symlinks_path)?;
        let manifest_raw: HashMap<String, Value> =
            toml_loader::load_required_config(&manifest_path)?;

        let symlink_sections: HashSet<String> = symlink_raw.into_keys().collect();
        let manifest_sections: HashSet<String> = manifest_raw.into_keys().collect();

        let mut warnings: Vec<String> = symlink_sections
            .iter()
            .filter(|s| s.as_str() != "base" && !manifest_sections.contains(*s))
            .map(|s| format!("symlinks.toml has section [{s}] but manifest.toml does not"))
            .chain(
                manifest_sections
                    .iter()
                    .filter(|s| !symlink_sections.contains(*s))
                    .map(|s| format!("manifest.toml has section [{s}] but symlinks.toml does not")),
            )
            .collect();
        warnings.sort_unstable();

        if warnings.is_empty() {
            ctx.log()
                .info("symlinks.toml and manifest.toml sections are in sync");
            return Ok(TaskResult::CheckPassed);
        }

        for warning in &warnings {
            ctx.log().error(warning);
        }
        anyhow::bail!(
            "test failed: {} section(s) differ between symlinks.toml and manifest.toml",
            warnings.len()
        );
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
                .run_unchecked_in(plugin, "apm", &["pack", "--dry-run", "--verbose"])
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
