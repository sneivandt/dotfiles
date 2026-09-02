//! Version-check cache: tracks the latest known release tag and the time it
//! was fetched so the next run can skip redundant network calls.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

/// Maximum age (in seconds) before a version check is performed again.
pub(super) const CACHE_MAX_AGE: u64 = 3600;

/// Path to the version-check cache file.
pub(super) fn cache_path(root: &Path) -> PathBuf {
    root.join("bin").join(".dotfiles-version-cache")
}

/// Return the cached latest release tag when it is from the current boot and
/// still fresh (less than [`CACHE_MAX_AGE`] seconds old).
///
/// Linux boot identifiers prevent a saved pre-NTP clock from making an entry
/// from a previous boot look fresh. A timestamp in the future is also stale.
pub(super) fn read_fresh_cache(root: &Path) -> Option<String> {
    let now = unix_timestamp()?;
    let boot_id = current_boot_id();
    read_fresh_cache_at(root, now, boot_id.as_deref())
}

fn read_fresh_cache_at(root: &Path, now: u64, current_boot_id: Option<&str>) -> Option<String> {
    let path = cache_path(root);
    let Ok(content) = fs::read_to_string(&path) else {
        return None;
    };
    let mut lines = content.lines();
    let tag = lines.next().map(str::trim).filter(|tag| !tag.is_empty())?;
    let ts_str = lines.next()?;
    let Ok(ts) = ts_str.trim().parse::<u64>() else {
        return None;
    };
    let cached_boot_id = lines.next().map(str::trim).filter(|id| !id.is_empty());
    if current_boot_id.is_some() && cached_boot_id != current_boot_id {
        return None;
    }
    (now.checked_sub(ts)? < CACHE_MAX_AGE).then(|| tag.to_string())
}

/// Write a new cache file with the given tag and current timestamp.
pub(super) fn write_cache(root: &Path, tag: &str) -> Result<()> {
    let now = unix_timestamp().unwrap_or(0);
    let boot_id = current_boot_id();
    write_cache_at(root, tag, now, boot_id.as_deref())
}

fn write_cache_at(root: &Path, tag: &str, now: u64, boot_id: Option<&str>) -> Result<()> {
    let content = boot_id.map_or_else(
        || format!("{tag}\n{now}\n"),
        |boot_id| format!("{tag}\n{now}\n{boot_id}\n"),
    );
    fs::write(cache_path(root), content).context("writing version cache file")?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn current_boot_id() -> Option<String> {
    fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
}

#[cfg(not(target_os = "linux"))]
fn current_boot_id() -> Option<String> {
    None
}

/// Return the current UTC time as seconds since the Unix epoch.
///
/// Returns `None` if the system clock is before the epoch, ensuring callers
/// treat this as a stale/missing timestamp rather than a "fresh" zero value.
fn unix_timestamp() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .ok()
        .filter(|&t| t > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_fresh_cache_returns_none_when_no_cache() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_fresh_cache_at(dir.path(), 100, Some("boot-a")), None);
    }

    #[test]
    fn read_fresh_cache_returns_tag_when_recent() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        write_cache_at(dir.path(), "v1.0", 100, Some("boot-a")).unwrap();

        assert_eq!(
            read_fresh_cache_at(dir.path(), 101, Some("boot-a")),
            Some("v1.0".to_string())
        );
    }

    #[test]
    fn read_fresh_cache_returns_none_when_stale() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        write_cache_at(dir.path(), "v1.0", 100, Some("boot-a")).unwrap();

        assert_eq!(
            read_fresh_cache_at(dir.path(), 100 + CACHE_MAX_AGE, Some("boot-a")),
            None
        );
    }

    #[test]
    fn read_fresh_cache_returns_none_when_tag_missing() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        fs::write(bin_dir.join(".dotfiles-version-cache"), "\n100\nboot-a\n").unwrap();

        assert_eq!(read_fresh_cache_at(dir.path(), 101, Some("boot-a")), None);
    }

    #[test]
    fn read_fresh_cache_returns_none_when_timestamp_is_in_the_future() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        write_cache_at(dir.path(), "v1.0", 200, Some("boot-a")).unwrap();

        assert_eq!(
            read_fresh_cache_at(dir.path(), 100, Some("boot-a")),
            None,
            "a future timestamp must be treated as stale, not permanently fresh"
        );
    }

    #[test]
    fn cache_from_a_previous_boot_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("bin")).unwrap();
        write_cache_at(dir.path(), "v1.0", 100, Some("boot-a")).unwrap();

        assert_eq!(
            read_fresh_cache_at(dir.path(), 101, Some("boot-b")),
            None,
            "wall-clock freshness cannot make a previous boot's cache current"
        );
    }

    #[test]
    fn legacy_cache_is_stale_when_a_boot_id_is_available() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::write(bin_dir.join(".dotfiles-version-cache"), "v1.0\n100\n").unwrap();

        assert_eq!(read_fresh_cache_at(dir.path(), 101, Some("boot-a")), None);
        assert_eq!(
            read_fresh_cache_at(dir.path(), 101, None),
            Some("v1.0".to_string()),
            "platforms without a boot identifier retain the timestamp policy"
        );
    }

    #[test]
    fn write_cache_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        write_cache_at(dir.path(), "v2026.07.25-99", 100, Some("boot-a")).unwrap();
        let content = fs::read_to_string(bin_dir.join(".dotfiles-version-cache")).unwrap();
        assert_eq!(content, "v2026.07.25-99\n100\nboot-a\n");
    }
}
