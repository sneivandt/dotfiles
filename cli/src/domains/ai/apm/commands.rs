//! APM process invocation and common output/error handling.

use anyhow::{Context as _, Result};

use super::targets::ApmTargets;
use crate::engine::{Context, TaskResult};

pub(super) const APM_NONINTERACTIVE_ENV: &[(&str, &str)] = &[
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GCM_INTERACTIVE", "Never"),
    ("GCM_GUI_PROMPT", "false"),
];

/// Per-package failure record emitted by the experimental `copilot-app` target
/// when it refuses to lockfile-encode a deployed primitive whose id is outside
/// apm's `apm--<owner>--<pkg>--<prompt>` workflow namespace.
///
/// This happens for `.agent.md` agent files shipped by third-party packages:
/// the primary unscoped install still deploys those agents correctly, so the
/// failure is a benign, upstream-only limitation of the experimental target. APM has
/// changed the prefix for this diagnostic across releases, so match the stable
/// refusal text and still fail closed unless the count equals APM's error total.
const APM_WORKFLOW_ENCODE_FAILURE_MARKER: &str = "Refusing to lockfile-encode non-APM workflow id";

#[derive(Debug, Clone, Copy)]
pub(super) enum ApmCommand {
    Install,
    Update,
}

impl ApmCommand {
    const fn verb(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Update => "update",
        }
    }

    const fn args(self) -> &'static [&'static str] {
        match self {
            Self::Install => ApmTargets::install_args(),
            Self::Update => ApmTargets::update_args(),
        }
    }

    fn auth_reason(self) -> String {
        format!(
            "apm {} requires GitHub authentication; run `gh auth login` or set \
             GH_TOKEN/GITHUB_TOKEN and re-run",
            self.verb()
        )
    }

    const fn error_context(self) -> &'static str {
        match self {
            Self::Install => "running apm install",
            Self::Update => "updating APM dependencies",
        }
    }
}

#[derive(Debug)]
pub(super) enum ApmCommandResult {
    Success,
    AuthSkipped(String),
    ToleratedWorkflowEncodeFailures,
}

/// Run an APM install/update command with shared environment, logging, and
/// failure classification.
///
/// The primary command deliberately omits `--target` so APM 0.25 can
/// auto-detect all installed MCP runtimes and reconcile their shared ledger
/// together. Copilot App workflows require an explicit experimental target,
/// so they are deployed by a separate install after the primary command.
///
/// # Errors
///
/// Returns an error when APM exits unsuccessfully for anything other than a
/// recognized authentication skip or the known `copilot-app` workflow-encoding
/// limitation.
pub(super) fn run_apm_command(
    ctx: &Context,
    command: ApmCommand,
    targets: ApmTargets,
) -> Result<ApmCommandResult> {
    match run_apm_invocation(ctx, command, command.args(), false)? {
        ApmCommandResult::Success => {}
        result @ (ApmCommandResult::AuthSkipped(_)
        | ApmCommandResult::ToleratedWorkflowEncodeFailures) => return Ok(result),
    }

    let Some(copilot_app_args) = targets.copilot_app_install_args() else {
        return Ok(ApmCommandResult::Success);
    };
    run_apm_invocation(ctx, ApmCommand::Install, copilot_app_args, true)
}

fn run_apm_invocation(
    ctx: &Context,
    command: ApmCommand,
    args: &[&str],
    tolerate_workflow_encode_failures: bool,
) -> Result<ApmCommandResult> {
    let cwd = ctx.home.clone();
    let rendered = args.join(" ");
    ctx.debug_fmt(|| {
        format!(
            "running `apm {rendered}` in {} (interactive credential prompts disabled)",
            cwd.display()
        )
    });

    match ctx
        .executor
        .run_in_with_env(&cwd, "apm", args, APM_NONINTERACTIVE_ENV)
    {
        Ok(result) => {
            report_apm_output(ctx, &result.stdout, &result.stderr);
            Ok(ApmCommandResult::Success)
        }
        Err(err) => classify_apm_error(ctx, command, tolerate_workflow_encode_failures, err),
    }
}

fn classify_apm_error(
    ctx: &Context,
    command: ApmCommand,
    tolerate_workflow_encode_failures: bool,
    err: anyhow::Error,
) -> Result<ApmCommandResult> {
    let msg = format!("{err:#}");
    if looks_like_auth_failure(&msg) {
        let reason = command.auth_reason();
        ctx.log
            .warn(&format!("skipping: {reason} (details: {})", msg.trim()));
        return Ok(ApmCommandResult::AuthSkipped(reason));
    }

    if let Some(count) = tolerate_workflow_encode_failures
        .then(|| tolerable_workflow_encode_failures(&msg))
        .flatten()
    {
        report_apm_output(ctx, &msg, "");
        ctx.log.info(&format!(
            "apm {} succeeded; ignoring {count} experimental copilot-app \
             workflow-encoding error(s) for non-workflow primitives (e.g. .agent.md agents). \
             Other primitives deployed normally; full apm output is in the log.",
            command.verb()
        ));
        return Ok(ApmCommandResult::ToleratedWorkflowEncodeFailures);
    }

    Err(err).context(command.error_context())
}

