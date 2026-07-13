//! Internal API facade used by integration tests and doctests.
//!
//! This module keeps production internals crate-private while exposing a small,
//! feature-gated surface for tests that need to exercise the real engine.

pub mod cli {
    pub use crate::app::cli::{GlobalOpts, InstallOpts, TestOpts, UpdateOpts};
}

pub mod commands {
    pub mod install {
        pub use crate::app::commands::install::run;
    }

    pub mod log {
        pub use crate::app::commands::log::run;
    }

    pub mod test {
        pub use crate::app::commands::test::run;
    }

    pub mod uninstall {
        pub use crate::app::commands::uninstall::run;
    }

    pub mod update {
        pub use crate::app::commands::update::run;
    }

    pub mod version {
        pub use crate::app::commands::version::run;
    }
}

pub mod config {
    pub use crate::app::config::Config;
    pub use crate::app::config::store::ConfigStore;
    pub use crate::runtime::ConfigHandle;

    pub mod category_matcher {
        pub use crate::runtime::config_support::category_matcher::{Category, matches};
    }

    pub mod profiles {
        pub use crate::app::config::profiles::*;
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
        pub fn validate(tasks: &[&dyn crate::engine::Task]) -> Result<(), GraphError> {
            crate::engine::graph::ResolvedTaskGraph::resolve(tasks).map(|_| ())
        }
    }
}

pub mod exec {
    pub use crate::runtime::exec::{ExecResult, Executor, SystemExecutor};
}

pub mod error {
    pub use crate::engine::resource::ResourceError;
}

pub mod logging {
    pub use crate::runtime::logging::{Log, Logger};
}

pub mod tasks {
    pub use crate::app::catalog::{all_install_tasks, all_uninstall_tasks};
    pub use crate::engine::{
        Context, ContextOpts, ProcessMode, ProcessOpts, ResourceAction, Task, TaskId, TaskPhase,
        TaskResult, TaskStats, execute,
    };

    pub mod filter {
        pub use crate::app::filter::task_matches_filter;
    }

    pub mod files {
        pub mod chmod {
            pub use crate::domains::files::tasks::chmod::ApplyFilePermissions;
        }

        pub mod symlinks {
            pub use crate::domains::files::tasks::symlinks::{InstallSymlinks, UninstallSymlinks};
        }
    }

    pub mod editors {
        pub mod vscode_extensions {
            pub use crate::domains::editors::tasks::InstallVsCodeExtensions;
        }
    }

    pub mod git {
        pub mod git_config {
            pub use crate::domains::git::tasks::git_config::ConfigureGit;
        }

        pub mod hooks {
            pub use crate::domains::git::tasks::hooks::{InstallGitHooks, UninstallGitHooks};
        }
    }

    pub mod packages {
        pub use crate::domains::packages::tasks::{InstallAurPackages, InstallPackages};
    }

    pub mod system {
        pub mod systemd_units {
            pub use crate::domains::system::tasks::systemd_units::ConfigureSystemd;
        }
    }
}

pub mod platform {
    pub use crate::runtime::platform::{Os, Platform};
}

pub mod resources {
    pub use crate::engine::{IntrinsicState, ResourceChange, ResourceState};

    pub mod chmod {
        pub use crate::domains::files::resources::chmod::OctalMode;
    }

    pub mod symlink {
        pub use crate::domains::files::resources::symlink::SymlinkResource;
    }
}
