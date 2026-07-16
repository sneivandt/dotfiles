//! Generic configuration parsing support: category filtering, TOML section
//! loading, git-backed local state, validation helpers, and diagnostics.
//!
//! These are domain-agnostic building blocks used by per-domain config loaders
//! and the aggregate configuration composition in `app`.

pub(crate) mod category_matcher;
pub(crate) mod diagnostics;
pub(crate) mod git_state;
mod handle;
pub(crate) mod section_macro;
pub(crate) mod toml_loader;
pub(crate) mod validation;

pub(crate) use diagnostics::{Diagnostic, Severity};
pub use handle::ConfigHandle;
pub(crate) use section_macro::config_section;

#[cfg(test)]
pub mod test_helpers;

#[cfg(test)]
macro_rules! test_load_missing_returns_empty {
    ($loader:path) => {
        #[test]
        fn load_missing_file_returns_empty() {
            crate::infra::config::test_helpers::assert_load_missing_returns_empty($loader);
        }
    };
}

#[cfg(test)]
pub(crate) use test_load_missing_returns_empty;

#[cfg(test)]
macro_rules! test_load_missing_unfiltered_returns_empty {
    ($loader:path) => {
        #[test]
        fn load_missing_file_returns_empty() {
            crate::infra::config::test_helpers::assert_load_missing_unfiltered_returns_empty(
                $loader,
            );
        }
    };
}

#[cfg(test)]
pub(crate) use test_load_missing_unfiltered_returns_empty;
