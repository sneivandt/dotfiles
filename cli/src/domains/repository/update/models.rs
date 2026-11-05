//! Domain types for the repository update task.
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UpdateTargetKind {
    Main,
    Overlay,
}

#[derive(Debug, Clone)]
pub(super) struct UpdateTarget {
    /// Target kind — controls human-readable labels and error messages.
    kind: UpdateTargetKind,
    /// Root directory of the repository.
    pub(super) root: PathBuf,
}

impl UpdateTarget {
    pub(super) const fn new(kind: UpdateTargetKind, root: PathBuf) -> Self {
        Self { kind, root }
    }

    pub(super) const fn description(&self) -> &'static str {
        match self.kind {
            UpdateTargetKind::Main => "repository",
            UpdateTargetKind::Overlay => "overlay repository",
        }
    }

    pub(super) fn reason(&self, reason: &str) -> String {
        match self.kind {
            UpdateTargetKind::Main => reason.to_string(),
            UpdateTargetKind::Overlay => format!("{reason} in {}", self.description()),
        }
    }

    pub(super) fn dry_run_action(&self) -> String {
        match self.kind {
            UpdateTargetKind::Main => {
                "fast-forward available (local merge eligibility not checked)".to_string()
            }
            UpdateTargetKind::Overlay => {
                "overlay fast-forward available (local merge eligibility not checked)".to_string()
            }
        }
    }
}

/// A repository whose HEAD is on a branch and whose worktree is clean, ready
/// to be considered for a pull.
#[derive(Debug, Clone)]
pub(super) struct CheckedRepository {
    pub(super) target: UpdateTarget,
    pub(super) head_ref: String,
}

#[derive(Debug)]
pub(super) enum RepositoryReadiness {
    Ready(CheckedRepository),
    Blocked(String),
    NotApplicable(String),
}

#[derive(Debug)]
pub(super) enum RepositorySetReadiness {
    Ready(Vec<CheckedRepository>),
    Blocked(String),
    NotApplicable(String),
}

/// A repository and the exact commit eligible for a fast-forward.
#[derive(Debug)]
pub(super) struct RepositoryUpdatePlan {
    pub(super) target: UpdateTarget,
    pub(super) commit: Option<String>,
}

#[derive(Debug)]
pub(super) enum RepositoryPlanReadiness {
    Ready(RepositoryUpdatePlan),
    Skipped(String),
}

/// Dry-run comparison result between HEAD and the known upstream SHA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DryRunUpdateStatus {
    AlreadyCurrent,
    WouldUpdate,
    Ahead,
    Diverged,
    Unknown,
}
