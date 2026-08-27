//! Core task identity definitions.

use std::any::TypeId;
use std::sync::Arc;

use crate::infra::logging::TaskVisibility;

/// Unique identifier for a task in the dependency graph.
///
/// Static task types use [`TaskId::Type`], derived from the Rust type system,
/// which is globally unique at compile time. Dynamically created tasks — such
/// as scripts where multiple instances of the same struct appear in the same
/// task list — use [`TaskId::dynamic`] with an owned, instance-specific key.
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
/// let id = TaskId::dynamic::<String>("instance-42");
///
/// assert_ne!(id, TaskId::Type(TypeId::of::<u32>()));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TaskId {
    /// Type-derived identifier for static singleton task structs.
    ///
    /// Produced automatically by the default `task_id()` implementation.
    Type(TypeId),
    /// Collision-free instance identifier for dynamically created tasks.
    ///
    /// Used when multiple instances of the same struct appear in the task
    /// list (e.g. one `OverlayScriptTask` per configured script).
    Dynamic(u64),
    /// Structured identity for dynamically discovered tasks.
    NamedDynamic {
        /// Concrete task type, preventing keys from different dynamic task
        /// implementations from sharing an identity.
        kind: TypeId,
        /// Stable instance key within `kind`.
        key: Arc<str>,
    },
}

impl TaskId {
    /// Create a collision-free identity for one dynamic task instance.
    #[must_use]
    pub fn dynamic<T: 'static>(key: impl Into<Arc<str>>) -> Self {
        Self::NamedDynamic {
            kind: TypeId::of::<T>(),
            key: key.into(),
        }
    }

    /// Build the collision-free key used by execution and logging records.
    #[must_use]
    pub fn record_key(&self) -> String {
        match self {
            Self::Type(kind) => format!("type:{kind:?}"),
            Self::Dynamic(value) => format!("dynamic:{value}"),
            Self::NamedDynamic { kind, key } => format!("named:{kind:?}:{key}"),
        }
    }
}

/// Whether a task applies to the current run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applicability {
    /// The task is eligible to execute.
    Applicable,
    /// The task is ineligible for this run.
    NotApplicable {
        /// Optional user-facing reason for the decision.
        reason: Option<String>,
    },
}

/// Privilege requirement predicted while assessing a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevationRequirement {
    /// The task can run without elevated privileges.
    None,
    /// The task predicts that its pending mutation requires elevation.
    Required,
}

/// Immutable task eligibility and privilege assessment for one execution phase.
///
/// The coordinator computes this once before elevation planning and scheduling,
/// so filesystem and tool probes cannot disagree between those stages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskAssessment {
    /// Eligibility for this run.
    applicability: Applicability,
    /// Predicted privilege requirement.
    elevation: ElevationRequirement,
}

impl TaskAssessment {
    /// Build an applicable, unelevated assessment.
    #[must_use]
    pub const fn applicable() -> Self {
        Self {
            applicability: Applicability::Applicable,
            elevation: ElevationRequirement::None,
        }
    }

    /// Build a non-applicable assessment with an optional reason.
    #[must_use]
    pub fn not_applicable(reason: Option<impl Into<String>>) -> Self {
        Self {
            applicability: Applicability::NotApplicable {
                reason: reason.map(Into::into),
            },
            elevation: ElevationRequirement::None,
        }
    }

    /// Set whether this task requires elevation.
    #[must_use]
    pub const fn with_elevation(mut self, required: bool) -> Self {
        self.elevation = if required {
            ElevationRequirement::Required
        } else {
            ElevationRequirement::None
        };
        self
    }

    /// Whether this task applies to the current run.
    #[must_use]
    pub const fn is_applicable(&self) -> bool {
        matches!(self.applicability, Applicability::Applicable)
    }

    /// Why this task does not apply, when a reason is available.
    #[must_use]
    pub fn not_applicable_reason(&self) -> Option<&str> {
        match &self.applicability {
            Applicability::Applicable => None,
            Applicability::NotApplicable { reason } => reason.as_deref(),
        }
    }

    /// Whether this task predicts an elevated mutation.
    #[must_use]
    pub const fn requires_elevation(&self) -> bool {
        matches!(self.elevation, ElevationRequirement::Required)
    }
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
    /// Whether the task is included only by `install --update-pins`.
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

    /// Mark the task as included only by `install --update-pins`.
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
