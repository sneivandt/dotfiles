use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::cli::{GlobalOpts, TestOpts};
use crate::config::Config;
use crate::config::profiles;
use crate::exec;
use crate::logging::Logger;
use crate::platform::Platform;

/// Run the test/validation command.
pub fn run(global: &GlobalOpts, _opts: &TestOpts, log: &Logger) -> Result<()> {
    let platform = Platform::detect();
    let root = super::install::resolve_root(global)?;

    log.stage("Resolving profile");
    let persisted = profiles::read_persisted(&root);
    let profile_name = global
        .profile
        .as_deref()
        .or(persisted.as_deref())
        .unwrap_or("base");

    let conf_dir = root.join("conf");
    let profile = profiles::resolve(profile_name, &conf_dir, &platform)?;
    log.info(&format!("profile: {}", profile.name));

    log.stage("Validating configuration");
    let config = Config::load(&root, &profile, &platform)?;

    // Validate symlink sources exist
    let symlinks_dir = root.join("symlinks");
    let mut errors = 0u32;

    for symlink in &config.symlinks {
        let source = symlinks_dir.join(&symlink.source);
        if !source.exists() {
            log.error(&format!("symlink source missing: {}", source.display()));
            errors += 1;
        }
    }

    // Validate hooks directory
    let hooks_dir = root.join("hooks");
    if !hooks_dir.exists() {
        log.warn("hooks directory missing");
    }

    // Validate conf directory
    let conf = root.join("conf");
    let required_configs = [
        "profiles.ini",
        "symlinks.ini",
        "packages.ini",
        "manifest.ini",
    ];
    for config_file in &required_configs {
        if !conf.join(config_file).exists() {
            log.error(&format!("missing config: conf/{config_file}"));
            errors += 1;
        }
    }

    // Static analysis: shellcheck
    errors += run_shellcheck(&root, log)?;

    // Static analysis: PSScriptAnalyzer (when pwsh is available)
    errors += run_psscriptanalyzer(&root, log)?;

    log.stage("Validation complete");
    if errors > 0 {
        anyhow::bail!("{errors} validation errors found");
    }

    log.info("all checks passed");
    Ok(())
}

/// Discover and shellcheck all shell scripts in the repository.
fn run_shellcheck(root: &Path, log: &Logger) -> Result<u32> {
    if !exec::which("shellcheck") {
        log.warn("shellcheck not installed, skipping");
        return Ok(0);
    }

    log.stage("Running shellcheck");

    let mut scripts: Vec<PathBuf> = Vec::new();

    // Known entry-point scripts
    for name in ["dotfiles.sh", "install.sh"] {
        let path = root.join(name);
        if path.exists() {
            scripts.push(path);
        }
    }

    // Walk directories for shell scripts
    for dir in ["symlinks", "hooks", ".github"] {
        let dir_path = root.join(dir);
        if dir_path.exists() {
            discover_shell_scripts(&dir_path, &mut scripts);
        }
    }

    if scripts.is_empty() {
        log.info("no shell scripts found");
        return Ok(0);
    }

    log.info(&format!("checking {} shell scripts", scripts.len()));

    // Only report warnings and errors (not info/style)
    let mut args: Vec<&str> = vec!["--severity=warning"];
    let paths: Vec<String> = scripts
        .iter()
        .filter_map(|p| p.to_str().map(String::from))
        .collect();
    args.extend(paths.iter().map(std::string::String::as_str));
    let result = exec::run_unchecked("shellcheck", &args)?;
    if result.success {
        log.info("shellcheck passed");
        Ok(0)
    } else {
        log.error("shellcheck found issues:");
        // Print stdout/stderr so user sees the actual findings
        if !result.stdout.is_empty() {
            eprintln!("{}", result.stdout);
        }
        if !result.stderr.is_empty() {
            eprintln!("{}", result.stderr);
        }
        Ok(1)
    }
}

