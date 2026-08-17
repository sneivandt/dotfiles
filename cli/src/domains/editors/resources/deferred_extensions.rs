//! Durable marker for extension work deferred from OS provisioning.

use std::path::{Path, PathBuf};

use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};

/// Marker path relative to the user's home directory.
pub const MARKER_RELATIVE_PATH: &str = ".local/state/dotfiles/deferred-vscode-extensions";

const MARKER_CONTENT: &str = "VS Code extensions were deferred during Arch chroot provisioning.\n\
The dotfiles-first-login service removes this marker only after they converge.\n";

/// A marker consumed by the first-session systemd user service.
#[derive(Debug)]
pub struct DeferredExtensionsResource {
    marker: PathBuf,
}

impl DeferredExtensionsResource {
    /// Create the marker resource below `home`.
    #[must_use]
    pub fn new(home: &Path) -> Self {
        Self {
            marker: home.join(MARKER_RELATIVE_PATH),
        }
    }
}

impl Resource for DeferredExtensionsResource {
    fn description(&self) -> String {
        format!(
            "VS Code extensions until the first user session ({})",
            self.marker.display()
        )
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        if let Some(parent) = self.marker.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::infra::fs::write_atomic(&self.marker, MARKER_CONTENT)?;
        Ok(ResourceChange::Applied)
    }
}

impl IntrinsicState for DeferredExtensionsResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        match std::fs::symlink_metadata(&self.marker) {
            Ok(metadata) if metadata.is_file() => Ok(ResourceState::Correct),
            Ok(_) => Ok(ResourceState::Invalid {
                reason: format!("{} is not a regular file", self.marker.display()),
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(ResourceState::Missing)
            }
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_converges_idempotently() {
        let home = tempfile::tempdir().unwrap();
        let resource = DeferredExtensionsResource::new(home.path());

        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
        assert!(
            std::fs::read_to_string(&resource.marker)
                .unwrap()
                .contains("dotfiles-first-login service"),
            "the marker should explain who consumes it"
        );
    }
}
