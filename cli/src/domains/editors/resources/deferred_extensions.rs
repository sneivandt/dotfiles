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
    fn marker_apply_creates_an_explained_deferral() {
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

    #[test]
    fn marker_discovery_preserves_existing_files_and_rejects_directories() {
        for (case, content) in [
            ("empty marker", ""),
            ("custom marker", "pending extensions\n"),
        ] {
            let home = tempfile::tempdir().unwrap();
            let resource = DeferredExtensionsResource::new(home.path());
            std::fs::create_dir_all(resource.marker.parent().unwrap()).unwrap();
            std::fs::write(&resource.marker, content).unwrap();

            assert_eq!(
                resource.current_state().unwrap(),
                ResourceState::Correct,
                "{case}"
            );
            assert_eq!(
                std::fs::read_to_string(&resource.marker).unwrap(),
                content,
                "{case}"
            );
        }

        let home = tempfile::tempdir().unwrap();
        let resource = DeferredExtensionsResource::new(home.path());
        std::fs::create_dir_all(&resource.marker).unwrap();
        let sentinel = resource.marker.join("user-data");
        std::fs::write(&sentinel, "preserve").unwrap();

        assert!(
            matches!(
                resource.current_state().unwrap(),
                ResourceState::Invalid { reason } if reason.contains("not a regular file")
            ),
            "a directory must not be accepted as a completed marker"
        );
        assert_eq!(std::fs::read_to_string(sentinel).unwrap(), "preserve");
    }

    #[cfg(unix)]
    #[test]
    fn marker_symlinks_are_invalid_even_when_the_target_is_a_regular_file() {
        let home = tempfile::tempdir().unwrap();
        let resource = DeferredExtensionsResource::new(home.path());
        let target = home.path().join("user-marker");
        std::fs::write(&target, "user-owned").unwrap();
        std::fs::create_dir_all(resource.marker.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&target, &resource.marker).unwrap();

        for target_exists in [true, false] {
            if !target_exists {
                std::fs::remove_file(&target).unwrap();
            }
            assert!(
                matches!(
                    resource.current_state().unwrap(),
                    ResourceState::Invalid { .. }
                ),
                "marker discovery must not follow symlinks; target_exists={target_exists}"
            );
            assert_eq!(std::fs::read_link(&resource.marker).unwrap(), target);
        }
    }

    #[cfg(unix)]
    #[test]
    fn marker_inspection_errors_are_not_reported_as_missing() {
        let home = tempfile::tempdir().unwrap();
        let parent = home.path().join(".local");
        std::os::unix::fs::symlink(".local", &parent).unwrap();
        let resource = DeferredExtensionsResource::new(home.path());

        resource
            .current_state()
            .expect_err("a looping parent is an inspection failure, not an absent marker");
        assert_eq!(std::fs::read_link(parent).unwrap(), Path::new(".local"));
    }
}
