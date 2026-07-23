//! Core task identity definitions.

use std::any::TypeId;

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
