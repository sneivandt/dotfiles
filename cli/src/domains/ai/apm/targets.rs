//! Copilot App detection and platform-specific APM skip messages.

use std::path::PathBuf;

use anyhow::{Context as _, Result};

use crate::engine::Context;

/// Additional APM targets that need explicit handling for this user profile.
#[derive(Debug, Clone, Copy)]
pub(super) struct ApmTargets {
    include_copilot_app: bool,
}

impl ApmTargets {
    /// Detect whether the Copilot App target should be included.
    ///
    /// # Errors
    ///
    /// Returns an error when the Copilot App database path cannot be probed.
    pub(super) fn detect(ctx: &Context) -> Result<Self> {
        let db_path = copilot_app_db_path(ctx);
        let include_copilot_app = db_path
            .try_exists()
            .with_context(|| format!("checking Copilot App database {}", db_path.display()))?;
        if !include_copilot_app {
            ctx.debug_fmt(|| {
                format!(
                    "omitting apm target copilot-app because {} is missing",
                    db_path.display()
                )
            });
        }
        Ok(Self {
            include_copilot_app,
        })
    }

    #[must_use]
    pub(super) const fn includes_copilot_app(self) -> bool {
        self.include_copilot_app
    }

    #[must_use]
    pub(super) const fn install_args() -> &'static [&'static str] {
        &["install", "-g"]
    }

    #[must_use]
    pub(super) const fn update_args() -> &'static [&'static str] {
        &["update", "-g", "--yes"]
    }

    #[must_use]
    pub(super) const fn copilot_app_install_args(self) -> Option<&'static [&'static str]> {
        if self.include_copilot_app {
            Some(&["install", "-g", "--target", "copilot-app"])
        } else {
            None
        }
    }
}

/// Return the user-scope Copilot App database path used by APM.
pub(super) fn copilot_app_db_path(ctx: &Context) -> PathBuf {
    ctx.home.join(".copilot").join("data.db")
}

/// Return a platform-specific reason for skipping APM work when `apm` is absent.
pub(super) fn missing_apm_reason(ctx: &Context) -> String {
    let hint = if ctx.platform.is_wsl() {
        Some(
            "install the Windows package with `winget.exe install Microsoft.APM` and re-open your \
             WSL shell",
        )
    } else if ctx.platform.is_windows() {
        Some("install it with `winget install Microsoft.APM`")
    } else if ctx.platform.supports_aur() {
        Some("install it with `paru -S apm-bin`")
    } else {
        None
    };
    hint.map_or_else(
        || "apm not found in PATH".to_string(),
        |hint| format!("apm not found in PATH; {hint}"),
    )
}
