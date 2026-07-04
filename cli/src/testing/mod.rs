//! Internal API facade used by integration tests and doctests.
//!
//! This module keeps production internals crate-private while exposing a small,
//! feature-gated surface for tests that need to exercise the real engine.

pub mod cli {
    pub use crate::cli::{GlobalOpts, InstallOpts, TestOpts, UpdateOpts};
}

pub mod commands {
    pub mod install {
        pub use crate::commands::install::run;
    }

    pub mod log {
        pub use crate::commands::log::run;
    }

    pub mod test {
        pub use crate::commands::test::run;
    }

    pub mod uninstall {
        pub use crate::commands::uninstall::run;
    }

    pub mod update {
        pub use crate::commands::update::run;
    }

    pub mod version {
        pub use crate::commands::version::run;
    }
}

pub mod config {
    pub use crate::config::Config;

    pub mod category_matcher {
        pub use crate::config::category_matcher::{Category, matches};
    }

    pub mod profiles {
        pub use crate::config::profiles::*;
    }
}

pub mod engine {
    pub use crate::engine::{CancellationToken, Context, ContextOpts};

    pub mod graph {
        pub use crate::engine::graph::GraphError;

        /// Validate a task dependency graph.
        ///
        /// # Errors
        ///
        /// Returns [`GraphError::DuplicateId`] if two tasks share a task ID,
        /// or [`GraphError::Cycle`] if the graph contains a cycle.
        pub fn validate(tasks: &[&dyn crate::tasks::Task]) -> Result<(), GraphError> {
            crate::engine::graph::ResolvedTaskGraph::resolve(tasks).map(|_| ())
        }
    }
}

pub mod exec {
    pub use crate::exec::{ExecResult, Executor, SystemExecutor};
}

pub mod error {
    pub use crate::error::ResourceError;
}

pub mod logging {
    pub use crate::logging::{Log, Logger};
}

pub mod tasks {
    pub use crate::tasks::{
        Context, ContextOpts, ProcessMode, ProcessOpts, ResourceAction, Task, TaskId, TaskPhase,
        TaskResult, TaskStats, all_install_tasks, all_uninstall_tasks, execute,
    };

    pub mod filter {
        pub use crate::tasks::filter::task_matches_filter;
    }

    pub mod files {
        pub mod chmod {
            pub use crate::tasks::files::chmod::ApplyFilePermissions;
        }

        pub mod symlinks {
            pub use crate::tasks::files::symlinks::{InstallSymlinks, UninstallSymlinks};
        }
    }

    pub mod editors {
        pub mod vscode_extensions {
            pub use crate::tasks::editors::InstallVsCodeExtensions;
        }
    }

    pub mod git {
        pub mod git_config {
            pub use crate::tasks::git::git_config::ConfigureGit;
        }

        pub mod hooks {
            pub use crate::tasks::git::hooks::{InstallGitHooks, UninstallGitHooks};
        }
    }

    pub mod packages {
        pub use crate::tasks::packages::{InstallAurPackages, InstallPackages};
    }

    pub mod system {
        pub mod systemd_units {
            pub use crate::tasks::system::systemd_units::ConfigureSystemd;
        }
    }
}

pub mod platform {
    pub use crate::platform::{Os, Platform};
}

pub mod resources {
    pub use crate::resources::{IntrinsicState, ResourceChange, ResourceState};

    pub mod chmod {
        pub use crate::resources::chmod::OctalMode;
    }

    pub mod symlink {
        pub use crate::resources::symlink::SymlinkResource;
    }
}
