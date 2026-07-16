//! Persisted profile selection.

use std::path::Path;

use anyhow::Result;

const PROFILE_KEY: &str = "dotfiles.profile";

/// Try to read the profile from the repository's local Git config.
#[must_use]
pub fn read_persisted(root: &Path) -> Option<String> {
    crate::infra::config::git_state::read_local(root, PROFILE_KEY)
}

/// Persist the profile name to the repository's local Git config.
///
/// # Errors
///
/// Returns an error if the repository cannot be discovered or the config
/// cannot be written.
pub fn persist(root: &Path, name: &str) -> Result<()> {
    crate::infra::config::git_state::persist_local(root, PROFILE_KEY, name)
}
