//! Declared-mode conflict resolution and exclusion target helpers for chmod.

use anyhow::Result;
use std::path::{Path, PathBuf};

pub(super) fn error_is_permission_denied(error: &(dyn std::error::Error + 'static)) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::PermissionDenied)
        || error.source().is_some_and(error_is_permission_denied)
}

pub(super) fn resolve_declared_targets(modes: &[(PathBuf, u32)]) -> Result<Vec<Option<PathBuf>>> {
    modes
        .iter()
        .map(|(path, _)| resolve_declared_target(path))
        .collect()
}

pub(super) fn check_resolved_mode_conflicts(
    modes: &[(PathBuf, u32)],
    resolved: &[Option<PathBuf>],
) -> Result<()> {
    let mut seen = std::collections::HashMap::new();
    for ((path, mode), target) in modes.iter().zip(resolved) {
        if let Some(target) = target
            && let Some((previous_mode, previous_path)) = seen.insert(target.clone(), (mode, path))
        {
            anyhow::ensure!(
                previous_mode == mode,
                "conflicting permission modes for {} and {} resolving to {}; keep one mode for this destination",
                previous_path.display(),
                path.display(),
                target.display()
            );
        }
    }
    Ok(())
}

fn resolve_declared_target(path: &Path) -> Result<Option<PathBuf>> {
    let resolved = (|| {
        if path.try_exists()? {
            crate::infra::fs::canonicalize(path)
        } else {
            crate::infra::fs::resolve_target_entry(path)
        }
    })();
    match resolved {
        Err(error) if error_is_permission_denied(error.as_ref()) => Ok(None),
        result => result.map(Some),
    }
}

// An unresolved suffix is only an exclusion/order hint, never permission to
// mutate a destination. Each resource still validates its fully resolved target.
pub(super) fn exclusion_target(path: &Path) -> Result<PathBuf> {
    exclusion_target_with(path, crate::infra::fs::canonicalize)
}

pub(in crate::domains::files::resources) fn exclusion_target_with(
    path: &Path,
    canonicalize: impl Fn(&Path) -> Result<PathBuf>,
) -> Result<PathBuf> {
    if let Some(target) = resolve_declared_target(path)? {
        return Ok(target);
    }
    for ancestor in path.ancestors().skip(1) {
        match canonicalize(ancestor) {
            Ok(resolved) => return Ok(resolved.join(path.strip_prefix(ancestor)?)),
            Err(error) if error_is_permission_denied(error.as_ref()) => {}
            Err(error) => return Err(error),
        }
    }
    anyhow::bail!("cannot resolve chmod exclusion {}", path.display())
}
