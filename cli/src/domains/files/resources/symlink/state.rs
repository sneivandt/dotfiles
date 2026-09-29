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

pub(super) fn current_state(resource: &SymlinkResource) -> ResourceResult<ResourceState> {
    if let Some(reason) = &resource.validation_error {
        return Ok(ResourceState::Invalid {
            reason: reason.clone(),
        });
    }

    if let Some(reason) = crate::infra::fs::missing_source_reason(&resource.source) {
        return Ok(ResourceState::Invalid { reason });
    }

    if same_entry(&resource.source, &resource.target) {
        return Ok(ResourceState::Invalid {
            reason: "target aliases the source entry through its parent directory".to_string(),
        });
    }

    std::fs::read_link(&resource.target).map_or_else(
        |_| match crate::infra::fs::symlink_metadata_optional(&resource.target, "stat target")? {
            Some(_) if paths_equal(&resource.target, &resource.source) => {
                Ok(ResourceState::Invalid {
                    reason: "target resolves to the source instead of a separate managed link"
                        .to_string(),
                })
            }
            // `read_link` succeeds for a dangling symlink, so reaching here with
            // metadata present means the target is not a symlink at all.
            Some(_) => Ok(ResourceState::Incorrect {
                current: "target is a regular file or directory".to_string(),
            }),
            None => Ok(ResourceState::Missing),
        },
        |existing| {
            if link_points_to_source(&resource.target, &existing, &resource.source) {
                Ok(ResourceState::Correct)
            } else {
                Ok(ResourceState::Incorrect {
                    current: format!("points to {}", existing.display()),
                })
            }
        },
    )
}

pub(super) fn link_points_to_source(target: &Path, existing: &Path, source: &Path) -> bool {
    let resolved = if existing.is_absolute() {
        existing.to_path_buf()
    } else {
        target
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(existing)
    };
    paths_equal(&resolved, source)
}

/// Compare entry locations without following the final link.
pub(super) fn same_entry(source: &Path, target: &Path) -> bool {
    let same_name =
        source
            .file_name()
            .zip(target.file_name())
            .is_some_and(|(source_name, target_name)| {
                #[cfg(windows)]
                {
                    source_name.to_string_lossy().to_lowercase()
                        == target_name.to_string_lossy().to_lowercase()
                }
                #[cfg(not(windows))]
                {
                    source_name == target_name
                }
            });
    same_name
        && paths_equal(
            source.parent().unwrap_or_else(|| Path::new(".")),
            target.parent().unwrap_or_else(|| Path::new(".")),
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
