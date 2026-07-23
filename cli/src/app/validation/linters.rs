//! Command construction and output handling for validation linters.

use std::path::PathBuf;

use crate::infra::exec::ExecResult;
use crate::infra::logging::Log;

const SHELLCHECK_SEVERITY_ARG: &str = "--severity=warning";
const SHELLCHECK_ENABLE_ARG: &str = "--enable=avoid-nullary-conditions";
const SHELLCHECK_EXCLUDE_CODES: &str = "SC1090,SC1091,SC3043,SC2154";

pub(super) fn log_exec_output(log: &dyn Log, result: &ExecResult) {
    for line in result.stdout.lines().chain(result.stderr.lines()) {
        log.error(line);
    }
}

pub(crate) fn build_psscriptanalyzer_command(paths: &[PathBuf]) -> String {
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

pub(crate) fn build_shellcheck_args(paths: &[PathBuf]) -> Vec<String> {
    let mut args = vec![
        SHELLCHECK_SEVERITY_ARG.to_string(),
        format!("--exclude={SHELLCHECK_EXCLUDE_CODES}"),
        SHELLCHECK_ENABLE_ARG.to_string(),
    ];
    args.extend(paths.iter().map(|path| path.to_string_lossy().into_owned()));
    args
}