/// Best-effort enable of the experimental `copilot-app` apm target.
///
/// The `copilot-app` target only encodes workflow prompts once apm's
/// experimental flag is set in the machine-local `~/.apm/config.json`.  Running
/// `apm experimental enable copilot-app` here keeps fresh machines reproducible
/// without a manual step.  The command is idempotent (it reports the flag is
/// already enabled on repeat runs), so it is safe to call on every apply.
///
/// Failure is intentionally non-fatal: an older apm that predates the
/// `experimental` subcommand will error, but auto-detected runtimes and standard
/// primitives must still install. Any error is logged as a warning and swallowed.
pub(super) fn ensure_copilot_app_enabled(ctx: &Context) {
    let cwd = ctx.home.clone();
    ctx.debug_fmt(|| {
        format!(
            "running `apm experimental enable copilot-app` in {} (idempotent)",
            cwd.display()
        )
    });
    match ctx.executor.run_in_with_env(
        &cwd,
        "apm",
        &["experimental", "enable", "copilot-app"],
        APM_NONINTERACTIVE_ENV,
    ) {
        Ok(result) => report_apm_output(ctx, &result.stdout, &result.stderr),
        Err(err) => {
            let msg = format!("{err:#}");
            ctx.log.warn(&format!(
                "could not enable apm experimental copilot-app target; continuing without it \
                 (details: {})",
                msg.trim()
            ));
        }
    }
}

/// Convert a command result into the task-level result used by install.
pub(super) fn install_task_result(result: ApmCommandResult) -> TaskResult {
    match result {
        ApmCommandResult::Success | ApmCommandResult::ToleratedWorkflowEncodeFailures => {
            TaskResult::Ok
        }
        ApmCommandResult::AuthSkipped(reason) => TaskResult::Skipped(reason),
    }
}

/// Relay raw APM command output to the diagnostic log file and the verbose
/// console.
///
/// The headline state-change is emitted separately as an always-visible
/// `    {verb}: {desc}` line by the caller, so this routes APM's own
/// line-by-line chatter through `debug`: it is always captured in the log
/// file and shown under `--verbose`, but stays out of the way on ordinary
/// runs.  APM provides idempotency itself via its lockfile, so this output is
/// purely informational.
pub(super) fn report_apm_output(ctx: &Context, stdout: &str, stderr: &str) {
    for line in stdout.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() {
            continue;
        }
        ctx.log.debug(trimmed);
    }
    for line in stderr.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() {
            continue;
        }
        ctx.log.debug(trimmed);
    }
}

/// Heuristic: does an `apm install/update` failure message indicate a missing
/// or invalid GitHub credential rather than a real installation error?
pub(super) fn looks_like_auth_failure(message: &str) -> bool {
    let lowered = message.to_lowercase();
    [
        "authentication failed",
        "authentication required",
        "bad credentials",
        "could not read username",
        "could not read password",
        "fatal: authentication failed",
        "requires authentication",
        "terminal prompts disabled",
        "401 unauthorized",
        "403 forbidden",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

/// Decide whether an `apm install/update` failure is solely the experimental
/// `copilot-app` target refusing to lockfile-encode non-workflow primitives,
/// returning the count of such failures when so.
///
/// To avoid ever masking a genuine failure this fails closed: it returns
/// `Some(n)` only when apm's own reported error total parses *and* exactly
/// equals the number of workflow-encoding failure records.  Any unparseable
/// summary, or any additional error of a different kind, yields `None` so the
/// failure propagates normally.
pub(super) fn tolerable_workflow_encode_failures(message: &str) -> Option<usize> {
    let normalized = normalize_apm_output(message);
    let encode_failures = normalized
        .matches(APM_WORKFLOW_ENCODE_FAILURE_MARKER)
        .count();
    if encode_failures == 0 {
        return None;
    }
    match parse_apm_error_count(&normalized) {
        Some(total) if total == encode_failures => Some(encode_failures),
        _ => None,
    }
}

/// Collapse every run of whitespace (including newlines) to a single space so
/// console line-wrapping in captured apm output cannot split a marker phrase.
fn normalize_apm_output(message: &str) -> String {
    message.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Parse the total error count from apm's summary line, e.g. the `7` in
/// `... with 7 error(s).`.  Returns `None` when no digit-prefixed ` error`
/// token is present so callers fail closed on unexpected output.
fn parse_apm_error_count(normalized: &str) -> Option<usize> {
    let idx = normalized.find(" error")?;
    let digits: String = normalized
        .get(..idx)?
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<Vec<char>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse().ok()
}
