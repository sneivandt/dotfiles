use anyhow::Result;

use super::{Resource, ResourceChange, ResourceState};
use crate::exec::Executor;

/// A systemd user unit resource that can be checked and enabled.
#[derive(Debug)]
pub struct SystemdUnitResource<'a> {
    /// Unit name (e.g. "clean-home-tmp.timer").
    pub name: String,
    /// Executor for running systemctl commands.
    executor: &'a dyn Executor,
}

impl<'a> SystemdUnitResource<'a> {
    /// Create a new systemd unit resource.
    #[must_use]
    pub const fn new(name: String, executor: &'a dyn Executor) -> Self {
        Self { name, executor }
    }

    /// Create from a config entry.
    #[must_use]
    pub fn from_entry(
        entry: &crate::config::systemd_units::SystemdUnit,
        executor: &'a dyn Executor,
    ) -> Self {
        Self::new(entry.name.clone(), executor)
    }
}

impl Resource for SystemdUnitResource<'_> {
    fn description(&self) -> String {
        self.name.clone()
    }

    fn current_state(&self) -> Result<ResourceState> {
        let result = self
            .executor
            .run_unchecked("systemctl", &["--user", "is-enabled", &self.name])?;
        if result.success {
            Ok(ResourceState::Correct)
        } else {
            Ok(ResourceState::Missing)
        }
    }

    fn apply(&self) -> Result<ResourceChange> {
        let result = self
            .executor
            .run_unchecked("systemctl", &["--user", "enable", "--now", &self.name])?;
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
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn description_returns_unit_name() {
        let executor = crate::exec::SystemExecutor;
        let resource = SystemdUnitResource::new("clean-home-tmp.timer".to_string(), &executor);
        assert_eq!(resource.description(), "clean-home-tmp.timer");
    }

    #[test]
    fn from_entry_copies_name() {
        let executor = crate::exec::SystemExecutor;
        let entry = crate::config::systemd_units::SystemdUnit {
            name: "dunst.service".to_string(),
        };
        let resource = SystemdUnitResource::from_entry(&entry, &executor);
        assert_eq!(resource.name, "dunst.service");
    }
}
