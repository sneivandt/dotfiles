//! Ctrl-C handling: cooperative cancellation, then a confirmed force quit.
//!
//! Interrupts escalate rather than repeat. The first one flips the cancellation
//! token so the engine stops dispatching work and lets in-flight operations
//! finish, and it is reported exactly once no matter how long that takes. A
//! second one means the user is no longer willing to wait, so it asks whether
//! to abandon the run; a further interrupt while that question is open quits
//! immediately, which is also what happens when nothing can answer it.
//!
//! Force quitting skips the cooperative shutdown that normally terminates
//! spawned children, so it is deliberately gated behind that confirmation.

use std::io::{IsTerminal as _, Write as _};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::engine::CancellationToken;
use crate::infra::elevation;
use crate::infra::logging::{Output, OutputExt as _};

/// Exit code reported for a force quit (`128 + SIGINT`).
const FORCE_QUIT_EXIT_CODE: i32 = 130;

/// No interrupt has been received yet.
const STAGE_RUNNING: u8 = 0;
/// Cooperative cancellation has been requested.
const STAGE_CANCELLING: u8 = 1;
/// A force-quit confirmation is pending.
const STAGE_CONFIRMING: u8 = 2;
/// A force quit has been decided and cannot be taken back.
const STAGE_QUITTING: u8 = 3;

/// Reported once, when cancellation is first requested.
const CANCEL_MESSAGE: &str =
    "interrupt received - finishing in-flight operations (press Ctrl-C again to force quit)";
/// Reported when the user confirms, or when nothing can be asked.
const FORCE_QUIT_MESSAGE: &str = "force quit - in-flight operations abandoned";
/// Reported when the user declines the force quit.
const DECLINED_MESSAGE: &str = "still finishing in-flight operations";
/// The confirmation question, written without a trailing newline.
const PROMPT: &str = "Force quit? in-flight operations will be abandoned [y/N]: ";

/// What a single interrupt should do, given the ones before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InterruptAction {
    /// Request cooperative cancellation and say so once.
    Cancel,
    /// Ask whether to abandon the run.
    Confirm,
    /// Quit now, without asking again.
    ForceQuit,
}

/// Escalation state shared by the signal handler and the confirmation prompt.
#[derive(Debug, Default)]
struct Escalation {
    stage: AtomicU8,
}

impl Escalation {
    /// Record an interrupt and return the action it should take.
    fn advance(&self) -> InterruptAction {
        if self
            .stage
            .compare_exchange(
                STAGE_RUNNING,
                STAGE_CANCELLING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            return InterruptAction::Cancel;
        }
        if self
            .stage
            .compare_exchange(
                STAGE_CANCELLING,
                STAGE_CONFIRMING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            return InterruptAction::Confirm;
        }
        // Latched so a confirmation answered concurrently cannot roll the
        // decision back.
        self.stage.store(STAGE_QUITTING, Ordering::Release);
        InterruptAction::ForceQuit
    }

    /// Record that a declined confirmation is no longer pending.
    ///
    /// Returning to [`STAGE_CANCELLING`] means the next interrupt asks again
    /// instead of quitting on the strength of an answer already given. The
    /// swap is conditional because a concurrent interrupt may already have
    /// latched [`STAGE_QUITTING`], which a decline must not undo.
    fn confirmation_declined(&self) {
        let _unchanged_when_quitting = self.stage.compare_exchange(
            STAGE_CONFIRMING,
            STAGE_CANCELLING,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

/// Install the Ctrl-C handler for a run.
///
/// Losing the handler is not fatal — the run simply cannot be interrupted
/// gracefully — so a registration failure is reported as a warning.
pub fn install<L: Output + 'static>(token: &CancellationToken, log: &Arc<L>) {
    let state = Arc::new(Escalation::default());
    let handler_token = token.clone();
    let handler_log = Arc::clone(log);
    if ctrlc::set_handler(move || handle_interrupt(&state, &handler_token, &handler_log)).is_err() {
        log.warn("failed to register signal handler");
    }
}

/// Handle one interrupt.
///
/// This runs on the signal handler thread, which cannot process the next
/// interrupt until it returns, so it never blocks on input itself.
fn handle_interrupt<L: Output + 'static>(
    state: &Arc<Escalation>,
    token: &CancellationToken,
    log: &Arc<L>,
) {
    match state.advance() {
        InterruptAction::Cancel => {
            token.cancel();
            log.warn(CANCEL_MESSAGE);
        }
        InterruptAction::Confirm if can_prompt() => spawn_confirmation(state, log),
        // Nothing can answer the question, so honour the second interrupt.
        InterruptAction::Confirm => force_quit(&**log),
        InterruptAction::ForceQuit => {
            end_prompt_line();
            force_quit(&**log);
        }
    }
}

/// Ask for confirmation on a dedicated thread.
///
/// The prompt blocks on stdin, so it must not run on the signal handler
/// thread: that thread has to return promptly for a further interrupt to be
/// seen at all.
fn spawn_confirmation<L: Output + 'static>(state: &Arc<Escalation>, log: &Arc<L>) {
    let state = Arc::clone(state);
    let log = Arc::clone(log);
    drop(
        std::thread::Builder::new()
            .name("interrupt-confirm".to_owned())
            .spawn(move || {
                if confirm_force_quit(&*log) {
                    force_quit(&*log);
                } else {
                    state.confirmation_declined();
                    log.warn(DECLINED_MESSAGE);
                }
            }),
    );
}

