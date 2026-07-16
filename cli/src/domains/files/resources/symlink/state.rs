use anyhow::Result;
use std::path::Path;

use super::SymlinkResource;
use super::platform::is_link_like;
use crate::engine::{ResourceResult, ResourceState};

pub(super) fn pre_apply_warning(target: &Path) -> ResourceResult<Option<String>> {
    let metadata = crate::infra::fs::symlink_metadata_optional(target, "stat target")?;
    Ok(metadata
        .filter(|meta| !is_link_like(target, meta))
        .map(|_| {
            format!(
                "replacing existing non-symlink target without backup: {}",
                target.display()
            )
        }))
}

pub(super) fn current_state(resource: &SymlinkResource) -> Result<ResourceState> {
    if let Some(reason) = &resource.validation_error {
        return Ok(ResourceState::Invalid {
            reason: reason.clone(),
        });
    }

    if let Some(reason) = crate::infra::fs::missing_source_reason(&resource.source) {
        return Ok(ResourceState::Invalid { reason });
    }

    std::fs::read_link(&resource.target).map_or_else(
        |_| match crate::infra::fs::symlink_metadata_optional(&resource.target, "stat target")? {
            Some(_) => Ok(ResourceState::Incorrect {
                current: "target is a regular file or dangling symlink".to_string(),
            }),
            None => Ok(ResourceState::Missing),
        },
        |existing| {
            if paths_equal(&existing, &resource.source) {
                Ok(ResourceState::Correct)
            } else {
                Ok(ResourceState::Incorrect {
                    current: format!("points to {}", existing.display()),
                })
            }
        },
    )
}

/// Compare two paths for equality, canonicalizing when possible.
///
/// Attempts `fs::canonicalize` on both paths so that symlinks in the path,
/// case differences (Windows), and `\\?\` UNC prefixes are resolved before
/// comparison. Falls back to raw comparison when canonicalization fails
/// (e.g., dangling paths).
pub(super) fn paths_equal(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }

    let canon_a = std::fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf());
    let canon_b = std::fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf());

    #[cfg(windows)]
    {
        let sa = canon_a.to_string_lossy().to_lowercase();
        let sb = canon_b.to_string_lossy().to_lowercase();
        sa == sb
    }

    #[cfg(not(windows))]
    {
        canon_a == canon_b
    }
}
