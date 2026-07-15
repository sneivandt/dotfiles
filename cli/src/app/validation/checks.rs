//! Validation tasks for the `test` command.
//!
//! These tasks verify configuration integrity and run linters on shell and
//! `PowerShell` scripts.  They are used by [`crate::app::commands::test::run`] but
//! live in the `tasks` module so they follow the same `Task` trait pattern
//! as all other tasks and are independently testable.
use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};

use crate::app::config::Config;
use crate::engine::{Context, Domain, Task, TaskPhase, TaskResult, task_metadata};
use crate::runtime::ConfigHandle;

const SHELLCHECK_SEVERITY_ARG: &str = "--severity=warning";
const SHELLCHECK_ENABLE_ARG: &str = "--enable=avoid-nullary-conditions";
const SHELLCHECK_EXCLUDE_CODES: &str = "SC1090,SC1091,SC3043,SC2154";

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
        phase: TaskPhase::Validation,
        domain: Domain::Validation,
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let diagnostics = self.config.read().validate(ctx.platform());
        if diagnostics.is_empty() {
            ctx.log().info("no configuration diagnostics found");
            return Ok(TaskResult::Ok);
        }

        for d in &diagnostics {
            ctx.log().error(&format!(
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
        phase: TaskPhase::Validation,
        domain: Domain::Validation,
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
                    .error(&format!("symlink source missing: {}", source.display()));
                missing = missing.saturating_add(1);
            }
        }

        if missing > 0 {
            anyhow::bail!("{missing} symlink source(s) missing");
        }

        ctx.log()
            .info(&format!("all {} symlink sources exist", symlinks.len()));
        Ok(TaskResult::Ok)
    }
}

/// Validate that required configuration files exist.
#[derive(Debug)]
pub struct ValidateConfigFiles;

