//! Update command implementation.
//!
//! `update` runs the same task graph as `install` but additionally advances
//! locked dependency versions (currently the APM dependency refresh).  It is a
//! strict superset of `install`: everything `install` converges, plus version
//! advancement.  The shared pipeline lives in
//! [`crate::app::commands::install::run_pipeline`]; this command simply opts into
//! version advancement.
use anyhow::Result;
use std::sync::Arc;

use crate::app::cli::{GlobalOpts, UpdateOpts};
use crate::runtime::logging::Logger;

/// Run the update command.
///
/// # Errors
///
/// Returns an error if profile resolution, configuration loading, or task execution fails.
pub fn run(
    global: &GlobalOpts,
    opts: &UpdateOpts,
    log: &Arc<Logger>,
    token: &crate::engine::CancellationToken,
) -> Result<()> {
    super::install::run_pipeline(global, opts, log, token, super::install::RunMode::Update)
}
