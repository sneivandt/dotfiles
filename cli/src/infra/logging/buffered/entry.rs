//! Buffered console entries produced during parallel task execution.
//!
//! Splitting the entry model out of [`BufferedLog`](super::BufferedLog) keeps
//! replay/visibility policy (what a single entry means for the console and for
//! summary details) separate from the buffering and flush orchestration.

use unicode_width::UnicodeWidthStr as _;

use crate::infra::logging::logger::Logger;
use crate::infra::logging::runlog::RunLog;
use crate::infra::logging::style::{StyleChoice, TextStyle};
use crate::infra::logging::types::{MsgKind, TaskStatus};
use crate::infra::logging::utils::{duplicates_task_message, strip_ansi};

/// A single buffered console entry, replayed when the task completes.
///
/// Messages and typed actions share one buffer. Everything is
/// already recorded in the run log at the moment it is produced, so replay is
/// purely a console-rendering concern.
#[derive(Debug, Clone)]
pub(in crate::infra::logging) enum LogEntry {
    Message {
        kind: MsgKind,
        msg: String,
    },
    Action {
        verb: String,
        subject: String,
        planned: bool,
        message: String,
    },
}

impl LogEntry {
    pub(in crate::infra::logging) fn action(
        verb: &str,
        subject: &str,
        planned: bool,
        message: &str,
    ) -> Self {
        Self::Action {
            verb: verb.into(),
            subject: subject.into(),
            planned,
            message: message.into(),
        }
    }

    pub(in crate::infra::logging) const fn kind(&self) -> MsgKind {
        match self {
            Self::Message { kind, .. } => *kind,
            Self::Action { planned: true, .. } => MsgKind::DryRun,
            Self::Action { planned: false, .. } => MsgKind::Info,
        }
    }

    pub(in crate::infra::logging) fn message(&self) -> &str {
        match self {
            Self::Message { msg, .. } => msg,
            Self::Action { message, .. } => message,
        }
    }

    pub(in crate::infra::logging) fn persist(&self, log: &RunLog) {
        match self {
            Self::Message { kind, msg } => log.emit(kind.log_event(), msg),
            Self::Action {
                verb,
                subject,
                planned,
                message,
            } => log.record_action(verb, subject, *planned, message),
        }
    }

    /// Sort only consecutive actions; messages remain ordering barriers.
    pub(super) fn sort_actions(entries: &mut [Self]) {
        for actions in entries.split_mut(|entry| matches!(entry, Self::Message { .. })) {
            actions.sort_by(|left, right| left.message().cmp(right.message()));
        }
    }

    /// Replay this entry to the console.
    pub(in crate::infra::logging) fn replay(&self, logger: &Logger) {
        match self {
            Self::Message { kind, msg } => logger.emit_console(*kind, msg),
            Self::Action { .. } => {
                if let Some(line) = self.console_line(
                    logger.console.stdout_style,
                    logger.is_verbose(),
                    logger.symbols,
                    1,
                ) {
                    logger
                        .console
                        .detail(self.kind(), self.message(), &line, false);
                }
            }
        }
    }

    /// Measure the verb and the first line at its original spacing.
    pub(in crate::infra::logging) fn action_columns(&self) -> Option<(usize, usize)> {
        let Self::Action {
            verb,
            subject,
            message,
            ..
        } = self
        else {
            return None;
        };
        action_suffix(verb, subject, message)?;
        let verb = strip_ansi(verb);
        if verb.contains(['\n', '\r', '\t']) {
            return None;
        }
        let message = strip_ansi(message);
        Some((
            verb.width(),
            2 + message.lines().next().unwrap_or_default().width(),
        ))
    }

