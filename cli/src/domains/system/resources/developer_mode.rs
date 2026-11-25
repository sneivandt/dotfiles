//! Windows Developer Mode resource.

use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::infra::platform::{DeveloperModeFlag, developer_mode_flag};

/// Registry key path for Windows Developer Mode (display/description only).
const DEVELOPER_MODE_KEY: &str = r"HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock";

/// Registry value name for the developer mode flag.
const DEVELOPER_MODE_VALUE: &str = "AllowDevelopmentWithoutDevLicense";

/// A resource for enabling Windows Developer Mode.
///
/// Developer Mode allows symlink creation without administrator privileges.
/// Uses the `winreg` crate for native registry access on Windows.
#[derive(Debug)]
pub(crate) struct DeveloperModeResource;

impl DeveloperModeResource {
    /// Create a new developer mode resource.
    #[must_use]
    pub(crate) const fn new() -> Self {
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
            use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_SET_VALUE};
            let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
            let (key, _) = hklm.create_subkey_with_flags(
                crate::infra::platform::DEVELOPER_MODE_SUBKEY,
                KEY_SET_VALUE,
            )?;
            key.set_value(DEVELOPER_MODE_VALUE, &1u32)?;
            Ok(ResourceChange::Applied)
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
    fn current_state(&self) -> ResourceResult<ResourceState> {
        Ok(flag_state(developer_mode_flag()?))
    }
}

fn flag_state(flag: DeveloperModeFlag) -> ResourceState {
    match flag {
        DeveloperModeFlag::Dword(1) => ResourceState::Correct,
        DeveloperModeFlag::Dword(v) => ResourceState::Incorrect {
            current: v.to_string(),
        },
        DeveloperModeFlag::Invalid => ResourceState::Incorrect {
            current: "not a DWORD".into(),
        },
        DeveloperModeFlag::Missing => ResourceState::Missing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readable_wrong_type_or_size_can_be_repaired_then_observed_as_current() {
        for flag in [
            DeveloperModeFlag::from_bytes(false, b"1\0\0\0"),
            DeveloperModeFlag::from_bytes(true, &[1, 0]),
            DeveloperModeFlag::from_bytes(true, &[0; 4]),
        ] {
            assert!(!flag.enabled());
            assert!(matches!(flag_state(flag), ResourceState::Incorrect { .. }));
        }
        let enabled = DeveloperModeFlag::from_bytes(true, &[1, 0, 0, 0]);
        assert!(enabled.enabled());
        assert_eq!(flag_state(enabled), ResourceState::Correct);
    }

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
