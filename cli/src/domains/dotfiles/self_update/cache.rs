//! Version-check cache: tracks the latest known release tag and the time it
//! was fetched so the next run can skip redundant network calls.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

/// Maximum age (in seconds) before a version check is performed again.
pub(super) const CACHE_MAX_AGE: u64 = 600;

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
    if lines.next()? != super::REPO {
        return None;
    }
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

/// Persist an advisory cache without invalidating a completed check or update.
pub(super) fn write_cache_best_effort(root: &Path, tag: &str) {
    if let Err(error) = write_cache(root, tag) {
        tracing::warn!("could not persist release-check cache: {error:#}");
    }
}

fn write_cache_at(root: &Path, tag: &str, now: u64, boot_id: Option<&str>) -> Result<()> {
    let content = boot_id.map_or_else(
        || format!("{}\n{tag}\n{now}\n", super::REPO),
        |boot_id| format!("{}\n{tag}\n{now}\n{boot_id}\n", super::REPO),
    );
    crate::infra::fs::write_atomic(&cache_path(root), content)
        .context("writing version cache file")?;
    Ok(())
}

#[cfg_attr(
    not(target_os = "linux"),
    allow(
        clippy::missing_const_for_fn,
        reason = "the shared boot-ID function reads the kernel boot ID on Linux"
    )
)]
fn current_boot_id() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        read_boot_id(Path::new("/proc/sys/kernel/random/boot_id"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn read_boot_id(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
}

/// Return the current UTC time as seconds since the Unix epoch.
///
/// Returns `None` if the system clock is before the epoch, ensuring callers
/// treat this as a stale/missing timestamp rather than a "fresh" zero value.
fn unix_timestamp() -> Option<u64> {
    unix_timestamp_from(std::time::SystemTime::now())
}

fn unix_timestamp_from(now: std::time::SystemTime) -> Option<u64> {
    now.duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .ok()
        .filter(|&t| t > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writing_a_cache_records_the_current_time_and_platform_boot_id() {
        assert_eq!(unix_timestamp_from(std::time::UNIX_EPOCH), None);
        assert_eq!(
            unix_timestamp_from(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1)),
            Some(1)
        );
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("bin")).unwrap();
        write_cache(dir.path(), "v1.0").unwrap();
        let content = fs::read_to_string(cache_path(dir.path())).unwrap();
        let mut lines = content.lines();
        assert_eq!(lines.next(), Some(super::super::REPO));
        assert_eq!(lines.next(), Some("v1.0"));
        let timestamp = lines.next().unwrap().parse::<u64>().unwrap();
        assert!(timestamp >= 1_700_000_000, "{content}");
        #[cfg(target_os = "linux")]
        {
            let kernel_id = fs::read_to_string("/proc/sys/kernel/random/boot_id").ok();
            assert_eq!(lines.next(), kernel_id.as_deref().map(str::trim));
        }
        #[cfg(not(target_os = "linux"))]
        assert_eq!(lines.next(), None);
        assert_eq!(read_fresh_cache(dir.path()).as_deref(), Some("v1.0"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_boot_id_reader_ignores_missing_and_blank_ids() {
        let dir = tempfile::tempdir().unwrap();
        let boot_id = dir.path().join("boot_id");
        assert_eq!(read_boot_id(&boot_id), None);
        fs::write(&boot_id, " \n").unwrap();
        assert_eq!(read_boot_id(&boot_id), None);
        fs::write(&boot_id, "fixture-boot\n").unwrap();
        assert_eq!(read_boot_id(&boot_id).as_deref(), Some("fixture-boot"));
    }
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
    fn legacy_cache_without_a_publisher_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::write(bin_dir.join(".dotfiles-version-cache"), "v1.0\n100\n").unwrap();

        assert_eq!(read_fresh_cache_at(dir.path(), 101, Some("boot-a")), None);
        assert_eq!(
            read_fresh_cache_at(dir.path(), 101, None),
            None,
            "legacy caches do not identify their release repository"
        );
    }

    #[test]
    fn cache_from_another_publisher_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("bin")).unwrap();
        fs::write(
            cache_path(dir.path()),
            "another/publisher\nv2026.07.25-99\n100\nboot-a\n",
        )
        .unwrap();
        assert_eq!(read_fresh_cache_at(dir.path(), 101, Some("boot-a")), None);
        assert_eq!(read_fresh_cache_at(dir.path(), 101, None), None);
    }

    #[test]
    fn write_cache_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        write_cache_at(dir.path(), "v2026.07.25-99", 100, Some("boot-a")).unwrap();
        let content = fs::read_to_string(bin_dir.join(".dotfiles-version-cache")).unwrap();
        assert_eq!(
            content,
            format!("{}\nv2026.07.25-99\n100\nboot-a\n", super::super::REPO)
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_cache_replaces_a_symlink_without_modifying_its_target() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("bin")).unwrap();
        let target = dir.path().join("unrelated");
        fs::write(&target, "preserve this file").unwrap();
        std::os::unix::fs::symlink(&target, cache_path(dir.path())).unwrap();

        write_cache_at(dir.path(), "v1.0", 100, Some("boot-a")).unwrap();

        assert_eq!(fs::read_to_string(target).unwrap(), "preserve this file");
        assert!(
            !fs::symlink_metadata(cache_path(dir.path()))
                .unwrap()
                .is_symlink()
        );
        assert_eq!(
            read_fresh_cache_at(dir.path(), 101, Some("boot-a")),
            Some("v1.0".into())
        );
        assert_eq!(fs::read_dir(dir.path().join("bin")).unwrap().count(), 1);
    }
}