/// Return whether a confirmation can be asked and answered.
fn can_prompt() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

/// Ask the user whether to abandon the run.
#[allow(
    clippy::print_stderr,
    reason = "the prompt is interactive output that must bypass the logger to stay on one line"
)]
fn confirm_force_quit(log: &dyn Output) -> bool {
    log.clear_status_line();
    eprint!("\n{PROMPT}");
    drop(std::io::stderr().flush());

    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    is_affirmative(&answer)
}

/// Return whether an answer to [`PROMPT`] confirms the force quit.
///
/// Anything else — including an empty line — declines, so an accidental Enter
/// never abandons a run.
fn is_affirmative(answer: &str) -> bool {
    let answer = answer.trim();
    answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes")
}

/// Move off an unanswered prompt line so the next message starts clean.
#[allow(
    clippy::print_stderr,
    reason = "terminates the raw prompt written by confirm_force_quit"
)]
fn end_prompt_line() {
    eprintln!();
}

/// Abandon the run immediately.
///
/// In-flight children are not reaped, which is the cost the confirmation buys.
fn force_quit(log: &dyn Output) -> ! {
    log.clear_status_line();
    log.error(FORCE_QUIT_MESSAGE);
    log.startup("Run 'dotfiles log' for details.");
    elevation::wait_if_elevated();
    std::process::exit(FORCE_QUIT_EXIT_CODE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_interrupt_requests_cancellation() {
        let state = Escalation::default();

        assert_eq!(
            state.advance(),
            InterruptAction::Cancel,
            "the first interrupt should request cooperative cancellation"
        );
    }

    #[test]
    fn second_interrupt_asks_before_quitting() {
        let state = Escalation::default();
        state.advance();

        assert_eq!(
            state.advance(),
            InterruptAction::Confirm,
            "a repeated interrupt should escalate to a confirmation instead of repeating the warning"
        );
    }

    #[test]
    fn interrupt_during_confirmation_quits_without_asking_again() {
        let state = Escalation::default();
        state.advance();
        state.advance();

        assert_eq!(
            state.advance(),
            InterruptAction::ForceQuit,
            "interrupting an open confirmation should be taken as the answer"
        );
    }

    #[test]
    fn declining_makes_the_next_interrupt_ask_again() {
        let state = Escalation::default();
        state.advance();
        state.advance();
        state.confirmation_declined();

        assert_eq!(
            state.advance(),
            InterruptAction::Confirm,
            "declining should not leave the next interrupt quitting unprompted"
        );
    }

    #[test]
    fn declining_after_a_force_quit_stage_does_not_reopen_the_prompt() {
        let state = Escalation::default();
        state.advance();
        state.advance();
        state.advance();
        state.confirmation_declined();

        assert_eq!(
            state.advance(),
            InterruptAction::ForceQuit,
            "a late decline should not undo an interrupt that already forced the quit"
        );
    }

    #[test]
    fn affirmative_answers_are_accepted_case_insensitively() {
        for answer in ["y\n", "Y\r\n", " yes ", "YES\n"] {
            assert!(
                is_affirmative(answer),
                "{answer:?} should confirm the force quit"
            );
        }
    }

    #[test]
    fn anything_else_declines() {
        for answer in ["", "\n", "n\n", "no", "yep", "1"] {
            assert!(
                !is_affirmative(answer),
                "{answer:?} should not confirm the force quit"
            );
        }
    }
}
