//! Command construction and output handling for validation linters.

use std::path::PathBuf;

use anyhow::Result;

use crate::engine::{Context, TaskResult};
use crate::infra::exec::CommandSpec;
use crate::infra::exec::ExecResult;
use crate::infra::logging::Log;
use crate::infra::logging::OutputExt as _;

const SHELLCHECK_SEVERITY_ARG: &str = "--severity=warning";
const SHELLCHECK_ENABLE_ARG: &str = "--enable=avoid-nullary-conditions";
const SHELLCHECK_EXCLUDE_CODES: &str = "SC1090,SC1091,SC3043,SC2154";

pub(super) fn log_exec_output(log: &dyn Log, result: &ExecResult) {
    for line in result.stdout.lines().chain(result.stderr.lines()) {
        log.error(line);
    }
}

/// Run an external linter over `files` and turn its exit status into a result.
///
/// Reports a passing check when there is nothing to lint or the tool exits
/// successfully; otherwise mirrors the tool output as errors and fails the
/// task.  `exe` is the executable to spawn, `name` is how the linter is named
/// in log messages, and `label` is the plural noun for its inputs.
pub(super) fn run_linter(
    ctx: &Context,
    exe: &str,
    name: &str,
    label: &str,
    files: &[PathBuf],
    build_args: impl FnOnce(&[PathBuf]) -> Vec<String>,
) -> Result<TaskResult> {
    if files.is_empty() {
        ctx.log().info(format!("no {label} found"));
        return Ok(TaskResult::CheckPassed);
    }

    ctx.log().debug(format!("checking {} {label}", files.len()));

    let args = build_args(files);
    let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    let result = ctx
        .executor()
        .execute(CommandSpec::new(exe).args(&arg_refs).unchecked())?;
    if result.success {
        ctx.log().info(format!("{name} passed"));
        Ok(TaskResult::CheckPassed)
    } else {
        log_exec_output(ctx.log(), &result);
        anyhow::bail!("{name} found issues");
    }
}

pub(crate) fn build_psscriptanalyzer_command(paths: &[PathBuf]) -> String {
    let path_literals = paths
        .iter()
        .map(|path| powershell_single_quote(&path.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "$ErrorActionPreference = 'Stop'; \
         $paths = @({path_literals}); \
         if (!(Get-Module -ListAvailable PSScriptAnalyzer)) \
         {{ throw 'PSScriptAnalyzer module is not installed' }}; \
         Import-Module PSScriptAnalyzer -Force -ErrorAction Stop; \
         $results = $paths | ForEach-Object \
         {{ Invoke-ScriptAnalyzer -Path $_ -Severity Warning,Error -ErrorAction Stop }}; \
         if ($results.Count -gt 0) {{ $results | Format-Table -AutoSize; exit 1 }} \
         else {{ exit 0 }}"
    )
}

fn powershell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub(crate) fn build_shellcheck_args(paths: &[PathBuf]) -> Vec<String> {
    let mut args = vec![
        SHELLCHECK_SEVERITY_ARG.to_string(),
        format!("--exclude={SHELLCHECK_EXCLUDE_CODES}"),
        SHELLCHECK_ENABLE_ARG.to_string(),
    ];
    args.extend(paths.iter().map(|path| path.to_string_lossy().into_owned()));
    args
}