impl Task for ValidateConfigFiles {
    task_metadata! {
        name: "Validate config files",
        phase: TaskPhase::Validation,
        domain: Domain::Validation,
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
                    .error(&format!("missing config: conf/{config_file}"));
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
        ctx.log().info(&format!(
            "all {} required config files present",
            required.len()
        ));
        Ok(TaskResult::Ok)
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
        phase: TaskPhase::Validation,
        domain: Domain::Validation,
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        use std::collections::{HashMap, HashSet};

        use toml::Value;

        use crate::runtime::config_support::toml_loader;

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
            return Ok(TaskResult::Ok);
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
        phase: TaskPhase::Validation,
        domain: Domain::Validation,
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.executor().which("apm")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let plugins =
            discover_apm_plugin_dirs(&ctx.root().join("symlinks").join("apm").join("plugins"))?;
        if plugins.is_empty() {
            ctx.log().info("no local APM plugins found");
            return Ok(TaskResult::Ok);
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

            ctx.log().error(&format!(
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
            .info(&format!("validated {} local APM plugins", plugins.len()));
        Ok(TaskResult::Ok)
    }
}

/// Run shellcheck on all shell scripts in the repository.
#[derive(Debug)]
pub struct RunShellcheck;

impl Task for RunShellcheck {
    task_metadata! {
        name: "Shellcheck",
        phase: TaskPhase::Validation,
        domain: Domain::Validation,
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.executor().which("shellcheck")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let root = ctx.root();
        let mut scripts: Vec<PathBuf> = Vec::new();

        for name in ["dotfiles.sh", "install.sh"] {
            let path = root.join(name);
            if path.exists() {
                scripts.push(path);
            }
        }

        for dir in ["symlinks", "hooks", ".github"] {
            let dir_path = root.join(dir);
            if dir_path.exists() {
                discover_shell_scripts(&dir_path, &mut scripts);
            }
        }

        if scripts.is_empty() {
            ctx.log().info("no shell scripts found");
            return Ok(TaskResult::Ok);
        }

        ctx.log()
            .debug(&format!("checking {} shell scripts", scripts.len()));

        let args = build_shellcheck_args(&scripts);
        let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();

        let result = ctx.executor().run_unchecked("shellcheck", &arg_refs)?;
        if result.success {
            ctx.log().info("shellcheck passed");
            Ok(TaskResult::Ok)
        } else {
            log_exec_output(ctx.log(), &result);
            anyhow::bail!("shellcheck found issues");
        }
    }
}

/// Run `PSScriptAnalyzer` on `PowerShell` scripts.
#[derive(Debug)]
pub struct RunPSScriptAnalyzer;

impl Task for RunPSScriptAnalyzer {
    task_metadata! {
        name: "PSScriptAnalyzer",
        phase: TaskPhase::Validation,
        domain: Domain::Validation,
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.executor().which("pwsh")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let root = ctx.root();
        let mut ps_files: Vec<PathBuf> = Vec::new();

        for dir in ["symlinks", "hooks"] {
            let dir_path = root.join(dir);
            if dir_path.exists() {
                discover_powershell_scripts(&dir_path, &mut ps_files);
            }
        }

        let path = root.join("dotfiles.ps1");
        if path.exists() {
            ps_files.push(path);
        }

        if ps_files.is_empty() {
            ctx.log().info("no PowerShell scripts found");
            return Ok(TaskResult::Ok);
        }

        ctx.log()
            .debug(&format!("checking {} PowerShell scripts", ps_files.len()));

        let script = build_psscriptanalyzer_command(&ps_files);

        let result = ctx
            .executor()
            .run_unchecked("pwsh", &["-NoProfile", "-Command", &script])?;
        if result.success {
            ctx.log().info("PSScriptAnalyzer passed");
            Ok(TaskResult::Ok)
        } else {
            log_exec_output(ctx.log(), &result);
            anyhow::bail!("PSScriptAnalyzer found issues");
        }
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Recursively discover files in a directory tree that match a predicate.
pub(crate) fn discover_files<F>(dir: &Path, predicate: F, out: &mut Vec<PathBuf>)
where
    F: Fn(&Path) -> bool + Copy,
{
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            discover_files(&path, predicate, out);
        } else if path.is_file() && predicate(&path) {
            out.push(path);
        }
    }
}

/// Recursively discover shell scripts in a directory.
///
/// A file is considered a shell script if it has a `.sh` extension or its
/// first line contains a shebang for a known POSIX-compatible interpreter
/// (see [`SHELL_INTERPRETERS`]).  Files with a `.zsh` extension are always
/// excluded; zsh shebangs are implicitly excluded because `zsh` is not in
/// `SHELL_INTERPRETERS` (shellcheck does not support zsh syntax).
pub(crate) fn discover_shell_scripts(dir: &Path, out: &mut Vec<PathBuf>) {
    discover_files(
        dir,
        |path| {
            if path.extension().is_some_and(|e| e == "zsh") {
                return false;
            }
            path.extension().is_some_and(|e| e == "sh") || is_shell_shebang(path)
        },
        out,
    );
}

/// Recursively discover `PowerShell` scripts (.ps1, .psm1, .psd1) in a directory.
pub(crate) fn discover_powershell_scripts(dir: &Path, out: &mut Vec<PathBuf>) {
    discover_files(
        dir,
        |path| {
            path.extension()
                .is_some_and(|e| e == "ps1" || e == "psm1" || e == "psd1")
                || is_powershell_shebang(path)
        },
        out,
    );
}

/// Discover local APM plugin directories.
pub(crate) fn discover_apm_plugin_dirs(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(err)
                .with_context(|| format!("reading APM plugins directory {}", dir.display()));
        }
    };

    let mut plugins = Vec::new();
    for entry in entries {
        let entry =
            entry.with_context(|| format!("reading directory entry in {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() && path.join("apm.yml").is_file() {
            plugins.push(path);
        }
    }
    plugins.sort();
    Ok(plugins)
}

/// Known POSIX-compatible shell interpreter basenames that shellcheck supports.
const SHELL_INTERPRETERS: &[&[u8]] = &[b"sh", b"bash", b"dash", b"ksh"];

/// Known `PowerShell` interpreter basenames.
const POWERSHELL_INTERPRETERS: &[&[u8]] = &[b"pwsh", b"powershell"];

/// Check if a file has a POSIX-shell shebang (e.g. `#!/bin/bash`).
///
/// Only matches known shell interpreters to avoid false positives from
/// interpreters that happen to contain "sh" (e.g. `fish`, `csh`).
fn is_shell_shebang(path: &Path) -> bool {
    shebang_matches(path, SHELL_INTERPRETERS)
}

/// Check if a file has a `PowerShell` shebang (e.g. `#!/usr/bin/env pwsh`).
fn is_powershell_shebang(path: &Path) -> bool {
    shebang_matches(path, POWERSHELL_INTERPRETERS)
}

fn shebang_matches(path: &Path, interpreters: &[&[u8]]) -> bool {
    parse_shebang_interpreter(path).is_some_and(|name| {
        let trimmed = name.strip_suffix(b".exe").unwrap_or(name.as_slice());
        interpreters.contains(&trimmed)
    })
}

/// Parse shebang line to extract the interpreter name.
///
/// Returns the interpreter name from a shebang line, handling:
/// - Direct paths: `#!/bin/bash` → `bash`
/// - Non-standard paths: `#!/usr/local/bin/bash` → `bash`
/// - Env wrappers: `#!/usr/bin/env bash` → `bash`
/// - Env with flags: `#!/usr/bin/env -S bash` → `bash`
/// - With arguments: `#!/bin/sh -e` → `sh`
fn parse_shebang_interpreter(path: &Path) -> Option<Vec<u8>> {
    let first_line = read_first_line(path);
    if !first_line.starts_with(b"#!") {
        return None;
    }
    let shebang = first_line.get(2..).unwrap_or(&[]);
    // Split the shebang line into whitespace-separated tokens.
    let mut tokens = shebang
        .split(|&b| b == b' ' || b == b'\t')
        .filter(|s| !s.is_empty());
    // The first token is the interpreter path (e.g. `/usr/bin/env` or `/bin/bash`).
    let prog_path = tokens.next()?;
    // Extract the basename — the last `/`-separated component.
    let prog = prog_path
        .rsplit(|&b| b == b'/')
        .next()
        .filter(|s| !s.is_empty())?;
    let prog = prog.strip_suffix(b".exe").unwrap_or(prog);
    if prog == b"env" {
        // With `env`, skip option flags (tokens starting with `-`) and take
        // the first non-flag argument as the actual interpreter name.
        tokens.find(|s| !s.starts_with(b"-")).map(<[u8]>::to_vec)
    } else {
        Some(prog.to_vec())
    }
}

/// Read the first line of a file (up to 256 bytes).
fn read_first_line(path: &Path) -> Vec<u8> {
    use std::io::Read;

    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut buf = [0u8; 256];
    let n = file.read(&mut buf).unwrap_or(0);
    let end = buf
        .get(..n)
        .and_then(|slice| slice.iter().position(|&b| b == b'\n'))
        .unwrap_or(n);
    buf.get(..end).unwrap_or_default().to_vec()
}

/// Log command output (stdout and stderr) through the logger.
fn log_exec_output(
    log: &dyn crate::runtime::logging::Log,
    result: &crate::runtime::exec::ExecResult,
) {
    for line in result.stdout.lines().chain(result.stderr.lines()) {
        log.error(line);
    }
}

pub(super) fn build_psscriptanalyzer_command(paths: &[PathBuf]) -> String {
    let path_literals = paths
        .iter()
        .map(|path| powershell_single_quote(&path.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "$paths = @({path_literals}); \
         if (!(Get-Module -ListAvailable PSScriptAnalyzer)) \
         {{ Write-Host 'PSScriptAnalyzer not installed, skipping'; exit 0 }}; \
         $results = $paths | ForEach-Object \
         {{ Invoke-ScriptAnalyzer -Path $_ -Severity Warning,Error }}; \
         if ($results.Count -gt 0) {{ $results | Format-Table -AutoSize; exit 1 }} \
         else {{ exit 0 }}"
    )
}

fn powershell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub(super) fn build_shellcheck_args(paths: &[PathBuf]) -> Vec<String> {
    let mut args = vec![
        SHELLCHECK_SEVERITY_ARG.to_string(),
        format!("--exclude={SHELLCHECK_EXCLUDE_CODES}"),
        SHELLCHECK_ENABLE_ARG.to_string(),
    ];
    args.extend(paths.iter().map(|path| path.to_string_lossy().into_owned()));
    args
}
