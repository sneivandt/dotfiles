//! Managed Copilot target detection and platform-specific APM skip messages.

use std::path::PathBuf;

use anyhow::{Context as _, Result};

use crate::engine::Context;

pub(super) const ONEDRIVE_COMMERCIAL: &str = "ONEDRIVECOMMERCIAL";

/// A Copilot surface whose deployment needs handling beyond the base APM
/// manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CopilotTarget {
    CopilotApp,
    Cowork,
}

impl CopilotTarget {
    const ALL: [Self; 2] = [Self::CopilotApp, Self::Cowork];

    #[must_use]
    pub(super) const fn apm_name(self) -> &'static str {
        match self {
            Self::CopilotApp => "copilot-app",
            Self::Cowork => "copilot-cowork",
        }
    }

    #[must_use]
    pub(super) const fn display_name(self) -> &'static str {
        match self {
            Self::CopilotApp => "Copilot App",
            Self::Cowork => "Microsoft 365 Copilot Cowork",
        }
    }

    #[must_use]
    pub(super) const fn deployment(self) -> CopilotDeployment {
        match self {
            Self::CopilotApp => CopilotDeployment::ExperimentalInstall {
                config_key: "copilot_app",
                args: &["install", "-g", "--target", "copilot-app"],
            },
            Self::Cowork => CopilotDeployment::CoworkReconcile,
        }
    }

    const fn mask(self) -> u8 {
        match self {
            Self::CopilotApp => 1,
            Self::Cowork => 2,
        }
    }

    fn is_available(self, ctx: &Context) -> Result<bool> {
        match self {
            Self::CopilotApp => {
                let path = copilot_app_db_path(ctx);
                path.try_exists()
                    .with_context(|| format!("checking Copilot App database {}", path.display()))
            }
            Self::Cowork => Ok(copilot_cowork_skills_path(ctx).is_some()),
        }
    }

    fn unavailable_detail(self, ctx: &Context) -> String {
        match self {
            Self::CopilotApp => copilot_app_db_path(ctx).display().to_string(),
            Self::Cowork if !ctx.system().platform().is_windows() => {
                "this is not native Windows".to_string()
            }
            Self::Cowork => format!("{ONEDRIVE_COMMERCIAL} is not set"),
        }
    }
}

/// How a managed Copilot target converges after the primary APM command.
#[derive(Debug, Clone, Copy)]
pub(super) enum CopilotDeployment {
    ExperimentalInstall {
        config_key: &'static str,
        args: &'static [&'static str],
    },
    CoworkReconcile,
}

/// Active managed Copilot targets for this machine.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct ApmTargets {
    active: u8,
}

impl ApmTargets {
    /// Detect which managed Copilot targets should be included.
    ///
    /// # Errors
    ///
    /// Returns an error when the Copilot App database path cannot be probed.
    pub(super) fn detect(ctx: &Context) -> Result<Self> {
        let mut active = Vec::new();
        for target in CopilotTarget::ALL {
            if target.is_available(ctx)? {
                active.push(target);
            } else {
                ctx.debug_fmt(|| {
                    format!(
                        "omitting managed target {} because {}",
                        target.apm_name(),
                        target.unavailable_detail(ctx)
                    )
                });
            }
        }
        Ok(Self::from_targets(&active))
    }

    #[must_use]
    pub(super) fn from_targets(targets: &[CopilotTarget]) -> Self {
        let mut active = 0;
        for target in targets {
            active |= target.mask();
        }
        Self { active }
    }

    #[must_use]
    pub(super) const fn includes(self, target: CopilotTarget) -> bool {
        self.active & target.mask() != 0
    }

    pub(super) fn active(self) -> impl Iterator<Item = CopilotTarget> {
        CopilotTarget::ALL
            .into_iter()
            .filter(move |target| self.includes(*target))
    }
}

/// Return the user-scope Copilot App database path used by APM.
pub(super) fn copilot_app_db_path(ctx: &Context) -> PathBuf {
    ctx.home().join(".copilot").join("data.db")
}

/// Return the native Windows Copilot Cowork skill path when its `OneDrive`
/// location is configured.
pub(super) fn copilot_cowork_skills_path(ctx: &Context) -> Option<PathBuf> {
    ctx.system().platform().is_windows().then(|| {
        ctx.env()
            .var_os(ONEDRIVE_COMMERCIAL)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(|onedrive| onedrive.join("Documents").join("Cowork").join("skills"))
    })?
}

/// Return a platform-specific reason for skipping APM work when `apm` is absent.
pub(super) fn missing_apm_reason(ctx: &Context) -> String {
    let platform = ctx.system().platform();
    let hint = if platform.is_wsl() {
        Some(
            "install the Windows package with `winget.exe install Microsoft.APM` and re-open your \
             WSL shell",
        )
    } else if platform.is_windows() {
        Some("install it with `winget install Microsoft.APM`")
    } else if platform.supports_aur() {
        Some("install it with `paru -S apm-bin`")
    } else {
        None
    };
    hint.map_or_else(
        || "apm not found in PATH".to_string(),
        |hint| format!("apm not found in PATH; {hint}"),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::domains::ai::apm::test_fixture::make_context_with_home;
    use crate::infra::env::MapEnv;
    use crate::infra::exec::MockExecutor;
    use crate::infra::platform::{Os, Platform};

    use super::*;

    #[test]
    fn detect_collects_available_managed_targets() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::create_dir_all(dir.path().join(".copilot")).expect("create Copilot directory");
        std::fs::write(dir.path().join(".copilot").join("data.db"), "db")
            .expect("write Copilot App database");
        let ctx = make_context_with_home(
            dir.path(),
            Platform::new(Os::Windows, false),
            MockExecutor::new(),
        );

        let targets = ApmTargets::detect(&ctx).expect("detect targets");

        assert!(targets.includes(CopilotTarget::CopilotApp));
        assert!(targets.includes(CopilotTarget::Cowork));
        assert_eq!(targets.active().count(), 2);
    }

    #[test]
    fn detect_omits_cowork_without_native_onedrive_location() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let ctx = make_context_with_home(
            dir.path(),
            Platform::new(Os::Windows, false),
            MockExecutor::new(),
        )
        .with_env(Arc::new(MapEnv::new()));

        let targets = ApmTargets::detect(&ctx).expect("detect targets");

        assert!(!targets.includes(CopilotTarget::CopilotApp));
        assert!(!targets.includes(CopilotTarget::Cowork));
        assert_eq!(targets.active().count(), 0);
    }

    #[test]
    fn detect_omits_cowork_outside_native_windows() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let ctx = make_context_with_home(
            dir.path(),
            Platform::new(Os::Linux, false),
            MockExecutor::new(),
        )
        .with_env(Arc::new(
            MapEnv::new().with(ONEDRIVE_COMMERCIAL, dir.path()),
        ));

        let targets = ApmTargets::detect(&ctx).expect("detect targets");

        assert!(!targets.includes(CopilotTarget::Cowork));
    }
}
