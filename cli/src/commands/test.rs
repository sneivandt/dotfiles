use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::cli::{GlobalOpts, TestOpts};
use crate::exec;
use crate::logging::Logger;
use crate::tasks::{Context, Task, TaskResult};

/// Run the test/validation command.
///
/// # Errors
///
/// Returns an error if profile resolution, configuration validation, or script checks fail.
pub fn run(global: &GlobalOpts, _opts: &TestOpts, log: &Logger) -> Result<()> {
    let executor = exec::SystemExecutor;
    let setup = super::CommandSetup::init(global, log)?;
    let ctx = Context::new(
        &setup.config,
        &setup.platform,
        log,
        global.dry_run,
        &executor,
        global.parallel,
    )?;

    let tasks: Vec<Box<dyn Task>> = vec![
        Box::new(ValidateSymlinkSources),
        Box::new(ValidateConfigFiles),
        Box::new(RunShellcheck),
        Box::new(RunPSScriptAnalyzer),
    ];

    super::run_tasks_to_completion(tasks.iter().map(Box::as_ref), &ctx, log)
}

// ---------------------------------------------------------------------------
// Validation tasks
// ---------------------------------------------------------------------------

/// Validate that all symlink source files exist on disk.
#[derive(Debug)]
struct ValidateSymlinkSources;

impl Task for ValidateSymlinkSources {
    fn name(&self) -> &'static str {
        "Validate symlink sources"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        !ctx.config.symlinks.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let symlinks_dir = ctx.symlinks_dir();
        let mut missing = 0u32;

        for symlink in &ctx.config.symlinks {
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

        ctx.log.info(&format!(
            "all {} symlink sources exist",
            ctx.config.symlinks.len()
        ));
        Ok(TaskResult::Ok)
    }
}

/// Validate that required configuration files exist.
#[derive(Debug)]
struct ValidateConfigFiles;

impl Task for ValidateConfigFiles {
    fn name(&self) -> &'static str {
        "Validate config files"
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        true
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let conf = ctx.root().join("conf");
        let required = [
            "profiles.ini",
            "symlinks.ini",
            "packages.ini",
            "manifest.ini",
        ];

        let mut errors = 0u32;
        for config_file in &required {
            let path = conf.join(config_file);
            if path.exists() {
                ctx.log.debug(&format!("found conf/{config_file}"));
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

/// Run shellcheck on all shell scripts in the repository.
#[derive(Debug)]
struct RunShellcheck;

impl Task for RunShellcheck {
    fn name(&self) -> &'static str {
        "Shellcheck"
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

        let mut args: Vec<&str> = vec!["--severity=warning"];
        let paths: Vec<_> = scripts
            .iter()
            .filter_map(|p| p.to_str().map(String::from))
            .collect();
        args.extend(paths.iter().map(String::as_str));

        let result = ctx.executor.run_unchecked("shellcheck", &args)?;
        if result.success {
            ctx.log.info("shellcheck passed");
            Ok(TaskResult::Ok)
        } else {
            print_exec_output(&result);
            anyhow::bail!("shellcheck found issues");
        }
    }
}

/// Run `PSScriptAnalyzer` on `PowerShell` scripts.
#[derive(Debug)]
struct RunPSScriptAnalyzer;

impl Task for RunPSScriptAnalyzer {
    fn name(&self) -> &'static str {
        "PSScriptAnalyzer"
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

        let file_list: Vec<&str> = ps_files.iter().filter_map(|p| p.to_str()).collect();
        let paths_arg = file_list.join("','");

        let script = format!(
            "if (!(Get-Module -ListAvailable PSScriptAnalyzer)) \
             {{ Write-Host 'PSScriptAnalyzer not installed, skipping'; exit 0 }}; \
             $results = @('{paths_arg}') | ForEach-Object \
             {{ Invoke-ScriptAnalyzer -Path $_ -Severity Warning,Error }}; \
             if ($results.Count -gt 0) {{ $results | Format-Table -AutoSize; exit 1 }} \
             else {{ exit 0 }}"
        );

        let result = ctx
            .executor
            .run_unchecked("pwsh", &["-NoProfile", "-Command", &script])?;
        if result.success {
            ctx.log.info("PSScriptAnalyzer passed");
            Ok(TaskResult::Ok)
        } else {
            print_exec_output(&result);
            anyhow::bail!("PSScriptAnalyzer found issues");
        }
    }
}

/// Recursively discover files in a directory tree that match a predicate.
fn discover_files<F>(dir: &Path, predicate: F, out: &mut Vec<PathBuf>)
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
/// A file is considered a shell script if it has a `.sh` extension
/// or its first line starts with `#!/` and contains `sh`.
/// Files with `.zsh` extension or zsh shebangs are excluded (shellcheck
/// doesn't support zsh syntax).
fn discover_shell_scripts(dir: &Path, out: &mut Vec<PathBuf>) {
    discover_files(
        dir,
        |path| {
            if path.extension().is_some_and(|e| e == "zsh") {
                return false;
            }
            path.extension().is_some_and(|e| e == "sh")
                || (is_shell_shebang(path) && !is_zsh_shebang(path))
        },
        out,
    );
}

/// Recursively discover `PowerShell` scripts (.ps1, .psm1, .psd1) in a directory.
fn discover_powershell_scripts(dir: &Path, out: &mut Vec<PathBuf>) {
    discover_files(
        dir,
        |path| {
            path.extension()
                .is_some_and(|e| e == "ps1" || e == "psm1" || e == "psd1")
        },
        out,
    );
}

/// Known POSIX-compatible shell interpreter basenames that shellcheck supports.
const SHELL_INTERPRETERS: &[&[u8]] = &[b"sh", b"bash", b"dash", b"ksh"];

/// Check if a file has a POSIX-shell shebang (e.g. `#!/bin/bash`).
///
/// Only matches known shell interpreters to avoid false positives from
/// interpreters that happen to contain "sh" (e.g. `fish`, `csh`).
fn is_shell_shebang(path: &Path) -> bool {
    parse_shebang_interpreter(path)
        .is_some_and(|name| SHELL_INTERPRETERS.contains(&name.as_slice()))
}

/// Check if a file has a zsh shebang (e.g. `#!/bin/zsh`).
fn is_zsh_shebang(path: &Path) -> bool {
    parse_shebang_interpreter(path).is_some_and(|name| name == b"zsh")
}

/// Parse shebang line to extract the interpreter name.
///
/// Returns the interpreter name from a shebang line, handling:
/// - Direct paths: `#!/bin/bash` → `bash`
/// - Env wrappers: `#!/usr/bin/env bash` → `bash`
/// - With arguments: `#!/bin/sh -e` → `sh`
fn parse_shebang_interpreter(path: &Path) -> Option<Vec<u8>> {
    let first_line = read_first_line(path);
    if !first_line.starts_with(b"#!") {
        return None;
    }
    let shebang = first_line.get(2..).unwrap_or(&[]);
    shebang
        .split(|&b| b == b' ' || b == b'/' || b == b'\t')
        .find(|s| !s.is_empty() && *s != b"usr" && *s != b"bin" && *s != b"env")
        .map(<[u8]>::to_vec)
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

/// Print command output (stdout and stderr) to stderr.
fn print_exec_output(result: &crate::exec::ExecResult) {
    if !result.stdout.is_empty() {
        eprintln!("{}", result.stdout);
    }
    if !result.stderr.is_empty() {
        eprintln!("{}", result.stderr);
    }
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
