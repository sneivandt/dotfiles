//! Validation tasks for the `test` command.
//!
//! These tasks verify configuration integrity and run linters on shell and
//! `PowerShell` scripts.  They are used by [`crate::commands::test::run`] but
//! live in the `tasks` module so they follow the same `Task` trait pattern
//! as all other tasks and are independently testable.
use anyhow::Result;
use std::path::{Path, PathBuf};

use super::{Context, Task, TaskPhase, TaskResult};

const SHELLCHECK_SEVERITY_ARG: &str = "--severity=warning";
const SHELLCHECK_ENABLE_ARG: &str = "--enable=avoid-nullary-conditions";
const SHELLCHECK_EXCLUDE_CODES: &str = "SC1090,SC1091,SC3043,SC2154";

/// Fail the test command when config validation emits warnings.
#[derive(Debug)]
pub struct ValidateConfigWarnings;

impl Task for ValidateConfigWarnings {
    fn name(&self) -> &'static str {
        "Validate config warnings"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Apply
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        true
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let warnings = ctx.config_read().validate(ctx.platform);
        if warnings.is_empty() {
            ctx.log.info("no configuration warnings found");
            return Ok(TaskResult::Ok);
        }

        for warning in &warnings {
            ctx.log.error(&format!(
                "{} [{}]: {}",
                warning.source, warning.item, warning.message
            ));
        }

        anyhow::bail!(
            "test failed: {} configuration warning(s) found",
            warnings.len()
        );
    }
}

/// Validate that all symlink source files exist on disk.
#[derive(Debug)]
pub struct ValidateSymlinkSources;

impl Task for ValidateSymlinkSources {
    fn name(&self) -> &'static str {
        "Validate symlink sources"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Apply
    }

    fn should_run(&self, ctx: &Context) -> bool {
        !ctx.config_read().symlinks.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let config = ctx.config_read();
        let symlinks = config.symlinks.clone();

        let repo_root = ctx.root();
        let mut missing = 0u32;

        for symlink in &symlinks {
            let symlinks_dir = crate::config::symlinks::resolve_symlinks_dir(symlink, &repo_root);
            let source = symlinks_dir.join(&symlink.source);
            if !source.exists() {
                ctx.log
                    .error(&format!("symlink source missing: {}", source.display()));
                missing += 1;
            }
        }

        if missing > 0 {
            anyhow::bail!("{missing} symlink source(s) missing");
        }

        ctx.log
            .info(&format!("all {} symlink sources exist", symlinks.len()));
        Ok(TaskResult::Ok)
    }
}

/// Validate that required configuration files exist.
#[derive(Debug)]
pub struct ValidateConfigFiles;

impl Task for ValidateConfigFiles {
    fn name(&self) -> &'static str {
        "Validate config files"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Apply
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        true
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let conf = ctx.root().join("conf");
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
                ctx.log
                    .error(&format!("missing config: conf/{config_file}"));
                errors += 1;
            }
        }

        let hooks_dir = ctx.root().join("hooks");
        if hooks_dir.exists() {
            ctx.log.debug("found hooks directory");
        } else {
            ctx.log.warn("hooks directory missing");
        }

        if errors > 0 {
            anyhow::bail!("{errors} required config file(s) missing");
        }
        ctx.log.info(&format!(
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
    fn name(&self) -> &'static str {
        "Validate manifest sync"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Apply
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        true
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        use std::collections::{HashMap, HashSet};

        use toml::Value;

        use crate::config::helpers::toml_loader;

        let conf = ctx.root().join("conf");
        let symlinks_path = conf.join("symlinks.toml");
        let manifest_path = conf.join("manifest.toml");

        let symlink_raw: HashMap<String, Value> = toml_loader::load_config(&symlinks_path)?;
        let manifest_raw: HashMap<String, Value> = toml_loader::load_config(&manifest_path)?;

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
            ctx.log
                .info("symlinks.toml and manifest.toml sections are in sync");
            return Ok(TaskResult::Ok);
        }

        for warning in &warnings {
            ctx.log.error(warning);
        }
        anyhow::bail!(
            "test failed: {} section(s) differ between symlinks.toml and manifest.toml",
            warnings.len()
        );
    }
}

/// Run shellcheck on all shell scripts in the repository.
#[derive(Debug)]
pub struct RunShellcheck;

