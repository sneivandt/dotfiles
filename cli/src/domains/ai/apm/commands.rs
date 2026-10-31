//! APM process invocation and common output/error handling.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::engine::Context;
use crate::infra::exec::{CommandSpec, ExecError, ExecResult};
use crate::infra::logging::OutputExt as _;
use anyhow::{Context as _, Result};

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

    pub(super) const fn args(self) -> &'static [&'static str] {
        match self {
            Self::Install => &["install", "-g"],
            Self::Update => &["update", "-g", "--yes"],
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
            Self::Update => "updating agent packages",
        }
    }
}

#[derive(Debug)]
pub(super) enum ApmCommandResult {
    Success(ExecResult),
    AuthSkipped(String),
}

pub(super) fn run_apm_invocation(
    ctx: &Context,
    command: ApmCommand,
    args: &[&str],
) -> Result<ApmCommandResult> {
    let cwd = ctx.home();
    let rendered = args.join(" ");
    ctx.debug_fmt(|| {
        format!(
            "running `apm {rendered}` in {} (interactive credential prompts disabled)",
            cwd.display()
        )
    });

    match ctx.executor().execute(
        CommandSpec::new("apm")
            .args(args)
            .current_dir(cwd)
            .envs(APM_NONINTERACTIVE_ENV)
            .timeout(Duration::from_mins(15)),
    ) {
        Ok(result) => {
            report_apm_output(ctx, &result.stdout, &result.stderr);
            Ok(ApmCommandResult::Success(result))
        }
        Err(err) => classify_apm_error(ctx, command, err),
    }
}

fn classify_apm_error(
    ctx: &Context,
    command: ApmCommand,
    err: ExecError,
) -> Result<ApmCommandResult> {
    if !matches!(err, ExecError::NonZero { .. }) {
        return Err(err).context(command.error_context());
    }
    let msg = format!("{err:#}");
    if looks_like_auth_failure(&msg) {
        let reason = command.auth_reason();
        ctx.log()
            .warn(format!("skipping: {reason} (details: {})", msg.trim()));
        return Ok(ApmCommandResult::AuthSkipped(reason));
    }

    Err(err).context(command.error_context())
}

/// Best-effort enable of an experimental APM deployment target.
///
/// Cancellation must stop deployment even though other enablement failures
/// remain best-effort for compatibility with older APM versions.
pub(super) fn ensure_experimental_target_enabled(
    ctx: &Context,
    target: &str,
    config_key: &str,
) -> Result<()> {
    let cwd = ctx.home();

    // The CLI call costs a full apm process start (~1.3s) purely to re-assert a
    // flag that is almost always already set.  Reading the config apm itself
    // writes answers the same question for free; anything ambiguous falls
    // through to the authoritative idempotent command.
    if experimental_target_enabled(cwd, config_key) == Some(true) {
        ctx.debug_fmt(|| {
            format!(
                "apm experimental {target} already enabled in {}; skipping `apm experimental enable`",
                apm_config_path(cwd).display()
            )
        });
        return Ok(());
    }

    ctx.debug_fmt(|| {
        format!(
            "running `apm experimental enable {target}` in {} (idempotent)",
            cwd.display()
        )
    });
    match ctx.executor().execute(
        CommandSpec::new("apm")
            .args(&["experimental", "enable", target])
            .current_dir(cwd)
            .envs(APM_NONINTERACTIVE_ENV)
            .timeout(Duration::from_mins(2)),
    ) {
        Ok(result) => report_apm_output(ctx, &result.stdout, &result.stderr),
        Err(err) if err.is_cancelled() => {
            return Err(err).with_context(|| format!("enabling apm experimental {target}"));
        }
        Err(err) => {
            let msg = format!("{err:#}");
            ctx.log().warn(format!(
                "could not enable apm experimental {target} target; continuing without it \
                 (details: {})",
                msg.trim()
            ));
        }
    }
    Ok(())
}

/// Path to the machine-local apm configuration file.
fn apm_config_path(home: &Path) -> PathBuf {
    home.join(".apm").join("config.json")
}

