//! Top-level command handlers and shared command orchestration.

pub mod check;
pub mod list;
pub mod log;
pub mod remove;
pub mod update;

pub(crate) mod error;
mod execution;
mod reexec;
mod runner;
mod runtime;

pub(crate) use reexec::prepare_self_update;
pub(crate) use reexec::re_exec_after_repository_update;
pub use runner::CommandRunner;
pub use runtime::RuntimePolicy;

#[cfg(test)]
use reexec::{
    REEXEC_GUARD_VAR, REPOSITORY_REEXEC_GUARD_VAR, SELF_UPDATE_REEXEC_GUARD_VAR,
    build_reexec_command, re_exec_path, should_check_self_update,
};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

/// Retain requested selectors before startup can fail or restart the process.
fn record_scope(
    log: &crate::infra::logging::Logger,
    filters: &crate::app::cli::TaskFilters,
    with_deps: bool,
    no_repo_update: bool,
) {
    log.record(crate::infra::logging::records::Record::RunScope {
        only: filters.only.clone(),
        skip: filters.skip.clone(),
        with_deps,
        no_repo_update,
    });
}

#[cfg(test)]
mod scope_tests {
    use super::*;
    use crate::infra::logging::records::{Record, StoredRecord};

    #[test]
    fn scope_retains_requested_filters_and_dependency_and_repository_policy() {
        let (log, _tmp, _guard) = crate::infra::logging::isolated_logger();
        let filters = crate::app::cli::TaskFilters {
            only: vec!["symlinks".into(), "git-config".into()],
            skip: vec!["packages".into()],
        };
        record_scope(&log, &filters, true, true);
        let content = std::fs::read_to_string(log.log_path().unwrap()).unwrap();
        let records: Vec<_> = content
            .lines()
            .filter_map(StoredRecord::from_line)
            .collect();
        assert_eq!(records.len(), 1);
        assert!(matches!(&records[0].record,
            Record::RunScope { only, skip, with_deps: true, no_repo_update: true }
            if only == &filters.only && skip == &filters.skip));
    }
}
