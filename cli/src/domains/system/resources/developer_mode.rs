//! Windows Developer Mode resource.
use anyhow::Result;

use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};

/// Registry key path for Windows Developer Mode (display/description only).
const DEVELOPER_MODE_KEY: &str = r"HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock";

/// Registry value name for the developer mode flag.
const DEVELOPER_MODE_VALUE: &str = "AllowDevelopmentWithoutDevLicense";

/// A resource for enabling Windows Developer Mode.
///
/// Developer Mode allows symlink creation without administrator privileges.
/// Uses the `winreg` crate for native registry access on Windows.
#[derive(Debug)]
pub struct DeveloperModeResource;

impl DeveloperModeResource {
    /// Create a new developer mode resource.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for DeveloperModeResource {
    fn default() -> Self {
        Self::new()
    }
}

impl Resource for DeveloperModeResource {
    fn description(&self) -> String {
        format!("{DEVELOPER_MODE_KEY}\\{DEVELOPER_MODE_VALUE}")
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        #[cfg(windows)]
        {
            use winreg::RegKey;
            use winreg::enums::HKEY_LOCAL_MACHINE;
            let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
            match hklm.create_subkey(crate::infra::platform::DEVELOPER_MODE_SUBKEY) {
                Ok((key, _)) => match key.set_value(DEVELOPER_MODE_VALUE, &1u32) {
                    Ok(()) => Ok(ResourceChange::Applied),
                    Err(e) => Ok(ResourceChange::Skipped {
                        reason: e.to_string(),
                    }),
                },
                Err(e) => Ok(ResourceChange::Skipped {
                    reason: e.to_string(),
                }),
            }
        }
        #[cfg(not(windows))]
        {
            Err(crate::engine::resource::ResourceError::not_supported(
                "developer mode is only supported on Windows",
            ))
        }
    }
}

impl IntrinsicState for DeveloperModeResource {
    fn current_state(&self) -> Result<ResourceState> {
        #[cfg(windows)]
        {
            match crate::infra::platform::developer_mode_flag()? {
                Some(1) => Ok(ResourceState::Correct),
                Some(v) => Ok(ResourceState::Incorrect {
                    current: v.to_string(),
                }),
                None => Ok(ResourceState::Missing),
            }
        }
        #[cfg(not(windows))]
        {
            Ok(ResourceState::Missing)
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;

    #[test]
    fn description_contains_key_and_value() {
        let resource = DeveloperModeResource::new();
        let desc = resource.description();
        assert!(desc.contains("AllowDevelopmentWithoutDevLicense"));
        assert!(desc.contains("AppModelUnlock"));
    }

    #[test]
    #[cfg(not(windows))]
    fn apply_is_not_supported_off_windows() {
        let error = DeveloperModeResource::new()
            .apply()
            .expect_err("developer mode should not be appliable off Windows");
        assert!(
            matches!(
                error,
                crate::engine::resource::ResourceError::NotSupported { .. }
            ),
            "expected NotSupported, got {error:?}"
        );
    }

    #[test]
    #[cfg(not(windows))]
    fn current_state_is_missing_off_windows() {
        assert_eq!(
            DeveloperModeResource::new().current_state().unwrap(),
            ResourceState::Missing,
            "off Windows the flag can never be observed as set"
        );
    }
}