/// Run `PSScriptAnalyzer` on `PowerShell` files when pwsh is available.
fn run_psscriptanalyzer(root: &Path, log: &Logger) -> Result<u32> {
    if !exec::which("pwsh") {
        log.info("pwsh not installed, skipping PSScriptAnalyzer");
        return Ok(0);
    }

    log.stage("Running PSScriptAnalyzer");

    // Discover .ps1 and .psm1 files
    let mut ps_files: Vec<PathBuf> = Vec::new();
    for dir in ["symlinks", "hooks"] {
        let dir_path = root.join(dir);
        if dir_path.exists() {
            discover_powershell_scripts(&dir_path, &mut ps_files);
        }
    }
    // Check entry-point scripts
    let path = root.join("dotfiles.ps1");
    if path.exists() {
        ps_files.push(path);
    }

    if ps_files.is_empty() {
        log.info("no PowerShell scripts found");
        return Ok(0);
    }

    log.info(&format!("checking {} PowerShell scripts", ps_files.len()));

    let file_list: Vec<&str> = ps_files.iter().filter_map(|p| p.to_str()).collect();
    let paths_arg = file_list.join("','");

    let script = format!(
        "if (!(Get-Module -ListAvailable PSScriptAnalyzer)) {{ Write-Host 'PSScriptAnalyzer not installed, skipping'; exit 0 }}; \
         $results = @('{paths_arg}') | ForEach-Object {{ Invoke-ScriptAnalyzer -Path $_ -Severity Warning,Error }}; \
         if ($results.Count -gt 0) {{ $results | Format-Table -AutoSize; exit 1 }} else {{ exit 0 }}"
    );

    let result = exec::run_unchecked("pwsh", &["-NoProfile", "-Command", &script])?;
    if result.success {
        log.info("PSScriptAnalyzer passed");
        Ok(0)
    } else {
        log.error("PSScriptAnalyzer found issues:");
        if !result.stdout.is_empty() {
            eprintln!("{}", result.stdout);
        }
        if !result.stderr.is_empty() {
            eprintln!("{}", result.stderr);
        }
        Ok(1)
    }
}

/// Recursively discover shell scripts in a directory.
///
/// A file is considered a shell script if it has a `.sh` extension
/// or its first line starts with `#!/` and contains `sh`.
/// Files with `.zsh` extension or zsh shebangs are excluded (shellcheck
/// doesn't support zsh syntax).
fn discover_shell_scripts(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            discover_shell_scripts(&path, out);
        } else if path.is_file() {
            // Skip zsh files — shellcheck can't parse them
            if path.extension().is_some_and(|e| e == "zsh") {
                continue;
            }
            let dominated = path.extension().is_some_and(|e| e == "sh")
                || (is_shell_shebang(&path) && !is_zsh_shebang(&path));
            if dominated {
                out.push(path);
            }
        }
    }
}

/// Recursively discover `PowerShell` scripts (.ps1, .psm1) in a directory.
fn discover_powershell_scripts(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            discover_powershell_scripts(&path, out);
        } else if path.is_file()
            && let Some(ext) = path.extension()
            && (ext == "ps1" || ext == "psm1" || ext == "psd1")
        {
            out.push(path);
        }
    }
}

/// Check if a file has a shell shebang (#!/...sh).
fn is_shell_shebang(path: &Path) -> bool {
    let first_line = read_first_line(path);
    first_line.starts_with(b"#!") && first_line.windows(2).any(|w| w == b"sh")
}

/// Check if a file has a zsh shebang (#!/...zsh).
fn is_zsh_shebang(path: &Path) -> bool {
    let first_line = read_first_line(path);
    first_line.starts_with(b"#!") && first_line.windows(3).any(|w| w == b"zsh")
}

/// Read the first line of a file (up to 256 bytes).
fn read_first_line(path: &Path) -> Vec<u8> {
    use std::io::Read;

    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut buf = [0u8; 256];
    let n = file.read(&mut buf).unwrap_or(0);
    let end = buf[..n].iter().position(|&b| b == b'\n').unwrap_or(n);
    buf[..end].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn detects_sh_extension() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("test.sh");
        std::fs::write(&script, "echo hello").unwrap();

        let mut found = Vec::new();
        discover_shell_scripts(dir.path(), &mut found);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0], script);
    }

    #[test]
    fn detects_shebang_without_extension() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("myscript");
        let mut f = std::fs::File::create(&script).unwrap();
        f.write_all(b"#!/bin/bash\necho hello").unwrap();

        let mut found = Vec::new();
        discover_shell_scripts(dir.path(), &mut found);
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn ignores_non_shell_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("readme.md"), "# Hello").unwrap();
        std::fs::write(dir.path().join("data.json"), "{}").unwrap();

        let mut found = Vec::new();
        discover_shell_scripts(dir.path(), &mut found);
        assert!(found.is_empty());
    }

    #[test]
    fn discovers_ps1_files() {
        let dir = tempfile::tempdir().unwrap();
        let ps1 = dir.path().join("test.ps1");
        let psm1 = dir.path().join("module.psm1");
        std::fs::write(&ps1, "Write-Host 'hi'").unwrap();
        std::fs::write(&psm1, "function Test {}").unwrap();
        std::fs::write(dir.path().join("readme.md"), "# Hello").unwrap();

        let mut found = Vec::new();
        discover_powershell_scripts(dir.path(), &mut found);
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn shebang_detects_various_shells() {
        let dir = tempfile::tempdir().unwrap();

        for (name, shebang) in [
            ("a", "#!/bin/sh\n"),
            ("b", "#!/bin/bash\n"),
            ("c", "#!/usr/bin/env zsh\n"),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, shebang).unwrap();
        }

        let mut found = Vec::new();
        discover_shell_scripts(dir.path(), &mut found);
        // zsh scripts are excluded (shellcheck doesn't support them)
        assert_eq!(found.len(), 2);
    }
}
