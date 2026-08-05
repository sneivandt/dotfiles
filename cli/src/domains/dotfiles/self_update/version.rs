//! Release-tag parsing and comparison for self-update handling.

/// Number of numeric components in a date-based release tag (`vYYYY.MM.DD-N`).
const COMPONENT_COUNT: usize = 4;

/// Return `true` if `v` is a proper release version tag (`vYYYY.MM.DD-N`).
///
/// Development builds produced by `git describe` (e.g., `v2026.07.25-1-dirty`
/// or `c6c5897-dirty`) are not release versions and must not trigger a
/// self-update. Legacy three-component `v0.1.x` tags predate date-based
/// versioning and are deliberately rejected; a binary reporting one simply
/// stops self-updating and must be replaced by re-running the wrapper.
pub(super) fn is_release_version(v: &str) -> bool {
    parse_version(v).is_some()
}

/// Parse a date-based release tag into a comparable component array.
///
/// Returns `None` for development builds, pre-release tags, tags that do not
/// have exactly [`COMPONENT_COUNT`] numeric components, or malformed input.
fn parse_version(v: &str) -> Option<[u64; COMPONENT_COUNT]> {
    let v = v.strip_prefix('v').unwrap_or(v);
    let (date, increment) = v.split_once('-')?;
    let mut components = [0_u64; COMPONENT_COUNT];
    let mut count = 0_usize;
    for part in date.split('.') {
        // `get_mut` yields `None` once the tag exceeds the supported width,
        // rejecting over-long tags without indexing.
        let slot = components.get_mut(count)?;
        *slot = part.parse::<u64>().ok()?;
        count = count.checked_add(1)?;
    }
    if count != COMPONENT_COUNT - 1 {
        return None;
    }
    *components.last_mut()? = increment.parse::<u64>().ok()?;
    Some(components)
}

/// Return `true` if `latest` is strictly newer than `current`.
///
/// Both must be valid release tags; returns `false` if either cannot be
/// parsed.
pub(super) fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
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
    fn is_release_version_accepts_date_based_tags() {
        assert!(is_release_version("v2026.07.25-1"));
        assert!(is_release_version("v2026.12.31-14"));
        assert!(is_release_version("2026.01.02-1"));
    }

    #[test]
    fn is_release_version_rejects_dev_builds() {
        assert!(!is_release_version("c6c5897-dirty"));
        assert!(!is_release_version("vc6c5897-dirty"));
        assert!(!is_release_version("v2026.07.25-1-3-gabcdef"));
        assert!(!is_release_version("v2026.07.25-1-dirty"));
        assert!(!is_release_version("dev-0.1.0"));
        assert!(!is_release_version(""));
    }

    #[test]
    fn is_release_version_rejects_wrong_component_counts() {
        assert!(!is_release_version("v2026"));
        assert!(!is_release_version("v2026.07"));
        assert!(!is_release_version("v2026.07.25"));
        assert!(!is_release_version("v2026.07.25.1"));
        assert!(!is_release_version("v2026.07.25-1.2"));
        assert!(!is_release_version("v2026.07.25.01-2"));
    }

    #[test]
    fn is_release_version_rejects_legacy_semver_tags() {
        assert!(!is_release_version("v0.1.0"));
        assert!(!is_release_version("v0.1.163"));
        assert!(!is_release_version("v1.2.3"));
    }

    #[test]
    fn parse_version_extracts_date_components() {
        assert_eq!(parse_version("v2026.07.25-1"), Some([2026, 7, 25, 1]));
        assert_eq!(parse_version("v2026.10.05-12"), Some([2026, 10, 5, 12]));
        assert_eq!(parse_version("2026.01.02-1"), Some([2026, 1, 2, 1]));
        assert_eq!(parse_version("not-a-version"), None);
        assert_eq!(parse_version("v2026.07.25-1-dirty"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn is_newer_orders_same_day_releases_by_counter() {
        assert!(is_newer("v2026.07.25-2", "v2026.07.25-1"));
        assert!(is_newer("v2026.07.25-10", "v2026.07.25-9"));
        assert!(!is_newer("v2026.07.25-1", "v2026.07.25-2"));
        assert!(!is_newer("v2026.07.25-1", "v2026.07.25-1"));
    }

    #[test]
    fn is_newer_orders_across_days_and_years() {
        assert!(is_newer("v2026.07.26-1", "v2026.07.25-9"));
        assert!(is_newer("v2026.08.01-1", "v2026.07.31-1"));
        assert!(is_newer("v2027.01.01-1", "v2026.12.31-3"));

        assert!(!is_newer("v2026.07.25-9", "v2026.07.26-1"));
        assert!(!is_newer("v2026.12.31-3", "v2027.01.01-1"));
    }

    #[test]
    fn is_newer_rejects_unparseable_tags() {
        assert!(!is_newer("garbage", "v2026.07.25-1"));
        assert!(!is_newer("v2026.07.25-1", "garbage"));
        assert!(!is_newer("v2026.07.25-1", "v0.1.163"));
    }
}
