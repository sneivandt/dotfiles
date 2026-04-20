//! Semver parsing and comparison for release-tag handling.

/// Return `true` if `v` is a proper release version tag (`vMAJOR.MINOR.PATCH`).
///
/// Development builds produced by `git describe` (e.g., `v0.1.2-3-gabcdef` or
/// `c6c5897-dirty`) are not release versions and must not trigger a self-update.
pub(super) fn is_release_version(v: &str) -> bool {
    parse_semver(v).is_some()
}

/// Parse a version string into `(major, minor, patch)`.
///
/// Accepts both `vMAJOR.MINOR.PATCH` and `MAJOR.MINOR.PATCH` formats.
/// Returns `None` for development builds, pre-release tags, or malformed input.
fn parse_semver(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.strip_prefix('v').unwrap_or(v);
    let mut parts = v.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// Return `true` if `latest` is strictly newer than `current`.
///
/// Both must be valid semver tags; returns `false` if either cannot be parsed.
pub(super) fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_semver(latest), parse_semver(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn is_release_version_accepts_semver_tags() {
        assert!(is_release_version("v0.1.0"));
        assert!(is_release_version("v1.2.3"));
        assert!(is_release_version("v0.1.163"));
    }

    #[test]
    fn is_release_version_rejects_dev_builds() {
        assert!(!is_release_version("c6c5897-dirty"));
        assert!(!is_release_version("vc6c5897-dirty"));
        assert!(!is_release_version("v0.1.2-3-gabcdef"));
        assert!(!is_release_version("v0.1.2-dirty"));
        assert!(!is_release_version("0.1.2-dirty"));
        assert!(!is_release_version(""));
    }

    #[test]
    fn parse_semver_extracts_version_tuple() {
        assert_eq!(parse_semver("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_semver("v0.1.163"), Some((0, 1, 163)));
        assert_eq!(parse_semver("not-a-version"), None);
        assert_eq!(parse_semver("v0.1.2-dirty"), None);
        assert_eq!(parse_semver(""), None);
    }

    #[test]
    fn is_newer_compares_semantically() {
        assert!(is_newer("v0.2.0", "v0.1.0"));
        assert!(is_newer("v1.0.0", "v0.9.9"));
        assert!(is_newer("v0.1.1", "v0.1.0"));

        assert!(!is_newer("v0.1.0", "v0.1.0"));

        assert!(!is_newer("v0.1.0", "v0.2.0"));
        assert!(!is_newer("v0.9.9", "v1.0.0"));

        assert!(!is_newer("garbage", "v0.1.0"));
        assert!(!is_newer("v0.2.0", "garbage"));
    }
}
