use anyhow::Result;

use super::{Resource, ResourceChange, ResourceState};
use crate::exec::Executor;

/// A git config entry resource that can be checked and applied.
pub struct GitConfigResource<'a> {
    /// Config key (e.g., "core.autocrlf").
    pub key: String,
    /// Desired value (e.g., "false").
    pub desired_value: String,
    /// Executor for running git commands.
    executor: &'a dyn Executor,
}

impl std::fmt::Debug for GitConfigResource<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitConfigResource")
            .field("key", &self.key)
            .field("desired_value", &self.desired_value)
            .field("executor", &"<dyn Executor>")
            .finish()
    }
}

impl<'a> GitConfigResource<'a> {
    /// Create a new git config resource.
    #[must_use]
    pub fn new(key: String, desired_value: String, executor: &'a dyn Executor) -> Self {
        Self {
            key,
            desired_value,
            executor,
        }
    }
}

impl Resource for GitConfigResource<'_> {
    fn description(&self) -> String {
        format!("{} = {}", self.key, self.desired_value)
    }

    fn current_state(&self) -> Result<ResourceState> {
        let result = self
            .executor
            .run_unchecked("git", &["config", "--global", "--get", &self.key])?;
        let current = result.stdout.trim().to_string();

        if !result.success || current.is_empty() {
            Ok(ResourceState::Missing)
        } else if current == self.desired_value {
            Ok(ResourceState::Correct)
        } else {
            Ok(ResourceState::Incorrect { current })
        }
    }

    fn apply(&self) -> Result<ResourceChange> {
        self.executor.run(
            "git",
            &["config", "--global", &self.key, &self.desired_value],
        )?;
        Ok(ResourceChange::Applied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn description_format() {
        let executor = crate::exec::SystemExecutor;
        let resource =
            GitConfigResource::new("core.autocrlf".to_string(), "false".to_string(), &executor);
        assert_eq!(resource.description(), "core.autocrlf = false");
    }
}
