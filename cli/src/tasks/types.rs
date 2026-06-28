//! Core task type definitions: identity, phase, policy, and domain.
//!
//! These are the value types that describe a task's metadata — *what* it is
//! ([`Domain`]), *when* it runs ([`TaskPhase`]), *how* it is identified
//! ([`TaskId`]), and the declarative pre-run rules the orchestration layer
//! enforces ([`ExecutionPolicy`]).  The [`Task`](super::Task) trait and the
//! execution engine (`super::execute`) consume these types; keeping them in a
//! dedicated module separates the data model from the trait and the runner.

use std::any::TypeId;
use std::fmt;

use crate::platform::Platform;

/// Unique identifier for a task in the dependency graph.
///
/// Static task types use [`TaskId::Type`], derived from the Rust type system,
/// which is globally unique at compile time.  Dynamically created tasks — such
/// as [`OverlayScriptTask`](crate::tasks::overlay::OverlayScriptTask)
/// where multiple instances of the same struct appear in the same task list —
/// use [`TaskId::Dynamic`] with a hash computed from instance-specific data so
/// that each instance has a distinct identity.
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

/// Declarative rules that the orchestration layer evaluates before a task runs.
#[derive(Debug, Clone, Copy)]
pub enum ExecutionPolicy {
    /// Run whenever the task's own applicability check passes.
    Always,
    /// Run only when the current platform supports the named capability.
    PlatformSupported(&'static str, fn(&Platform) -> bool),
    /// The task may require elevated privileges when it predicts a mutation.
    RequiresElevation,
}

/// Named platform capabilities used to build [`ExecutionPolicy`] values.
#[derive(Debug, Clone, Copy)]
pub enum PlatformCapability {
    /// POSIX chmod support.
    Chmod,
    /// Linux login-shell configuration.
    LinuxShell,
    /// Arch Linux platform support.
    ArchLinux,
    /// Systemd support.
    Systemd,
    /// Windows Subsystem for Linux.
    Wsl,
    /// Native Windows support.
    Windows,
    /// Windows registry support.
    WindowsRegistry,
    /// Arch User Repository support.
    Aur,
    /// Pacman package manager support.
    Pacman,
}

impl PlatformCapability {
    /// Build an execution policy for this capability.
    #[must_use]
    pub const fn policy(self) -> ExecutionPolicy {
        match self {
            Self::Chmod => ExecutionPolicy::PlatformSupported("chmod", Platform::supports_chmod),
            Self::LinuxShell => {
                ExecutionPolicy::PlatformSupported("Linux shell configuration", Platform::is_linux)
            }
            Self::ArchLinux => {
                ExecutionPolicy::PlatformSupported("Arch Linux", Platform::is_arch_linux)
            }
            Self::Systemd => {
                ExecutionPolicy::PlatformSupported("systemd", Platform::supports_systemd)
            }
            Self::Wsl => ExecutionPolicy::PlatformSupported("WSL", Platform::is_wsl),
            Self::Windows => ExecutionPolicy::PlatformSupported("Windows", Platform::is_windows),
            Self::WindowsRegistry => {
                ExecutionPolicy::PlatformSupported("Windows registry", Platform::has_registry)
            }
            Self::Aur => ExecutionPolicy::PlatformSupported("AUR", Platform::supports_aur),
            Self::Pacman => ExecutionPolicy::PlatformSupported("pacman", Platform::uses_pacman),
        }
    }
}

impl TaskPhase {
    /// Human-facing milestone label shown as a `::` header in console output.
    ///
    /// Unlike [`fmt::Display`] (which returns the bare enum variant name and is
    /// used in diagnostics and cycle-error messages), this returns an
    /// outcome-oriented phrase describing what the phase accomplishes for the
    /// user.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Bootstrap => "Setting up dotfiles",
            Self::Sync => "Updating the repository",
            Self::Provision => "Configuring your system",
            Self::Validation => "Checking the setup",
            Self::Update => "Updating dependencies",
        }
    }
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

/// Subject area a task is about, independent of its execution [`TaskPhase`].
///
/// Where [`TaskPhase`] answers *when* a task runs (the scheduler groups by
/// phase to enforce ordering barriers), `Domain` answers *what* a task is
/// about.  The end-of-run summary groups by domain so the report matches the
/// user's mental model (git, packages, files…) rather than internal timing.
///
/// The two axes are genuinely independent: a single domain may span multiple
/// phases.  For example the [`Overlay`](Domain::Overlay) domain loads
/// configuration during [`TaskPhase::Sync`] and runs scripts during
/// [`TaskPhase::Provision`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Domain {
    /// The dotfiles tool itself (binary self-update, wrapper, PATH).
    Core,
    /// The dotfiles repository (sparse checkout, pull, config reload).
    Repository,
    /// Git configuration and hooks.
    Git,
    /// System and language package installation.
    Packages,
    /// Files materialised into place (symlinks, permissions).
    Files,
    /// Shell configuration and completions.
    Shell,
    /// Operating-system integration (systemd, PAM, registry, WSL, developer mode).
    System,
    /// Editor configuration (VS Code extensions).
    Editors,
    /// AI tooling (client settings, APM packages).
    Ai,
    /// Overlay-provided configuration and custom scripts.
    Overlay,
    /// Configuration and lint validation checks.
    Validation,
    /// Default for tasks with no specific subject area (test/mock tasks only).
    General,
}

impl Domain {
    /// All domains in canonical display order, used to group summary output.
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::Core,
            Self::Repository,
            Self::Git,
            Self::Packages,
            Self::Files,
            Self::Shell,
            Self::System,
            Self::Editors,
            Self::Ai,
            Self::Overlay,
            Self::Validation,
            Self::General,
        ]
    }

    /// Human-facing label for this domain, shown as a summary group header.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Core => "Core",
            Self::Repository => "Repository",
            Self::Git => "Git",
            Self::Packages => "Packages",
            Self::Files => "Files",
            Self::Shell => "Shell",
            Self::System => "System",
            Self::Editors => "Editors",
            Self::Ai => "AI",
            Self::Overlay => "Overlay",
            Self::Validation => "Validation",
            Self::General => "General",
        }
    }
}
