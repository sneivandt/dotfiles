use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context as _, Result};

/// Lexical normalization of configuration keys, not evidence of file ownership.
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

/// Ask the filesystem for the entry's spelling, without following a final
/// symlink or conflating hard links. Keep native code units, including non-UTF-8.
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Win32 returns the stored entry spelling from a non-following handle"
)]
pub(crate) fn stored_entry_name(path: &Path) -> Result<Option<OsString>> {
    use std::os::windows::{ffi::OsStringExt as _, fs::OpenOptionsExt as _, io::AsRawHandle as _};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        GetFinalPathNameByHandleW,
    };

    let entry = match std::fs::File::options()
        .access_mode(FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
    {
        Ok(entry) => entry,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("opening entry {}", path.display()));
        }
    };
    let mut wide = vec![0; 256];
    loop {
        let capacity = u32::try_from(wide.len())?;
        // SAFETY: the live handle and writable buffer are valid for this call.
        let length = unsafe {
            GetFinalPathNameByHandleW(entry.as_raw_handle(), wide.as_mut_ptr(), capacity, 0)
        };
        if length == 0 {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("resolving entry {}", path.display()));
        }
        if length < capacity {
            wide.truncate(usize::try_from(length)?);
            let name = wide
                .rsplit(|&ch| ch == u16::from(b'\\'))
                .next()
                .filter(|name| !name.is_empty())
                .context("missing stored entry name")?;
            return Ok(Some(OsString::from_wide(name)));
        }
        wide.resize(usize::try_from(length)?, 0);
    }
}

/// The key for one entry in a known directory. An absent entry has no aliases
/// we can prove: retain the declared spelling.
pub(crate) fn entry_name(path: &Path) -> Result<OsString> {
    #[cfg(windows)]
    if let Some(name) = stored_entry_name(path)? {
        return Ok(name);
    }
    #[cfg(not(windows))]
    super::symlink_metadata_optional(path, "inspect entry")?;
    Ok(path.file_name().context("expected an entry name")?.into())
}

fn entry_location(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    Ok(super::resolve_directory(parent)?.join(entry_name(path)?))
}

/// Collapse a parent step only after proving the component it cancels is an
/// ordinary directory. Inspect the raw components before `absolute` or a
/// Windows verbatim-path join can discard the traversal.
fn checked_join(base: &Path, path: &Path) -> Result<Option<PathBuf>> {
    let parts = (!path.is_absolute())
        .then_some(base)
        .into_iter()
        .chain(std::iter::once(path))
        .flat_map(Path::components);
    if !parts.clone().any(|part| part == Component::ParentDir) {
        return Ok(Some(base.join(path)));
    }
    let mut route = std::path::absolute(".")?;
    for part in parts {
        if part == Component::ParentDir {
            let metadata = route
                .symlink_metadata()
                .with_context(|| format!("inspect parent traversal at {}", route.display()))?;
            if !metadata.is_dir() {
                return Ok(None);
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt as _;
                use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
                if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    return Ok(None);
                }
            }
            route.pop();
        } else {
            route.push(part);
        }
    }
    Ok(Some(route))
}

/// Preserve the route to the entry, including intervening directory links.
/// Windows can use another stored spelling (including an 8.3 name) for any
/// *existing* component, even above a now-missing suffix.
fn spelled_route(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(dunce::simplified(path))?;
    let mut route = PathBuf::new();
    for component in absolute.components() {
        #[cfg(windows)]
        if let Component::Normal(name) = component {
            route.push(entry_name(&route.join(name))?);
            continue;
        }
        route.push(component);
    }
    Ok(route)
}

/// Check the configured entry rather than the referent. A different home link
/// in the route must not become managed just because it currently resolves to
/// the same location: removing it changes the meaning of the stored link.
pub(crate) fn link_points_to_source(target: &Path, existing: &Path, source: &Path) -> Result<bool> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let Some(referenced) = checked_join(parent, existing)? else {
        return Ok(false);
    };
    let Some(declared) = checked_join(Path::new(""), source)? else {
        return Ok(false);
    };
    let configured = entry_location(source)?;
    // Also verify the original lookup: being able to stat a plain directory
    // does not guarantee permission to traverse it.
    let lookup = parent.join(existing);
    if lookup.file_name().is_none() || entry_location(&lookup)? != configured {
        return Ok(false);
    }
    if referenced == declared || referenced == configured {
        return Ok(true);
    }
    let route = spelled_route(&referenced)?;
    Ok(route == spelled_route(&declared)? || route == spelled_route(&configured)?)
}

/// Compare locations without following the final entry.
pub(crate) fn same_entry(source: &Path, target: &Path) -> Result<bool> {
    if source.file_name().is_none() || target.file_name().is_none() {
        return Ok(false);
    }
    let source_entry =
        entry_location(source).with_context(|| format!("stat source: {}", source.display()))?;
    let target_entry =
        entry_location(target).with_context(|| format!("stat target: {}", target.display()))?;
    Ok(source_entry == target_entry)
}

/// Follow both paths, comparing canonical spellings and any missing suffix
/// verbatim. Do not turn discovery failures into an ownership decision.
pub(crate) fn paths_equal(a: &Path, b: &Path) -> Result<bool> {
    Ok(super::resolve_directory(a)? == super::resolve_directory(b)?)
}
