//! Outcome parsing and reporting for APM workflow autopilot fixups.

use crate::engine::Context;

use super::DesiredApmWorkflows;
use super::scripts::parse_autopilot_result;

/// Report the outcome of a successful autopilot-fixup script run.
///
/// Only [`FixupOutcome::Set`] produces a console line; the other outcomes are
/// steady-state or non-actionable and stay at debug level to keep idempotent
/// runs quiet.
pub(super) fn report_fixup_outcome(ctx: &Context, outcome: FixupOutcome, stdout: &str) {
    match outcome {
        FixupOutcome::NoWorkflows => {
            // The lockfile listed dotfiles-managed workflows but none of them
            // are present in `~/.copilot/data.db` yet. This is the normal state
            // until the Copilot App has run discovery for the workflows'
            // project, so keep it out of the console and just log it.
            ctx.debug_fmt(|| {
                "autopilot fixup: dotfiles-managed workflows are listed in \
                 ~/.apm/apm.lock.yaml but none were found in ~/.copilot/data.db yet"
                    .to_string()
            });
        }
        FixupOutcome::Set(n) => {
            ctx.log.always(&format!(
                "    workflows: set {n} apm workflow(s) to autopilot + enabled"
            ));
        }
        FixupOutcome::Quiet => {
            ctx.debug_fmt(|| {
                "autopilot fixup: apm workflows already autopilot + enabled (no change)".to_string()
            });
        }
        FixupOutcome::Unparsed => {
            ctx.debug_fmt(|| {
                format!(
                    "autopilot fixup: could not parse script output (continuing): {}",
                    stdout.trim()
                )
            });
        }
    }
}

/// What the post-install autopilot fixup should report, derived purely from the
/// script output and the pre-install snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FixupOutcome {
    /// None of the requested dotfiles-managed workflow ids matched a row in
    /// `~/.copilot/data.db` (the lockfile listed workflows, but the App has not
    /// recorded them yet). Logged at debug rather than warned.
    NoWorkflows,
    /// `n` workflows newly reached the desired state; announce it.
    Set(usize),
    /// Nothing meaningfully changed (or the change cannot be substantiated);
    /// stay quiet.
    Quiet,
    /// The script output could not be parsed; log at debug and continue.
    Unparsed,
}

/// Decide what the fixup should report by diffing the post-install desired set
/// against the pre-install snapshot.
///
/// This is the suppression core: in the steady state APM resets every workflow
/// secure-by-default and the fixup restores them, so the post set equals the
/// pre set and the delta is zero -- [`FixupOutcome::Quiet`]. A non-zero delta
/// means a workflow genuinely transitioned (first install, a newly added
/// workflow, or a user-disabled one being re-enabled).
pub(super) fn decide_fixup_outcome(stdout: &str, pre: &DesiredApmWorkflows) -> FixupOutcome {
    let Some((matched, post_ids)) = parse_autopilot_result(stdout) else {
        return FixupOutcome::Unparsed;
    };
    if matched == 0 {
        return FixupOutcome::NoWorkflows;
    }
    let delta = match pre {
        DesiredApmWorkflows::Known(pre_ids) => post_ids.difference(pre_ids).count(),
        DesiredApmWorkflows::FirstInstall => post_ids.len(),
        DesiredApmWorkflows::Unavailable => return FixupOutcome::Quiet,
    };
    if delta > 0 {
        FixupOutcome::Set(delta)
    } else {
        FixupOutcome::Quiet
    }
}
