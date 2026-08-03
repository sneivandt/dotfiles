//! APM process invocation and common output/error handling.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

use super::targets::ApmTargets;
use crate::engine::{Context, TaskResult};
use crate::infra::logging::OutputExt as _;

pub(super) const APM_NONINTERACTIVE_ENV: &[(&str, &str)] = &[
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GCM_INTERACTIVE", "Never"),
    ("GCM_GUI_PROMPT", "false"),
];

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
}

#[derive(Debug)]
pub(super) enum ApmOutdatedResult {
    Outdated,
    Current,
    Unknown,
    AuthSkipped(String),
}

/// Run an APM install/update command with shared environment, logging, and
/// failure classification.
///
/// The primary command deliberately omits `--target` so APM resolves the
/// runtime set itself and reconciles their shared ledger together. APM resolves
/// runtimes as `--target` > `targets:` in the merged manifest > `apm config
/// target` > auto-detect, so `symlinks/apm/config/base.yml` pins the runtime set
/// without this call having to enumerate it. Copilot App workflows require an
/// explicit experimental target, so they are deployed by a separate install
/// after the primary command.
///
/// # Errors
///
/// Returns an error when APM exits unsuccessfully for anything other than a
/// recognized authentication skip.
pub(super) fn run_apm_command(
    ctx: &Context,
    command: ApmCommand,
    targets: ApmTargets,
) -> Result<ApmCommandResult> {
    match run_apm_invocation(ctx, command, command.args())? {
        ApmCommandResult::Success => {}
        result @ ApmCommandResult::AuthSkipped(_) => return Ok(result),
    }

    let Some(copilot_app_args) = targets.copilot_app_install_args() else {
        return Ok(ApmCommandResult::Success);
    };
    run_apm_invocation(ctx, ApmCommand::Install, copilot_app_args)
}

/// Check locked user-scope dependencies for remote updates without mutating
/// the manifest, lockfile, or deployed primitives.
pub(super) fn check_apm_outdated(ctx: &Context) -> Result<ApmOutdatedResult> {
    let system = ctx.system();
    let cwd = system.home();
    let args = ["outdated", "-g"];
    ctx.debug_fmt(|| {
        format!(
            "running `apm {}` in {} (interactive credential prompts disabled)",
            args.join(" "),
            cwd.display()
        )
    });

    match system
        .executor()
        .run_in_with_env(cwd, "apm", &args, APM_NONINTERACTIVE_ENV)
    {
        Ok(result) => {
            report_apm_output(ctx, &result.stdout, &result.stderr);
            let output = format!("{}\n{}", result.stdout, result.stderr);
            if output.lines().any(line_reports_outdated_dependencies) {
                Ok(ApmOutdatedResult::Outdated)
            } else if output.lines().any(line_reports_current_dependencies) {
                Ok(ApmOutdatedResult::Current)
            } else {
                Ok(ApmOutdatedResult::Unknown)
            }
        }
        Err(err) => {
            let msg = format!("{err:#}");
            if looks_like_auth_failure(&msg) {
                let reason = ApmCommand::Update.auth_reason();
                ctx.log()
                    .warn(format!("skipping: {reason} (details: {})", msg.trim()));
                return Ok(ApmOutdatedResult::AuthSkipped(reason));
            }
            Err(err).context("checking for outdated APM dependencies")
        }
    }
}

fn line_reports_outdated_dependencies(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    line.contains(" outdated dependency found") || line.contains(" outdated dependencies found")
}

fn line_reports_current_dependencies(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    line.contains("all dependencies are up-to-date")
        || line.contains("no locked dependencies to check")
        || line.contains("no remote dependencies to check")
}

fn run_apm_invocation(
    ctx: &Context,
    command: ApmCommand,
    args: &[&str],
) -> Result<ApmCommandResult> {
    let system = ctx.system();
    let cwd = system.home();
    let rendered = args.join(" ");
    ctx.debug_fmt(|| {
        format!(
            "running `apm {rendered}` in {} (interactive credential prompts disabled)",
            cwd.display()
        )
    });

    match system
        .executor()
        .run_in_with_env(cwd, "apm", args, APM_NONINTERACTIVE_ENV)
    {
        Ok(result) => {
            report_apm_output(ctx, &result.stdout, &result.stderr);
            Ok(ApmCommandResult::Success)
        }
        Err(err) => classify_apm_error(ctx, command, err),
    }
}

