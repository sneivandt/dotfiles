use anyhow::Result;

use super::{Resource, ResourceChange, ResourceState};
use crate::exec::Executor;

/// Registry key path for Windows Developer Mode.
const DEVELOPER_MODE_KEY: &str = r"HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock";

/// Registry value name for the developer mode flag.
const DEVELOPER_MODE_VALUE: &str = "AllowDevelopmentWithoutDevLicense";

/// Sentinel value returned when the registry value does not exist.
const NOT_FOUND_SENTINEL: &str = "::NOT_FOUND::";

/// A resource for enabling Windows Developer Mode.
///
/// Developer Mode allows symlink creation without administrator privileges.
#[derive(Debug)]
pub struct DeveloperModeResource<'a> {
    /// Executor for running `PowerShell` commands.
    executor: &'a dyn Executor,
}

impl<'a> DeveloperModeResource<'a> {
    /// Create a new developer mode resource.
    #[must_use]
    pub const fn new(executor: &'a dyn Executor) -> Self {
        Self { executor }
    }
}

impl Resource for DeveloperModeResource<'_> {
    fn description(&self) -> String {
        format!("{DEVELOPER_MODE_KEY}\\{DEVELOPER_MODE_VALUE}")
    }

    fn current_state(&self) -> Result<ResourceState> {
        let script = format!(
            "try {{ $v = (Get-ItemProperty -Path '{DEVELOPER_MODE_KEY}' \
             -Name '{DEVELOPER_MODE_VALUE}' -ErrorAction Stop).'{DEVELOPER_MODE_VALUE}'; \
             Write-Output $v }} catch {{ Write-Output '{NOT_FOUND_SENTINEL}' }}"
        );
        let result = self
            .executor
            .run_unchecked("powershell", &["-NoProfile", "-Command", &script])?;
        let output = result.stdout.trim();

        if output == "1" {
            Ok(ResourceState::Correct)
        } else if output == NOT_FOUND_SENTINEL || output.is_empty() {
            Ok(ResourceState::Missing)
        } else {
            Ok(ResourceState::Incorrect {
                current: output.to_string(),
            })
        }
    }

    fn apply(&self) -> Result<ResourceChange> {
        let script = format!(
            "if (!(Test-Path '{DEVELOPER_MODE_KEY}')) \
             {{ New-Item -Path '{DEVELOPER_MODE_KEY}' -Force | Out-Null }}; \
             Set-ItemProperty -Path '{DEVELOPER_MODE_KEY}' \
             -Name '{DEVELOPER_MODE_VALUE}' -Value 1 -Type DWord"
        );
        let result = self
            .executor
            .run_unchecked("powershell", &["-NoProfile", "-Command", &script])?;

        if result.success {
            Ok(ResourceChange::Applied)
        } else {
            Ok(ResourceChange::Skipped {
                reason: result.stderr.trim().to_string(),
            })
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::resources::test_helpers::MockExecutor;

    #[test]
    fn description_contains_key_and_value() {
        let executor = crate::exec::SystemExecutor;
        let resource = DeveloperModeResource::new(&executor);
        let desc = resource.description();
        assert!(desc.contains("AllowDevelopmentWithoutDevLicense"));
        assert!(desc.contains("AppModelUnlock"));
    }

    // ------------------------------------------------------------------
    // current_state
    // ------------------------------------------------------------------

    #[test]
    fn current_state_correct_when_value_is_one() {
        let executor = MockExecutor::with_output(true, "1\n");
        let resource = DeveloperModeResource::new(&executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    }

    #[test]
    fn current_state_missing_when_output_is_sentinel() {
        let executor = MockExecutor::with_output(true, "::NOT_FOUND::\n");
        let resource = DeveloperModeResource::new(&executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    }

    #[test]
    fn current_state_missing_when_output_is_empty() {
        let executor = MockExecutor::with_output(true, "");
        let resource = DeveloperModeResource::new(&executor);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
    }

    #[test]
    fn current_state_incorrect_when_value_is_zero() {
        let executor = MockExecutor::with_output(true, "0\n");
        let resource = DeveloperModeResource::new(&executor);
        assert!(
            matches!(resource.current_state().unwrap(), ResourceState::Incorrect { ref current } if current == "0"),
            "expected Incorrect(0)"
        );
    }

    // ------------------------------------------------------------------
    // apply
    // ------------------------------------------------------------------

    #[test]
    fn apply_returns_applied_when_command_succeeds() {
        let executor = MockExecutor::with_output(true, "");
        let resource = DeveloperModeResource::new(&executor);
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    }

    #[test]
    fn apply_returns_skipped_when_command_fails() {
        let executor = MockExecutor::with_output(false, "");
        let resource = DeveloperModeResource::new(&executor);
        assert!(
            matches!(resource.apply().unwrap(), ResourceChange::Skipped { .. }),
            "expected Skipped when apply fails"
        );
    }
}
