//! Concrete domain modules.
//!
//! Each domain colocates its configuration models, resource implementations,
//! task implementations, and unit tests. Domains depend on [`crate::engine`]
//! and [`crate::infra`] only; cross-domain wiring lives in the application
//! layer.

pub(crate) mod ai;
pub(crate) mod dotfiles;
pub(crate) mod editors;
pub(crate) mod files;
pub(crate) mod git;
pub(crate) mod overlay;
pub(crate) mod packages;
pub(crate) mod repository;
pub(crate) mod shell;
pub(crate) mod system;
