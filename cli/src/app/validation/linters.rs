//! Shared script invocation and output handling for validation linters.

use crate::engine::{Context, TaskResult};
use crate::infra::exec::{CommandSpec, ExecResult};
use crate::infra::logging::{Log, OutputExt as _};
use anyhow::Result;

pub(super) const SHELLCHECK_SCRIPT: &str = include_str!("scripts/shellcheck.sh");
pub(super) const PSSCRIPTANALYZER_SCRIPT: &str = include_str!("scripts/psscriptanalyzer.ps1");

pub(super) fn log_exec_output(log: &dyn Log, result: &ExecResult) {
    for line in result.stdout.lines().chain(result.stderr.lines()) {
        log.error(line);
    }
}

pub(super) fn run_linter(ctx: &Context, name: &str, spec: CommandSpec) -> Result<TaskResult> {
    let result = ctx.executor().execute(spec.unchecked())?;
    if result.success {
        Ok(TaskResult::CheckPassed)
    } else {
        log_exec_output(ctx.log(), &result);
        anyhow::bail!("{name} found issues");
    }
}

pub(super) fn build_psscriptanalyzer_command(root: &std::path::Path) -> String {
    let root = root.to_string_lossy().replace('\'', "''");
    format!("& {{ {PSSCRIPTANALYZER_SCRIPT} }} -Root '{root}'")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    fn inputs(root: &Path, cases: &[(&str, &str, bool)]) -> Vec<String> {
        std::fs::create_dir_all(root.join("hooks/nested")).unwrap();
        let mut expected = Vec::new();
        for &(name, contents, accepted) in cases {
            std::fs::write(root.join("hooks/nested").join(name), contents).unwrap();
            if accepted {
                expected.push(name.to_owned());
            }
        }
        expected.sort();
        expected
    }

    #[cfg(unix)]
    fn add_recipe_inputs(root: &Path) {
        let recipe = root.join("packages/recipe with spaces");
        std::fs::create_dir_all(recipe.join("src")).unwrap();
        std::fs::write(recipe.join("PKGBUILD"), "# shellcheck shell=bash\n").unwrap();
        std::fs::write(recipe.join("version.sh"), "generated pins").unwrap();
        std::fs::write(recipe.join("src/ignored.sh"), "generated source").unwrap();
    }

    fn recorded(log: &Path) -> Vec<String> {
        let mut names: Vec<_> = std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect();
        names.sort();
        names
    }

    #[cfg(unix)]
    #[test]
    fn shared_shell_discovery_and_failures() {
        use std::os::unix::fs::{PermissionsExt as _, symlink};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root o'connor [literal]");
        let mut expected = inputs(
            &root,
            &[
                ("plain.sh", "echo hi", true),
                ("space name.sh", "echo hi", true),
                ("glob[name].sh", "echo hi", true),
                ("sh", "#!/bin/sh -e\n", true),
                ("bash", "#!/usr/local/bin/bash -x\n", true),
                ("ksh", "#!/bin/ksh\n", true),
                ("env", "#!/usr/bin/env -S dash -e\n", true),
                ("exe", "#!/usr/bin/env.exe bash.exe\n", true),
                ("tabs", "#!\t/usr/bin/env\tbash\n", true),
                ("no newline", "#!/bin/bash", true),
                ("skip.zsh", "#!/bin/bash\n", false),
                ("zsh", "#!/usr/bin/env zsh\n", false),
                ("fish", "#!/bin/fish\n", false),
                ("csh", "#!/bin/csh\n", false),
                ("tcsh", "#!/bin/tcsh\n", false),
                ("python", "#!/bin/python3\n", false),
                ("suffix", "#!/bin/notbash\n", false),
                ("env missing", "#!/usr/bin/env -S\n", false),
                ("empty", "", false),
                ("not first line", "\n#!/bin/bash\n", false),
            ],
        );
        add_recipe_inputs(&root);
        expected.push("PKGBUILD".into());
        expected.sort();
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let mock = bin.join("shellcheck");
        std::fs::write(
            &mock,
            r#"#!/bin/sh
set -eu
for file do
  case "$file" in --*) continue ;; esac
  basename "$file" >> "$LINT_INPUT_LOG"
done
exit "$LINT_EXIT"
"#,
        )
        .unwrap();
        std::fs::set_permissions(&mock, std::fs::Permissions::from_mode(0o755)).unwrap();
        let log = dir.path().join("inputs");
        let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
        let run = |input_root: &Path, exit: &str| {
            Command::new("sh")
                .args(["-c", SHELLCHECK_SCRIPT, "fixture", "--root"])
                .arg(input_root)
                .env("PATH", &path)
                .env("LINT_INPUT_LOG", &log)
                .env("LINT_EXIT", exit)
                .output()
                .unwrap()
        };
        let output = run(&root, "0");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(recorded(&log), expected);
        std::fs::remove_file(&log).unwrap();
        let selected = root.join("hooks/nested/space name.sh");
        let selected_output = Command::new("sh")
            .args(["-c", SHELLCHECK_SCRIPT, "fixture"])
            .arg(&selected)
            .env("PATH", &path)
            .env("LINT_INPUT_LOG", &log)
            .env("LINT_EXIT", "0")
            .output()
            .unwrap();
        assert!(selected_output.status.success());
        assert_eq!(recorded(&log), ["space name.sh"]);
        assert!(
            !run(&root, "1").status.success(),
            "linter findings must fail"
        );
        symlink("missing", root.join("hooks/broken")).unwrap();
        assert!(
            !run(&root, "0").status.success(),
            "broken inputs must fail discovery"
        );
        std::fs::remove_dir_all(root.join("hooks")).unwrap();
        std::fs::write(root.join("hooks"), "not a directory").unwrap();
        assert!(
            !run(&root, "0").status.success(),
            "non-directory inputs must fail"
        );
        std::fs::remove_file(root.join("hooks")).unwrap();
        std::fs::remove_dir_all(root.join("packages")).unwrap();
        assert!(
            run(&root, "1").status.success(),
            "empty discovery must not invoke the tool"
        );
    }

    fn write_fake_psscriptanalyzer_module(dir: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
        let modules = dir.join("modules");
        let module = modules.join("PSScriptAnalyzer");
        std::fs::create_dir_all(&module).unwrap();
        std::fs::write(
            module.join("PSScriptAnalyzer.psm1"),
            r#"
function Invoke-ScriptAnalyzer {
    param([string]$Path, [string[]]$Severity)
    if (($Severity -join ',') -ne 'Warning,Error') { throw 'wrong severity' }
    [IO.File]::AppendAllText($env:LINT_INPUT_LOG, [IO.Path]::GetFileName($Path) + "`n")
    if ($env:LINT_EXIT -eq '1') { 'fixture finding' }
    if ($env:LINT_EXIT -eq '2') { throw 'fixture analyzer failure' }
}
Export-ModuleMember -Function Invoke-ScriptAnalyzer
"#,
        )
        .unwrap();
        (modules, module)
    }

    #[test]
    fn shared_powershell_discovery_and_failures() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root o'connor [literal]");
        let expected = inputs(
            &root,
            &[
                ("space name.ps1", "Write-Host hi", true),
                ("glob[name].psm1", "function Test {}", true),
                ("manifest.psd1", "@{}", true),
                ("pwsh", "#!/usr/bin/env -S pwsh -NoProfile\n", true),
                ("powershell", "#!/usr/bin/powershell.exe\n", true),
                ("tabs", "#!\t/usr/bin/env.exe\tpwsh.exe\n", true),
                ("no newline", "#!/usr/bin/pwsh", true),
                ("shell", "#!/bin/sh\n", false),
                ("suffix", "#!/bin/notpwsh\n", false),
                ("empty", "", false),
                ("not first line", "\n#!/usr/bin/pwsh\n", false),
            ],
        );
        let (modules, module) = write_fake_psscriptanalyzer_module(dir.path());
        let log_all = dir.path().join("inputs-all");
        let log_selected = dir.path().join("inputs-selected");
        let log_other = dir.path().join("inputs-other");
        let cmd_script = dir.path().join("cmd.ps1");
        std::fs::write(&cmd_script, build_psscriptanalyzer_command(&root)).unwrap();
        let analyzer_script = dir.path().join("analyzer.ps1");
        std::fs::write(&analyzer_script, PSSCRIPTANALYZER_SCRIPT).unwrap();
        let selected = root.join("hooks/nested/space name.ps1");
        let hooks = root.join("hooks");
        let ps = |path: &Path| path.to_string_lossy().replace('\'', "''");
        let driver = format!(
            r#"$env:PSModulePath = '{modules}' + [IO.Path]::PathSeparator + (Join-Path $PSHOME 'Modules')
function Invoke-Case([string]$Name, [scriptblock]$Body) {{
    $global:LASTEXITCODE = 0
    $failed = $false
    try {{
        $null = & $Body
        if ($LASTEXITCODE -ne 0) {{ $failed = $true }}
    }} catch {{
        $failed = $true
    }}
    "$Name=$(if ($failed) {{ 'fail' }} else {{ 'ok' }})"
}}
Invoke-Case 'ok' {{ $env:LINT_INPUT_LOG = '{log_all}'; $env:LINT_EXIT = '0'; & '{cmd}' }}
Invoke-Case 'explicit' {{ $env:LINT_INPUT_LOG = '{log_selected}'; $env:LINT_EXIT = '0'; & '{analyzer}' '{selected}' }}
Invoke-Case 'finding' {{ $env:LINT_INPUT_LOG = '{log_other}'; $env:LINT_EXIT = '1'; & '{cmd}' }}
Invoke-Case 'throw' {{ $env:LINT_INPUT_LOG = '{log_other}'; $env:LINT_EXIT = '2'; & '{cmd}' }}
Remove-Module PSScriptAnalyzer -ErrorAction SilentlyContinue
Remove-Item -LiteralPath '{module}' -Recurse -Force
Invoke-Case 'missing_module' {{ $env:LINT_EXIT = '0'; & '{cmd}' }}
Remove-Item -LiteralPath '{hooks}' -Recurse -Force
Set-Content -LiteralPath '{hooks}' -Value 'not a directory'
Invoke-Case 'invalid_dir' {{ $env:LINT_EXIT = '0'; & '{cmd}' }}
Remove-Item -LiteralPath '{hooks}' -Force
Invoke-Case 'empty' {{ $env:LINT_EXIT = '1'; & '{cmd}' }}
"#,
            modules = ps(&modules),
            log_all = ps(&log_all),
            log_selected = ps(&log_selected),
            log_other = ps(&log_other),
            cmd = ps(&cmd_script),
            analyzer = ps(&analyzer_script),
            selected = ps(&selected),
            module = ps(&module),
            hooks = ps(&hooks),
        );
        let output = match Command::new("pwsh")
            .args(["-NoProfile", "-Command", &driver])
            .env("PSModulePath", &modules)
            .output()
        {
            Ok(output) => output,
            Err(error) => {
                assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
                return;
            }
        };
        assert!(
            output.status.success(),
            "driver failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let results: Vec<&str> = stdout.lines().collect();
        assert_eq!(
            results,
            [
                "ok=ok",
                "explicit=ok",
                "finding=fail",
                "throw=fail",
                "missing_module=fail",
                "invalid_dir=fail",
                "empty=ok",
            ],
            "unexpected case outcomes: {stdout}"
        );
        assert_eq!(recorded(&log_all), expected);
        assert_eq!(recorded(&log_selected), ["space name.ps1"]);
    }
}
