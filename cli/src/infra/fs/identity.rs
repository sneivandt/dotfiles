use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

pub(crate) fn path_identity(path: &Path) -> Vec<OsString> {
    path.components()
        .filter(|component| !matches!(component, Component::CurDir))
        .map(|component| {
            let segment = component.as_os_str();
            #[cfg(windows)]
            {
                segment.to_string_lossy().to_lowercase().into()
            }
            #[cfg(not(windows))]
            {
                segment.to_os_string()
            }
        })
        .collect()
}

/// Check ownership against the configured entry rather than the file it resolves to.
pub(crate) fn link_points_to_source(target: &Path, existing: &Path, source: &Path) -> bool {
    let resolved = if existing.is_absolute() {
        existing.to_path_buf()
    } else {
        target
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(existing)
    };
    // Following a different home link here would let removal of that link
    // change whether this one is managed during the same batch.
    let identity = |path: &Path| -> Option<Vec<OsString>> {
        let absolute = std::path::absolute(dunce::simplified(path)).ok()?;
        let mut lexical = PathBuf::new();
        for part in absolute.components() {
            if part == Component::ParentDir {
                lexical.pop();
            } else {
                lexical.push(part);
            }
        }
        Some(path_identity(&lexical))
    };
    let configured = identity(source);
    let canonical_entry = super::resolve_target_entry(source).ok();
    let canonical = canonical_entry.as_deref().and_then(identity);
    let matches_entry = |path: &Path| {
        identity(path).is_some_and(|referenced| {
            (configured.as_ref() == Some(&referenced) || canonical.as_ref() == Some(&referenced))
                && (same_entry(source, path)
                    || canonical_entry
                        .as_deref()
                        .is_some_and(|source| same_entry_with_missing_parent(source, path)))
        })
    };
    #[cfg(windows)]
    {
        matches_entry(&resolved)
            || expand_short_path(&resolved).is_some_and(|expanded| matches_entry(&expanded))
    }
    #[cfg(not(windows))]
    {
        matches_entry(&resolved)
    }
}

fn same_entry_with_missing_parent(source: &Path, target: &Path) -> bool {
    let Some(parent) = target.parent() else {
        return false;
    };
    if !parent
        .symlink_metadata()
        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        return false;
    }
    // A retired path can have missing directories below an existing ancestor
    // whose historical spelling differs. Resolve the ancestor before comparing
    // the missing suffix. Do not follow the final source entry.
    super::resolve_target_entry(target).is_ok_and(|resolved| {
        path_identity(dunce::simplified(&resolved)) == path_identity(dunce::simplified(source))
    })
}

/// Compare entry locations without following the final link.
pub(crate) fn same_entry(source: &Path, target: &Path) -> bool {
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

/// Compare paths, falling back to their original spelling if they cannot be resolved.
pub(crate) fn paths_equal(a: &Path, b: &Path) -> bool {
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

#[cfg(windows)]
fn expand_short_path(path: &Path) -> Option<PathBuf> {
    for prefix in path.ancestors() {
        if let Some(expanded) = long_path_name(prefix) {
            let suffix = path.strip_prefix(prefix).ok()?;
            return Some(expanded.join(suffix));
        }
        // GetLongPathNameW requires existing entries. A retired link source
        // can have a short-name ancestor and a long suffix that is now missing.
        // Keep the suffix verbatim and stop climbing on other lookup failures.
        if !std::fs::symlink_metadata(prefix)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            return None;
        }
    }
    None
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Win32 provides the long spelling of an existing path without resolving links"
)]
fn long_path_name(path: &Path) -> Option<PathBuf> {
    use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
    use windows_sys::Win32::Storage::FileSystem::GetLongPathNameW;

    let mut raw: Vec<_> = path.as_os_str().encode_wide().collect();
    if raw.contains(&0) {
        return None;
    }
    raw.push(0);
    // SAFETY: The input is NUL-terminated and an empty output queries capacity.
    let capacity = unsafe { GetLongPathNameW(raw.as_ptr(), std::ptr::null_mut(), 0) };
    if capacity == 0 {
        return None;
    }
    let mut long = vec![0; usize::try_from(capacity).ok()?];
    // SAFETY: The input is still valid and long contains capacity writable u16s.
    let written = unsafe { GetLongPathNameW(raw.as_ptr(), long.as_mut_ptr(), capacity) };
    if written == 0 || written >= capacity {
        return None;
    }
    long.truncate(usize::try_from(written).ok()?);
    Some(OsString::from_wide(&long).into())
}