impl Task for RunShellcheck {
    fn name(&self) -> &'static str {
        "Shellcheck"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Apply
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.executor.which("shellcheck")
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
            ctx.log.info("no shell scripts found");
            return Ok(TaskResult::Ok);
        }

        ctx.log
            .info(&format!("checking {} shell scripts", scripts.len()));

        let args = build_shellcheck_args(&scripts);
        let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();

        let result = ctx.executor.run_unchecked("shellcheck", &arg_refs)?;
        if result.success {
            ctx.log.info("shellcheck passed");
            Ok(TaskResult::Ok)
        } else {
            log_exec_output(&*ctx.log, &result);
            anyhow::bail!("shellcheck found issues");
        }
    }
}

/// Run `PSScriptAnalyzer` on `PowerShell` scripts.
#[derive(Debug)]
pub struct RunPSScriptAnalyzer;

impl Task for RunPSScriptAnalyzer {
    fn name(&self) -> &'static str {
        "PSScriptAnalyzer"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Apply
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.executor.which("pwsh")
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
            ctx.log.info("no PowerShell scripts found");
            return Ok(TaskResult::Ok);
        }

        ctx.log
            .info(&format!("checking {} PowerShell scripts", ps_files.len()));

        let script = build_psscriptanalyzer_command(&ps_files);

        let result = ctx
            .executor
            .run_unchecked("pwsh", &["-NoProfile", "-Command", &script])?;
        if result.success {
            ctx.log.info("PSScriptAnalyzer passed");
            Ok(TaskResult::Ok)
        } else {
            log_exec_output(&*ctx.log, &result);
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
fn log_exec_output(log: &dyn crate::logging::Log, result: &crate::exec::ExecResult) {
    for line in result.stdout.lines().chain(result.stderr.lines()) {
        log.error(line);
    }
}

fn build_psscriptanalyzer_command(paths: &[PathBuf]) -> String {
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

fn build_shellcheck_args(paths: &[PathBuf]) -> Vec<String> {
    let mut args = vec![
        SHELLCHECK_SEVERITY_ARG.to_string(),
        format!("--exclude={SHELLCHECK_EXCLUDE_CODES}"),
        SHELLCHECK_ENABLE_ARG.to_string(),
    ];
    args.extend(paths.iter().map(|path| path.to_string_lossy().into_owned()));
    args
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn detects_sh_extension() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let script = dir.path().join("test.sh");
        std::fs::write(&script, "echo hello").expect("write should succeed");

        let mut found = Vec::new();
        discover_shell_scripts(dir.path(), &mut found);
        assert_eq!(found.len(), 1);
        assert_eq!(found.first().expect("found 0 should exist"), &script);
    }

    #[test]
    fn detects_shebang_without_extension() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let script = dir.path().join("myscript");
        let mut f = std::fs::File::create(&script).expect("create should succeed");
        f.write_all(b"#!/bin/bash\necho hello")
            .expect("write_all should succeed");

        let mut found = Vec::new();
        discover_shell_scripts(dir.path(), &mut found);
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn ignores_non_shell_files() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        std::fs::write(dir.path().join("readme.md"), "# Hello").expect("write should succeed");
        std::fs::write(dir.path().join("data.json"), "{}").expect("write should succeed");

        let mut found = Vec::new();
        discover_shell_scripts(dir.path(), &mut found);
        assert!(found.is_empty());
    }

    #[test]
    fn discovers_ps1_files() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let script_path = dir.path().join("test.ps1");
        let module_path = dir.path().join("module.psm1");
        std::fs::write(&script_path, "Write-Host 'hi'").expect("write should succeed");
        std::fs::write(&module_path, "function Test {}").expect("write should succeed");
        std::fs::write(dir.path().join("readme.md"), "# Hello").expect("write should succeed");

        let mut found = Vec::new();
        discover_powershell_scripts(dir.path(), &mut found);
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn discovers_powershell_shebang_without_extension() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let script = dir.path().join("profile-hook");
        std::fs::write(&script, "#!/usr/bin/env pwsh\nWrite-Host 'hi'")
            .expect("write should succeed");

        let mut found = Vec::new();
        discover_powershell_scripts(dir.path(), &mut found);
        assert_eq!(found, vec![script]);
    }

    #[test]
    fn powershell_command_escapes_single_quotes_in_paths() {
        let path = PathBuf::from("C:\\Users\\o'connor\\script.ps1");
        let script = build_psscriptanalyzer_command(&[path]);
        assert!(
            script.contains("C:\\Users\\o''connor\\script.ps1"),
            "single quotes in file paths must be PowerShell-escaped"
        );
    }

    #[test]
    fn shellcheck_command_includes_project_defaults() {
        let args = build_shellcheck_args(&[
            PathBuf::from("dotfiles.sh"),
            PathBuf::from("hooks/pre-commit"),
        ]);

        assert_eq!(
            args,
            vec![
                "--severity=warning".to_string(),
                "--exclude=SC1090,SC1091,SC3043,SC2154".to_string(),
                "--enable=avoid-nullary-conditions".to_string(),
                "dotfiles.sh".to_string(),
                "hooks/pre-commit".to_string(),
            ]
        );
    }

    #[test]
    fn shebang_detects_various_shells() {
        let dir = tempfile::tempdir().expect("tempdir should create");

        for (name, shebang) in [
            ("a", "#!/bin/sh\n"),
            ("b", "#!/bin/bash\n"),
            ("c", "#!/usr/bin/env zsh\n"),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, shebang).expect("write should succeed");
        }

        let mut found = Vec::new();
        discover_shell_scripts(dir.path(), &mut found);
        // zsh scripts are excluded (shellcheck doesn't support them)
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn shebang_excludes_non_posix_shells() {
        let dir = tempfile::tempdir().expect("tempdir should create");

        // These should NOT be detected as shell scripts
        for (name, shebang) in [
            ("fish_script", "#!/usr/bin/fish\n"),
            ("csh_script", "#!/bin/csh\n"),
            ("tcsh_script", "#!/usr/bin/tcsh\n"),
            ("python_script", "#!/usr/bin/python3\n"),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, shebang).expect("write should succeed");
        }

        let mut found = Vec::new();
        discover_shell_scripts(dir.path(), &mut found);
        assert!(
            found.is_empty(),
            "should not match non-POSIX shell shebangs"
        );
    }

    #[test]
    fn shebang_detects_env_wrappers() {
        let dir = tempfile::tempdir().expect("tempdir should create");

        for (name, shebang) in [
            ("a", "#!/usr/bin/env sh\n"),
            ("b", "#!/usr/bin/env bash\n"),
            ("c", "#!/usr/bin/env dash\n"),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, shebang).expect("write should succeed");
        }

        let mut found = Vec::new();
        discover_shell_scripts(dir.path(), &mut found);
        assert_eq!(found.len(), 3);
    }

    #[test]
    fn shebang_with_arguments() {
        let dir = tempfile::tempdir().expect("tempdir should create");

        // Shebangs with arguments should still correctly identify the interpreter
        for (name, shebang) in [
            ("a", "#!/bin/sh -e\n"),
            ("b", "#!/bin/bash -x\n"),
            ("c", "#!/usr/bin/env bash -e\n"),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, shebang).expect("write should succeed");
        }

        let mut found = Vec::new();
        discover_shell_scripts(dir.path(), &mut found);
        assert_eq!(found.len(), 3, "should detect shell scripts with arguments");
    }

    #[test]
    fn shebang_detects_non_standard_install_paths() {
        let dir = tempfile::tempdir().expect("tempdir should create");

        // Non-standard paths like /usr/local/bin or /opt/homebrew/bin (macOS)
        // must still correctly resolve the interpreter name.
        for (name, shebang) in [
            ("a", "#!/usr/local/bin/bash\n"),
            ("b", "#!/opt/homebrew/bin/bash\n"),
            ("c", "#!/usr/local/bin/sh\n"),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, shebang).expect("write should succeed");
        }

        let mut found = Vec::new();
        discover_shell_scripts(dir.path(), &mut found);
        assert_eq!(
            found.len(),
            3,
            "should detect shell scripts with non-standard install paths"
        );
    }

    #[test]
    fn shebang_detects_env_with_flags() {
        let dir = tempfile::tempdir().expect("tempdir should create");

        // `env -S` is used to pass arguments through env on some systems.
        let path = dir.path().join("script");
        std::fs::write(&path, "#!/usr/bin/env -S bash -e\necho hi\n")
            .expect("write should succeed");

        let mut found = Vec::new();
        discover_shell_scripts(dir.path(), &mut found);
        assert_eq!(found.len(), 1, "should detect shell scripts with env -S");
    }

    #[test]
    fn discover_files_with_custom_predicate() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        std::fs::write(dir.path().join("a.txt"), "hello").expect("write should succeed");
        std::fs::write(dir.path().join("b.txt"), "world").expect("write should succeed");
        std::fs::write(dir.path().join("c.md"), "# doc").expect("write should succeed");
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).expect("create_dir should succeed");
        std::fs::write(sub.join("d.txt"), "nested").expect("write should succeed");

        let mut found = Vec::new();
        discover_files(
            dir.path(),
            |p| p.extension().is_some_and(|e| e == "txt"),
            &mut found,
        );
        assert_eq!(found.len(), 3, "should find .txt files recursively");
    }
}
