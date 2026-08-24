//! Managed Copilot target detection and platform-specific APM skip messages.

use std::path::PathBuf;

use anyhow::{Context as _, Result};

use crate::engine::Context;

pub(super) const ONEDRIVE_COMMERCIAL: &str = "ONEDRIVECOMMERCIAL";
const ONEDRIVE_CONSUMER: &str = "ONEDRIVE";
const COWORK_OVERRIDE: &str = "APM_COPILOT_COWORK_SKILLS_DIR";

/// A Copilot surface whose deployment is conditional on host availability.
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
            Self::CopilotApp => CopilotDeployment::NativeApm {
                config_key: "copilot_app",
                args: &["install", "-g", "--target", "copilot-app"],
            },
            Self::Cowork => CopilotDeployment::CoworkFileReconcile {
                config_key: "copilot_cowork",
            },
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
            Self::Cowork => "no Cowork skills directory is configured or auto-detected".to_string(),
        }
    }
}

/// How a conditionally available Copilot target converges.
#[derive(Debug, Clone, Copy)]
pub(super) enum CopilotDeployment {
    NativeApm {
        config_key: &'static str,
        args: &'static [&'static str],
    },
    CoworkFileReconcile {
        config_key: &'static str,
    },
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

/// Resolve Cowork's skills directory using the same precedence as APM.
pub(super) fn copilot_cowork_skills_path(ctx: &Context) -> Option<PathBuf> {
    if let Some(path) = ctx
        .env()
        .var_os(COWORK_OVERRIDE)
        .filter(|value| !value.is_empty())
    {
        return Some(PathBuf::from(path));
    }

    let config_path = ctx.home().join(".apm").join("config.json");
    if let Ok(raw) = std::fs::read_to_string(config_path)
        && let Ok(config) = serde_json::from_str::<serde_json::Value>(&raw)
        && let Some(path) = config
            .get("copilot_cowork_skills_dir")
            .and_then(serde_json::Value::as_str)
            .filter(|path| !path.trim().is_empty())
    {
        return Some(PathBuf::from(path));
    }

    if ctx.system().platform().is_windows() {
        for name in [ONEDRIVE_COMMERCIAL, ONEDRIVE_CONSUMER] {
            if let Some(root) = ctx.env().var_os(name).filter(|value| !value.is_empty()) {
                return Some(
                    PathBuf::from(root)
                        .join("Documents")
                        .join("Cowork")
                        .join("skills"),
                );
            }
        }
    }
    None
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
        )
        .with_env(Arc::new(
            MapEnv::new().with(ONEDRIVE_COMMERCIAL, dir.path().join("OneDrive")),
        ));

        let targets = ApmTargets::detect(&ctx).expect("detect targets");

        assert!(targets.includes(CopilotTarget::CopilotApp));
        assert!(targets.includes(CopilotTarget::Cowork));
    }

    #[test]
    fn cowork_path_accepts_apm_override_on_linux() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("Cowork").join("skills");
        let ctx = make_context_with_home(
            dir.path(),
            Platform::new(Os::Linux, false),
            MockExecutor::new(),
        )
        .with_env(Arc::new(MapEnv::new().with(COWORK_OVERRIDE, &path)));

        assert_eq!(copilot_cowork_skills_path(&ctx), Some(path));
    }

    #[test]
    fn cowork_path_accepts_apm_persisted_config() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("Cowork").join("skills");
        std::fs::create_dir_all(dir.path().join(".apm")).expect("create APM config dir");
        std::fs::write(
            dir.path().join(".apm").join("config.json"),
            format!(
                "{{\"copilot_cowork_skills_dir\":{}}}",
                serde_json::to_string(&path).expect("serialize path")
            ),
        )
        .expect("write APM config");
        let ctx = make_context_with_home(
            dir.path(),
            Platform::new(Os::Linux, false),
            MockExecutor::new(),
        )
        .with_env(Arc::new(MapEnv::new()));

        assert_eq!(copilot_cowork_skills_path(&ctx), Some(path));
    }

    #[test]
    fn cowork_path_uses_windows_onedrive_fallback() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let root = dir.path().join("OneDrive");
        let ctx = make_context_with_home(
            dir.path(),
            Platform::new(Os::Windows, false),
            MockExecutor::new(),
        )
        .with_env(Arc::new(MapEnv::new().with(ONEDRIVE_COMMERCIAL, &root)));

        assert_eq!(
            copilot_cowork_skills_path(&ctx),
            Some(root.join("Documents").join("Cowork").join("skills"))
        );
    }

    #[test]
    fn detect_omits_cowork_without_path_configuration() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let ctx = make_context_with_home(
            dir.path(),
            Platform::new(Os::Linux, false),
            MockExecutor::new(),
        )
        .with_env(Arc::new(MapEnv::new()));

        let targets = ApmTargets::detect(&ctx).expect("detect targets");

        assert!(!targets.includes(CopilotTarget::Cowork));
    }
}
