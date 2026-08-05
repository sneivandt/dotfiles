//! Core task identity definitions.

use std::any::TypeId;

use crate::infra::logging::TaskVisibility;

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

/// Static descriptive metadata for a [`Task`](crate::engine::Task).
///
/// The four values here used to be four separately defaulted trait methods.
/// Bundling them into one required method means a task declares its identity
/// in exactly one place, and a decorator such as
/// [`TaskWithExtraDeps`](crate::engine::TaskWithExtraDeps) forwards one method
/// instead of four — it can no longer silently drop a selector or a visibility
/// override by forgetting to forward it.
///
/// # Examples
///
/// ```
/// use dotfiles_cli::testing::tasks::TaskMeta;
///
/// let meta = TaskMeta::new("Install symlinks").with_selector("symlinks");
///
/// assert_eq!(meta.name, "Install symlinks");
/// assert_eq!(meta.selector(), "symlinks");
/// assert!(!meta.update_only);
///
/// // The selector falls back to the name when none is given.
/// assert_eq!(TaskMeta::new("checks").selector(), "checks");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskMeta<'a> {
    /// Human-readable task name.
    pub name: &'a str,
    /// Stable selector used by `--only` and `--skip`.
    ///
    /// `None` means the selector is the name; read it through
    /// [`TaskMeta::selector`] rather than matching on the field.
    pub selector: Option<&'a str>,
    /// Whether the task is shown in user-facing discovery and results.
    pub visibility: TaskVisibility,
    /// Whether the task is included only by the `update` command.
    pub update_only: bool,
}

impl<'a> TaskMeta<'a> {
    /// Metadata for a visible, always-included task named `name`.
    #[must_use]
    pub const fn new(name: &'a str) -> Self {
        Self {
            name,
            selector: None,
            visibility: TaskVisibility::Visible,
            update_only: false,
        }
    }

    /// Set the `--only`/`--skip` selector.
    #[must_use]
    pub const fn with_selector(mut self, selector: &'a str) -> Self {
        self.selector = Some(selector);
        self
    }

    /// Set the task's visibility.
    #[must_use]
    pub const fn with_visibility(mut self, visibility: TaskVisibility) -> Self {
        self.visibility = visibility;
        self
    }

    /// Mark the task as included only by the `update` command.
    #[must_use]
    pub const fn with_update_only(mut self, update_only: bool) -> Self {
        self.update_only = update_only;
        self
    }

    /// The `--only`/`--skip` selector, falling back to the task name.
    #[must_use]
    pub const fn selector(&self) -> &'a str {
        match self.selector {
            Some(selector) => selector,
            None => self.name,
        }
    }
}
