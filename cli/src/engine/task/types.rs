//! Core task type definitions: identity and execution phase.
//!
//! [`TaskPhase`] controls scheduler barriers and [`TaskId`] provides stable
//! dependency-graph identity.

use std::any::TypeId;
use std::fmt;

/// Unique identifier for a task in the dependency graph.
///
/// Static task types use [`TaskId::Type`], derived from the Rust type system,
/// which is globally unique at compile time.  Dynamically created tasks — such
/// as scripts where multiple instances of the same struct appear in the same
/// task list — use [`TaskId::Dynamic`] with a hash computed from
/// instance-specific data so that each instance has a distinct identity.
///
/// # Examples
///
/// ```
/// use std::any::TypeId;
/// use dotfiles_cli::testing::tasks::TaskId;
///
/// // Type-based ID (the usual case):
/// let id = TaskId::Type(TypeId::of::<u32>());
///
/// // Instance-based ID (for dynamic tasks):
/// let id = TaskId::Dynamic(42);
///
/// assert_ne!(id, TaskId::Type(TypeId::of::<u32>()));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskId {
    /// Type-derived identifier for static singleton task structs.
    ///
    /// Produced automatically by the default `task_id()` implementation.
    Type(TypeId),
    /// Instance-derived identifier for dynamically created tasks.
    ///
    /// Used when multiple instances of the same struct appear in the task
    /// list (e.g. one `OverlayScriptTask` per configured script).
    Dynamic(u64),
}

/// Execution phase of a task.
///
/// Bootstrap tasks run first to prepare the tool itself (binary update,
/// wrapper installation, PATH configuration).  Sync tasks run
/// second to synchronise the dotfiles repository (sparse checkout,
/// pull, config reload, hooks).  Provision tasks run third to converge the
/// user environment to its declared state (symlinks, packages, etc.).
/// Validation tasks run the `test` command's checks. Update tasks advance
/// pinned/locked dependency versions beyond the declared state; they are only
/// scheduled by the `update` command, so the phase is absent (and its header
/// omitted) under ordinary `install` runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskPhase {
    /// Prepare the dotfiles tool itself.
    Bootstrap,
    /// Synchronise the dotfiles repository.
    Sync,
    /// Converge the user environment to its declared state.
    Provision,
    /// Run configuration and script validation checks.
    Validation,
    /// Advance pinned/locked dependency versions (the `update` command only).
    Update,
}

impl fmt::Display for TaskPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bootstrap => f.write_str("Bootstrap"),
            Self::Sync => f.write_str("Sync"),
            Self::Provision => f.write_str("Provision"),
            Self::Validation => f.write_str("Validation"),
            Self::Update => f.write_str("Update"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_phase_display() {
        assert_eq!(TaskPhase::Bootstrap.to_string(), "Bootstrap");
        assert_eq!(TaskPhase::Sync.to_string(), "Sync");
        assert_eq!(TaskPhase::Provision.to_string(), "Provision");
        assert_eq!(TaskPhase::Validation.to_string(), "Validation");
        assert_eq!(TaskPhase::Update.to_string(), "Update");
    }

    #[test]
    fn task_phase_equality() {
        assert_eq!(TaskPhase::Bootstrap, TaskPhase::Bootstrap);
        assert_eq!(TaskPhase::Sync, TaskPhase::Sync);
        assert_eq!(TaskPhase::Provision, TaskPhase::Provision);
        assert_ne!(TaskPhase::Bootstrap, TaskPhase::Sync);
        assert_ne!(TaskPhase::Bootstrap, TaskPhase::Provision);
        assert_ne!(TaskPhase::Sync, TaskPhase::Provision);
    }
}
