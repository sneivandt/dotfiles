//! Durable marker for extension work deferred from OS provisioning.

use std::path::{Path, PathBuf};

use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::infra::provisioning::{
    DEFERRED_EXTENSIONS_IDS, DEFERRED_EXTENSIONS_MARKER, DeferredExtensionsState,
    deferred_extensions_state,
};

const MARKER_CONTENT: &str = "# VS Code extensions were deferred during Arch chroot provisioning.\n\
# The dotfiles-first-login service removes this marker only after they converge.\n";

/// A marker consumed by the first-session systemd user service.
#[derive(Debug)]
pub struct DeferredExtensionsResource {
    marker: PathBuf,
    assignments: String,
}

impl DeferredExtensionsResource {
    /// Create the marker resource below `home`.
    ///
    /// # Errors
    ///
    /// Returns an error when the overlay path or extension list cannot be encoded.
    pub fn new(
        home: &Path,
        profile: &str,
        overlay: Option<&Path>,
        extensions: &[String],
    ) -> anyhow::Result<Self> {
        let overlay = overlay
            .map_or(Some(""), Path::to_str)
            .ok_or_else(|| anyhow::anyhow!("deferred extensions require a UTF-8 overlay path"))?;
        let ids = serde_json::to_string(extensions)?;
        Ok(Self {
            marker: home.join(DEFERRED_EXTENSIONS_MARKER),
            assignments: format!(
                "DOTFILES_PROFILE={}\nDOTFILES_OVERLAY={}\n{DEFERRED_EXTENSIONS_IDS}={}\n",
                environment_value(profile),
                environment_value(overlay),
                environment_value(&ids),
            ),
        })
    }

    fn marker_contents(&self) -> std::io::Result<Option<String>> {
        match deferred_extensions_state(&self.marker)? {
            DeferredExtensionsState::Pending => std::fs::read_to_string(&self.marker).map(Some),
            DeferredExtensionsState::Absent => Ok(None),
            DeferredExtensionsState::Invalid => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "deferred extensions marker is not a regular file",
            )),
        }
    }
}

// systemd EnvironmentFile double-quoted values preserve whitespace and newlines.
fn environment_value(value: &str) -> String {
    let mut quoted = String::from("\"");
    for character in value.chars() {
        if matches!(character, '\\' | '"' | '$' | '`') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    quoted
}

impl Resource for DeferredExtensionsResource {
    fn description(&self) -> String {
        format!(
            "VS Code extensions until the first user session ({})",
            self.marker.display()
        )
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        let previous = self.marker_contents()?;
        let mut content = previous.as_deref().unwrap_or(MARKER_CONTENT).to_owned();
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(&self.assignments);
        crate::infra::fs::write_atomic_checked(&self.marker, content, || {
            anyhow::ensure!(
                self.marker_contents()? == previous,
                "deferred extensions marker changed; retry"
            );
            Ok(())
        })?;
        Ok(ResourceChange::Applied)
    }
}

impl IntrinsicState for DeferredExtensionsResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        Ok(match deferred_extensions_state(&self.marker)? {
            DeferredExtensionsState::Pending => {
                let content = std::fs::read_to_string(&self.marker)?;
                if content.ends_with(&self.assignments) {
                    ResourceState::Correct
                } else {
                    ResourceState::Incorrect {
                        current: "recorded extension work differs from the current selection"
                            .into(),
                    }
                }
            }
            DeferredExtensionsState::Invalid => ResourceState::Invalid {
                reason: format!("{} is not a regular file", self.marker.display()),
            },
            DeferredExtensionsState::Absent => ResourceState::Missing,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_apply_creates_an_explained_deferral() {
        let home = tempfile::tempdir().unwrap();
        let resource = DeferredExtensionsResource::new(home.path(), "desktop", None, &[]).unwrap();

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
            let resource =
                DeferredExtensionsResource::new(home.path(), "desktop", None, &[]).unwrap();
            std::fs::create_dir_all(resource.marker.parent().unwrap()).unwrap();
            std::fs::write(&resource.marker, content).unwrap();

            assert_eq!(
                resource.current_state().unwrap(),
                ResourceState::Incorrect {
                    current: "recorded extension work differs from the current selection".into()
                },
                "{case}"
            );
            assert_eq!(
                std::fs::read_to_string(&resource.marker).unwrap(),
                content,
                "{case}"
            );
        }

        let home = tempfile::tempdir().unwrap();
        let resource = DeferredExtensionsResource::new(home.path(), "desktop", None, &[]).unwrap();
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
        let resource = DeferredExtensionsResource::new(home.path(), "desktop", None, &[]).unwrap();
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
        let resource = DeferredExtensionsResource::new(home.path(), "desktop", None, &[]).unwrap();

        resource
            .current_state()
            .expect_err("a looping parent is an inspection failure, not an absent marker");
        assert_eq!(std::fs::read_link(parent).unwrap(), Path::new(".local"));
    }
}