    /// Preserve typed spans and opaque subjects; never infer actions from prose.
    pub(in crate::infra::logging) fn console_line(
        &self,
        style: StyleChoice,
        verbose: bool,
        symbols: bool,
        action_gap: usize,
    ) -> Option<String> {
        let Self::Action {
            verb,
            subject,
            planned,
            message,
        } = self
        else {
            return crate::infra::logging::console::ui_line_with_style(
                self.kind(),
                self.message(),
                style,
                verbose,
                symbols,
            );
        };
        if !planned && !verbose {
            return None;
        }
        let Some(suffix) = action_suffix(verb, subject, message) else {
            return Some(format!("  {}", style.clean(message)).replace('\n', "\n  "));
        };
        let subject = style.clean(subject);
        let formatted_subject = match subject.split_once(" \u{2192} ") {
            Some((left, right)) => {
                format!("{left}{}{right}", style.paint(TextStyle::Dim, " \u{2192} "))
            }
            None => subject,
        };
        Some(
            format!(
                "  {}{:action_gap$}{}{}",
                style.paint(
                    if *planned {
                        TextStyle::Magenta
                    } else {
                        TextStyle::Cyan
                    },
                    verb
                ),
                "",
                formatted_subject,
                if suffix.is_empty() {
                    String::new()
                } else {
                    style.paint(TextStyle::Dim, suffix)
                },
            )
            .replace('\n', "\n  "),
        )
    }

    /// Select the message kind and text for verbose task detail.
    ///
    /// Task-name headers are suppressed because the task status line already
    /// names the task, as are lines that only restate the task's own outcome:
    /// its reason (already on the status row) and its aggregate counters
    /// (already implied by the per-item lines around them).
    ///
    /// Action messages already have their console form, shared with completed rows.
    pub(in crate::infra::logging) fn verbose_detail(
        &self,
        task_message: Option<&str>,
    ) -> Option<(MsgKind, &str)> {
        if matches!(
            self.kind(),
            MsgKind::Stage | MsgKind::Trace | MsgKind::Summary
        ) || duplicates_task_message(self.message(), task_message)
        {
            return None;
        }
        Some((self.kind(), self.message().trim_start()))
    }

    /// The summary detail line contributed by this entry, if any.
    pub(in crate::infra::logging) fn detail_line(&self, status: TaskStatus) -> Option<&str> {
        match self.kind() {
            MsgKind::Info | MsgKind::DryRun | MsgKind::Always => Some(self.message()),
            MsgKind::Warn | MsgKind::WarnContinuation | MsgKind::Error
                if status == TaskStatus::Failed =>
            {
                Some(self.message())
            }
            MsgKind::Stage
            | MsgKind::Debug
            | MsgKind::Context
            | MsgKind::Trace
            | MsgKind::Summary
            | MsgKind::Startup
            | MsgKind::Transition
            | MsgKind::RecoveryHint
            | MsgKind::SudoRequired
            | MsgKind::Warn
            | MsgKind::WarnContinuation
            | MsgKind::Error => None,
        }
    }

    /// Whether this entry appears on the console in non-verbose mode.
    ///
    /// A failed task reports through its summary entry instead, so its
    /// buffered output stays in the run log only.  Warnings that merely repeat
    /// the task's own reason are dropped as well: that reason is already on the
    /// task's status row, and printing it again on stderr detaches it from the
    /// task it belongs to.
    pub(in crate::infra::logging) fn is_visible_in_non_verbose(
        &self,
        status: TaskStatus,
        task_message: Option<&str>,
    ) -> bool {
        status != TaskStatus::Failed
            && matches!(
                self.kind(),
                MsgKind::Warn | MsgKind::WarnContinuation | MsgKind::Error
            )
            && !duplicates_task_message(self.message(), task_message)
    }
}

fn action_suffix<'a>(verb: &str, subject: &str, message: &'a str) -> Option<&'a str> {
    message
        .strip_prefix(verb)?
        .strip_prefix(' ')?
        .strip_prefix(subject)
        .filter(|suffix| suffix.is_empty() || suffix.starts_with(char::is_whitespace))
}

pub(in crate::infra::logging) const fn should_record_task_details(status: TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Changed
            | TaskStatus::Skipped
            | TaskStatus::Blocked
            | TaskStatus::Interrupted
            | TaskStatus::DryRun
            | TaskStatus::Failed
    )
}
