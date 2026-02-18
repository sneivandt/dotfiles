use anyhow::Result;

use super::{Resource, ResourceChange, ResourceState};
use crate::exec;

/// A systemd user unit resource that can be checked and enabled.
#[derive(Debug, Clone)]
pub struct SystemdUnitResource {
    /// Unit name (e.g. "clean-home-tmp.timer").
    pub name: String,
}

impl SystemdUnitResource {
    /// Create a new systemd unit resource.
    #[must_use]
    pub const fn new(name: String) -> Self {
        Self { name }
    }

    /// Create from a config entry.
    #[must_use]
    pub fn from_entry(entry: &crate::config::systemd_units::SystemdUnit) -> Self {
        Self::new(entry.name.clone())
    }
}

impl Resource for SystemdUnitResource {
    fn description(&self) -> String {
        self.name.clone()
    }

    fn current_state(&self) -> Result<ResourceState> {
        let result = exec::run_unchecked("systemctl", &["--user", "is-enabled", &self.name])?;
        if result.success {
            Ok(ResourceState::Correct)
        } else {
            Ok(ResourceState::Missing)
        }
    }

    fn apply(&self) -> Result<ResourceChange> {
        let result = exec::run_unchecked("systemctl", &["--user", "enable", "--now", &self.name])?;
        if result.success {
            Ok(ResourceChange::Applied)
        } else {
            Ok(ResourceChange::Skipped {
                reason: format!("failed to enable: {}", result.stderr.trim()),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn description_returns_unit_name() {
        let resource = SystemdUnitResource::new("clean-home-tmp.timer".to_string());
        assert_eq!(resource.description(), "clean-home-tmp.timer");
    }

    #[test]
    fn from_entry_copies_name() {
        let entry = crate::config::systemd_units::SystemdUnit {
            name: "dunst.service".to_string(),
        };
        let resource = SystemdUnitResource::from_entry(&entry);
        assert_eq!(resource.name, "dunst.service");
    }
}