fn classify_apm_error(
    ctx: &Context,
    command: ApmCommand,
    err: anyhow::Error,
) -> Result<ApmCommandResult> {
    let msg = format!("{err:#}");
    if looks_like_auth_failure(&msg) {
        let reason = command.auth_reason();
        ctx.log()
            .warn(format!("skipping: {reason} (details: {})", msg.trim()));
        return Ok(ApmCommandResult::AuthSkipped(reason));
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
/// `experimental` subcommand will error, but the manifest-resolved runtimes and
/// standard primitives must still install. Any error is logged as a warning and
/// swallowed.
pub(super) fn ensure_copilot_app_enabled(ctx: &Context) {
    let system = ctx.system();
    let cwd = system.home();

    // The CLI call costs a full apm process start (~1.3s) purely to re-assert a
    // flag that is almost always already set.  Reading the config apm itself
    // writes answers the same question for free; anything ambiguous falls
    // through to the authoritative idempotent command.
    if copilot_app_experimental_enabled(cwd) == Some(true) {
        ctx.debug_fmt(|| {
            format!(
                "apm experimental copilot-app already enabled in {}; skipping `apm experimental \
                 enable`",
                apm_config_path(cwd).display()
            )
        });
        return;
    }

    ctx.debug_fmt(|| {
        format!(
            "running `apm experimental enable copilot-app` in {} (idempotent)",
            cwd.display()
        )
    });
    match system.executor().run_in_with_env(
        cwd,
        "apm",
        &["experimental", "enable", "copilot-app"],
        APM_NONINTERACTIVE_ENV,
    ) {
        Ok(result) => report_apm_output(ctx, &result.stdout, &result.stderr),
        Err(err) => {
            let msg = format!("{err:#}");
            ctx.log().warn(format!(
                "could not enable apm experimental copilot-app target; continuing without it \
                 (details: {})",
                msg.trim()
            ));
        }
    }
}

/// Path to the machine-local apm configuration file.
fn apm_config_path(home: &Path) -> PathBuf {
    home.join(".apm").join("config.json")
}

/// Read the `copilot_app` experimental flag out of apm's own config.
///
/// Returns `None` whenever the answer cannot be established — the file is
/// missing, unreadable, not JSON, or shaped differently than expected — so
/// callers treat "unknown" as "ask apm", never as "already enabled".
///
/// Note the spelling difference: the config key is the snake-case
/// `copilot_app` while the CLI argument is the kebab-case `copilot-app`.
fn copilot_app_experimental_enabled(home: &Path) -> Option<bool> {
    let raw = std::fs::read_to_string(apm_config_path(home)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    value.get("experimental")?.get("copilot_app")?.as_bool()
}

/// Convert a command result into the task-level result used by install.
pub(super) fn install_task_result(result: ApmCommandResult) -> TaskResult {
    match result {
        ApmCommandResult::Success => TaskResult::Ok,
        ApmCommandResult::AuthSkipped(reason) => TaskResult::Skipped(reason),
    }
}

/// Remove user-scope deployments that are no longer owned by the manifest.
///
/// `apm prune` has no global flag; running it from `~/.apm` selects the
/// user-scope manifest and lockfile.
pub(super) fn prune_user_scope(ctx: &Context) -> Result<()> {
    let cwd = ctx.system().home().join(".apm");
    ctx.debug_fmt(|| format!("running `apm prune` in {}", cwd.display()));
    let result = ctx
        .system()
        .executor()
        .run_in_with_env(&cwd, "apm", &["prune"], APM_NONINTERACTIVE_ENV)
        .context("pruning unowned user-scope APM deployments")?;
    report_apm_output(ctx, &result.stdout, &result.stderr);
    Ok(())
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
        ctx.log().debug(trimmed);
    }
    for line in stderr.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() {
            continue;
        }
        ctx.log().debug(trimmed);
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
