//! Windows Developer Mode resource.
use anyhow::Result;

use super::{Applicable, Resource, ResourceChange, ResourceState};

/// Registry key path for Windows Developer Mode (display/description only).
const DEVELOPER_MODE_KEY: &str = r"HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock";

/// Registry subkey path for native Windows registry access.
#[cfg(windows)]
const DEVELOPER_MODE_SUBKEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock";

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

impl Applicable for DeveloperModeResource {
    fn description(&self) -> String {
        format!("{DEVELOPER_MODE_KEY}\\{DEVELOPER_MODE_VALUE}")
    }

    fn apply(&self) -> Result<ResourceChange> {
        #[cfg(windows)]
        {
            use winreg::RegKey;
            use winreg::enums::HKEY_LOCAL_MACHINE;
            let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
            match hklm.create_subkey(DEVELOPER_MODE_SUBKEY) {
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
            anyhow::bail!("developer mode is only supported on Windows")
        }
    }
}

impl Resource for DeveloperModeResource {
    fn current_state(&self) -> Result<ResourceState> {
        #[cfg(windows)]
        {
            use winreg::RegKey;
            use winreg::enums::HKEY_LOCAL_MACHINE;
            let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
            match hklm.open_subkey(DEVELOPER_MODE_SUBKEY) {
                Ok(key) => match key.get_value::<u32, _>(DEVELOPER_MODE_VALUE) {
                    Ok(1) => Ok(ResourceState::Correct),
                    Ok(v) => Ok(ResourceState::Incorrect {
                        current: v.to_string(),
                    }),
                    Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {
                        Ok(ResourceState::Missing)
                    }
                    Err(e) => Err(e.into()),
                },
                Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {
                    Ok(ResourceState::Missing)
                }
                Err(e) => Err(e.into()),
            }
        }
        #[cfg(not(windows))]
        {
            Ok(ResourceState::Missing)
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn description_contains_key_and_value() {
        let resource = DeveloperModeResource::new();
        let desc = resource.description();
        assert!(desc.contains("AllowDevelopmentWithoutDevLicense"));
        assert!(desc.contains("AppModelUnlock"));
    }
}
