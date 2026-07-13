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

/// Return the cached latest release tag when it is still fresh (less than
/// [`CACHE_MAX_AGE`] seconds old).
pub(super) fn read_fresh_cache(root: &Path) -> Option<String> {
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
    let now = unix_timestamp()?;
    (now.saturating_sub(ts) < CACHE_MAX_AGE).then(|| tag.to_string())
}

/// Write a new cache file with the given tag and current timestamp.
pub(super) fn write_cache(root: &Path, tag: &str) -> Result<()> {
    let now = unix_timestamp().unwrap_or(0);
    fs::write(cache_path(root), format!("{tag}\n{now}\n")).context("writing version cache file")?;
    Ok(())
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
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;

    #[test]
    fn read_fresh_cache_returns_none_when_no_cache() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_fresh_cache(dir.path()), None);
    }

    #[test]
    fn read_fresh_cache_returns_tag_when_recent() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        fs::write(
            bin_dir.join(".dotfiles-version-cache"),
            format!("v1.0\n{now}\n"),
        )
        .unwrap();

        assert_eq!(read_fresh_cache(dir.path()), Some("v1.0".to_string()));
    }

    #[test]
    fn read_fresh_cache_returns_none_when_stale() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let stale = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - CACHE_MAX_AGE
            - 100;
        fs::write(
            bin_dir.join(".dotfiles-version-cache"),
            format!("v1.0\n{stale}\n"),
        )
        .unwrap();

        assert_eq!(read_fresh_cache(dir.path()), None);
    }

    #[test]
    fn read_fresh_cache_returns_none_when_tag_missing() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        fs::write(
            bin_dir.join(".dotfiles-version-cache"),
            format!("\n{now}\n"),
        )
        .unwrap();

        assert_eq!(read_fresh_cache(dir.path()), None);
    }

    #[test]
    fn write_cache_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        write_cache(dir.path(), "v0.1.99").unwrap();
        let content = fs::read_to_string(bin_dir.join(".dotfiles-version-cache")).unwrap();
        assert!(content.starts_with("v0.1.99\n"));
    }
}
