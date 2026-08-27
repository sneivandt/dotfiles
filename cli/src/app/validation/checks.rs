//! Validation tasks for the `check` command.
//!
//! These tasks verify configuration integrity and run linters on shell and
//! `PowerShell` scripts.  They are used by [`crate::app::commands::check::run`] but
//! live in the `tasks` module so they follow the same `Task` trait pattern
//! as all other tasks and are independently testable.
use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};

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

const GIT_INDEX_SKIP_WORKTREE: u16 = 1 << 14;

#[derive(Debug, Default)]
pub(super) struct SparseSources {
    pub(super) paths: Vec<PathBuf>,
}

impl SparseSources {
    fn load(root: &Path) -> Result<Self> {
        let repository = match git2::Repository::open(root) {
            Ok(repository) => repository,
            Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(Self::default()),
            Err(error) => {
                return Err(anyhow::Error::from(error)
                    .context(format!("opening repository index at {}", root.display())));
            }
        };
        let index = repository
            .index()
            .with_context(|| format!("opening repository index at {}", root.display()))?;
        let paths = index
            .iter()
            .filter(|entry| entry.flags_extended & GIT_INDEX_SKIP_WORKTREE != 0)
            .map(|entry| PathBuf::from(String::from_utf8_lossy(&entry.path).into_owned()))
            .collect();
        Ok(Self { paths })
    }

    pub(super) fn contains_source(&self, source: &str) -> bool {
        let source = Path::new("symlinks").join(source.replace('\\', "/"));
        self.paths
            .iter()
            .any(|path| path == &source || path.starts_with(&source))
    }

    pub(super) fn contains_glob(&self, source: &str) -> bool {
        let pattern = Path::new(source);
        self.paths.iter().any(|path| {
            path.strip_prefix("symlinks")
                .is_ok_and(|candidate| glob_prefix_matches(pattern, candidate))
        })
    }
}

fn glob_prefix_matches(pattern: &Path, candidate: &Path) -> bool {
    let pattern = pattern.components().collect::<Vec<_>>();
    let candidate = candidate.components().collect::<Vec<_>>();
    candidate.len() >= pattern.len()
        && pattern
            .iter()
            .zip(candidate)
            .all(|(expected, actual)| expected.as_os_str() == "*" || expected == &actual)
}

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

        let main_sparse = SparseSources::load(&repo_root)?;
        let overlay_sparse = overlay_root
            .as_deref()
            .map(SparseSources::load)
            .transpose()?;
        let mut missing = 0u32;

        for symlink in &symlinks {
            let symlinks_dir =
                crate::domains::files::config::symlinks::resolve_symlinks_dir(symlink, &repo_root);
            let source = symlinks_dir.join(&symlink.source);
            let sparse = overlay_root.as_ref().map_or(&main_sparse, |overlay| {
                if symlink.origin.as_ref() == Some(overlay) {
                    overlay_sparse.as_ref().unwrap_or(&main_sparse)
                } else {
                    &main_sparse
                }
            });

            if symlink.source.contains('*') {
                let expanded =
                    crate::domains::files::config::symlinks::expand_present_glob_patterns(
                        std::slice::from_ref(symlink),
                        &repo_root,
                    )
                    .with_context(|| {
                        format!("validating symlink source glob {}", symlink.source)
                    })?;
                if expanded.is_empty() && !sparse.contains_glob(&symlink.source) {
                    ctx.log().error(format!(
                        "symlink source glob matched no entries: {}",
                        source.display()
                    ));
                    missing = missing.saturating_add(1);
                }
            } else if !source.exists() && !sparse.contains_source(&symlink.source) {
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
            let sparse_omitted = main_sparse.contains_source(&entry.path)
                || overlay_sparse
                    .as_ref()
                    .is_some_and(|sparse| sparse.contains_source(&entry.path));
            if !main_source.exists() && !overlay_source_exists && !sparse_omitted {
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
