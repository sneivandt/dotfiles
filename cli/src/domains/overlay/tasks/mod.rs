//! Overlay tasks.

mod scripts;

#[allow(
    unused_imports,
    reason = "OverlayScriptTask is re-exported for documentation links and test access"
)]
pub use scripts::{OverlayScriptTask, ReportOverlayScriptSnapshot, overlay_script_tasks};

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests;
