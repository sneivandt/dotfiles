//! Concrete domain modules.
//!
//! Each domain colocates its configuration models, resource implementations,
//! task implementations, and unit tests. Domains depend on [`crate::engine`]
//! and [`crate::infra`] only; cross-domain wiring lives in the application
//! layer.

pub mod ai;
pub mod dotfiles;
pub mod editors;
pub mod files;
pub mod git;
pub mod overlay;
pub mod packages;
pub mod repository;
pub mod shell;
pub mod system;
