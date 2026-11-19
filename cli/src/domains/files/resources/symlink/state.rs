use std::path::Path;

use super::SymlinkResource;
use crate::domains::files::config::symlinks::InspectedSource;
use crate::engine::{ResourceResult, ResourceState};
use crate::infra::fs::{link_points_to_source, paths_equal, same_entry};

pub(super) fn current_state(
    resource: &SymlinkResource,
    target: &Path,
) -> ResourceResult<ResourceState> {
    if let Some(reason) = invalid_source_reason(resource, target)? {
        return Ok(ResourceState::Invalid { reason });
    }

    std::fs::read_link(target).map_or_else(
        |_| match crate::infra::fs::symlink_metadata_optional(target, "stat target")? {
            Some(_) if paths_equal(target, &resource.source) => Ok(ResourceState::Invalid {
                reason: "target resolves to the source instead of a separate managed link"
                    .to_string(),
            }),
            // `read_link` succeeds for a dangling symlink, so reaching here with
            // metadata present means the target is not a symlink at all.
            Some(_) => Ok(ResourceState::Invalid {
                reason: "target already exists; move it aside before installing the link"
                    .to_string(),
            }),
            None => Ok(ResourceState::Missing),
        },
        |existing| {
            if link_points_to_source(target, &existing, &resource.source) {
                Ok(ResourceState::Correct)
            } else if resource.legacy_link(target, &existing)?.is_some() {
                Ok(ResourceState::Incorrect {
                    current: format!("historical managed link to {}", existing.display()),
                })
            } else {
                Ok(ResourceState::Invalid {
                    reason: format!(
                        "target already points to {}; move it aside before installing the link",
                        existing.display()
                    ),
                })
            }
        },
    )
}

pub(super) fn invalid_source_reason(
    resource: &SymlinkResource,
    target: &Path,
) -> ResourceResult<Option<String>> {
    if let Some(reason) = &resource.validation_error {
        return Ok(Some(reason.clone()));
    }
    match resource.inspect_source()? {
        InspectedSource::Available(_) => {}
        InspectedSource::Missing(reason) | InspectedSource::Outside(reason) => {
            return Ok(Some(reason));
        }
    }
    Ok(same_entry(&resource.source, target)
        .then(|| "target aliases the source entry through its parent directory".to_string()))
}