/// Read an experimental flag out of apm's own config.
///
/// Returns `None` whenever the answer cannot be established — the file is
/// missing, unreadable, not JSON, or shaped differently than expected — so
/// callers treat "unknown" as "ask apm", never as "already enabled".
///
/// APM stores flag keys in snake case while CLI target names use kebab case.
fn experimental_target_enabled(home: &Path, config_key: &str) -> Option<bool> {
    let raw = std::fs::read_to_string(apm_config_path(home)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    value.get("experimental")?.get(config_key)?.as_bool()
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
    for line in stdout.lines().chain(stderr.lines()) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::ai::apm::test_fixture::{assert_apm_command, make_context_with_home};
    use crate::infra::exec::MockExecutor;
    use crate::infra::platform::{Os, Platform};

    #[test]
    fn native_execution_failures_are_not_classified_by_auth_text() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let ctx = make_context_with_home(
            dir.path(),
            Platform::new(Os::Linux, false),
            MockExecutor::new(),
        );
        let output = || ExecResult::failure("", "authentication required", None);
        for error in [
            ExecError::TimedOut {
                command: "apm".into(),
                timeout: Duration::from_secs(1),
                result: output(),
            },
            ExecError::OutputLimit {
                command: "apm".into(),
                stream: "stderr",
                limit: 20,
                result: output(),
            },
            ExecError::spawn("apm", std::io::Error::other("authentication required")),
            ExecError::Io {
                command: "apm".into(),
                operation: "reading output",
                source: std::io::Error::other("authentication required"),
            },
        ] {
            let expected = std::mem::discriminant(&error);
            let error = classify_apm_error(&ctx, ApmCommand::Install, error).unwrap_err();
            assert_eq!(
                std::mem::discriminant(error.downcast_ref::<ExecError>().unwrap()),
                expected,
                "{error:#}"
            );
        }
    }

    #[test]
    fn only_a_true_flag_for_the_requested_target_skips_experimental_enablement() {
        for (target, config_key, other_key) in [
            ("copilot-app", "copilot_app", "copilot_cowork"),
            ("copilot-cowork", "copilot_cowork", "copilot_app"),
        ] {
            for (case, contents, enabled) in [
                ("missing", None, false),
                ("invalid-json", Some("not json".to_string()), false),
                ("missing-flag", Some("{}".to_string()), false),
                (
                    "wrong-shape",
                    Some(r#"{"experimental":true}"#.to_string()),
                    false,
                ),
                (
                    "false",
                    Some(format!(r#"{{"experimental":{{"{config_key}":false}}}}"#)),
                    false,
                ),
                (
                    "string",
                    Some(format!(r#"{{"experimental":{{"{config_key}":"true"}}}}"#)),
                    false,
                ),
                (
                    "other-target",
                    Some(format!(r#"{{"experimental":{{"{other_key}":true}}}}"#)),
                    false,
                ),
                (
                    "enabled",
                    Some(format!(r#"{{"experimental":{{"{config_key}":true}}}}"#)),
                    true,
                ),
            ] {
                let dir = tempfile::tempdir_in(".").unwrap();
                let path = apm_config_path(dir.path());
                if let Some(contents) = &contents {
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(&path, contents).unwrap();
                }
                let mut mock = MockExecutor::new();
                if !enabled {
                    let home = dir.path().to_path_buf();
                    mock.expect_execute().once().returning(move |spec| {
                        assert_apm_command(&spec);
                        assert_eq!(spec.arguments(), ["experimental", "enable", target]);
                        assert_eq!(spec.working_dir(), Some(home.as_path()));
                        Ok(ExecResult::success("enabled"))
                    });
                }
                let ctx = make_context_with_home(dir.path(), Platform::new(Os::Linux, false), mock);

                ensure_experimental_target_enabled(&ctx, target, config_key)
                    .unwrap_or_else(|error| panic!("{target}/{case}: {error:#}"));

                if let Some(contents) = contents {
                    assert_eq!(std::fs::read(&path).unwrap(), contents.as_bytes(), "{case}");
                } else {
                    assert!(!path.parent().unwrap().exists());
                }
            }
        }
    }

    #[test]
    fn non_cancelled_target_enablement_failure_remains_best_effort() {
        for target in ["copilot-app", "copilot-cowork"] {
            let dir = tempfile::tempdir_in(".").unwrap();
            let mut mock = MockExecutor::new();
            mock.expect_execute().once().returning(move |spec| {
                assert_apm_command(&spec);
                assert_eq!(spec.arguments(), ["experimental", "enable", target]);
                Err(ExecError::non_zero(
                    "apm",
                    ExecResult::failure("", "unsupported experimental target", Some(1)),
                ))
            });
            let ctx = make_context_with_home(dir.path(), Platform::new(Os::Linux, false), mock);

            ensure_experimental_target_enabled(&ctx, target, "fixture").unwrap();
        }
    }
}
