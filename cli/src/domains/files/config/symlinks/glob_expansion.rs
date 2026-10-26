use anyhow::{Context as _, Result, bail};
use std::path::{Path, PathBuf};

use super::target_capture::apply_target_captures;
use super::{Symlink, path_segments, resolve_symlinks_dir, validate_paths};

#[derive(Debug)]
struct GlobMatch {
    relative_source: PathBuf,
    captures: Vec<String>,
}

pub(super) fn expand_glob_patterns(symlinks: &[Symlink], fallback: &Path) -> Result<Vec<Symlink>> {
    let mut expanded = Vec::new();
    for symlink in symlinks {
        expanded.extend(expand_one(symlink, fallback)?);
    }
    Ok(expanded)
}

fn expand_one(symlink: &Symlink, fallback: &Path) -> Result<Vec<Symlink>> {
    validate_supported_pattern("source", &symlink.source)?;
    if let Some(target) = &symlink.target {
        validate_supported_pattern("target", target)?;
    }

    let source_wildcards = wildcard_count(&symlink.source);
    let target_wildcards = symlink
        .target
        .as_ref()
        .map_or(0, |target| wildcard_count(target));
    if source_wildcards == 0 {
        if target_wildcards != 0 {
            bail!(
                "target pattern '{}' contains '*' but source '{}' is not a glob",
                symlink.target.as_deref().unwrap_or_default(),
                symlink.source
            );
        }
        return Ok(vec![symlink.clone()]);
    }
    validate_paths(symlink)?;
    if symlink.target.is_some() && source_wildcards != target_wildcards {
        bail!(
            "source pattern '{}' has {source_wildcards} wildcard(s), but target pattern '{}' has {target_wildcards}",
            symlink.source,
            symlink.target.as_deref().unwrap_or_default()
        );
    }

    let symlinks_dir = resolve_symlinks_dir(symlink, fallback);
    let matches = expand_segments(
        &symlinks_dir,
        &path_segments(&symlink.source),
        Path::new(""),
        &[],
    )
    .with_context(|| {
        format!(
            "expanding symlink glob '{}' under {}",
            symlink.source,
            symlinks_dir.display()
        )
    })?;
    if matches.is_empty() {
        bail!(
            "symlink glob '{}' matched no entries under {}",
            symlink.source,
            symlinks_dir.display()
        );
    }

    let mut expanded: Vec<Symlink> = matches
        .into_iter()
        .map(|glob_match| {
            let target = symlink
                .target
                .as_ref()
                .map(|target| apply_target_captures(target, &glob_match.captures))
                .transpose()?;
            Ok(Symlink {
                source: path_to_config_string(&glob_match.relative_source),
                target,
                origin: symlink.origin.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    expanded.sort_by(|left, right| left.source.cmp(&right.source));
    Ok(expanded)
}

fn validate_supported_pattern(kind: &str, pattern: &str) -> Result<()> {
    for segment in path_segments(pattern) {
        if segment == "**" {
            bail!("{kind} pattern '{pattern}' uses unsupported recursive wildcard '**'");
        }
        if segment.contains('*') && segment != "*" {
            bail!(
                "{kind} pattern '{pattern}' uses unsupported wildcard segment '{segment}'; only a full path segment '*' is supported"
            );
        }
    }
    Ok(())
}

fn wildcard_count(pattern: &str) -> usize {
    path_segments(pattern)
        .into_iter()
        .filter(|segment| segment == "*")
        .count()
}

fn expand_segments(
    base: &Path,
    remaining: &[String],
    relative: &Path,
    captures: &[String],
) -> Result<Vec<GlobMatch>> {
    let current = base.join(relative);
    let Some(metadata) =
        crate::infra::fs::symlink_metadata_optional(&current, "inspect glob source")?
    else {
        return Ok(Vec::new());
    };
    let Some((segment, tail)) = remaining.split_first() else {
        // Include terminal symlinks even when dangling: source validation must
        // report them rather than silently dropping a configured entry.
        return Ok(vec![GlobMatch {
            relative_source: relative.to_path_buf(),
            captures: captures.to_vec(),
        }]);
    };

    // Inspect the entry itself; never descend through directory symlinks.
    if !metadata.file_type().is_dir() {
        return Ok(Vec::new());
    }

    if segment == "*" {
        let mut entries: Vec<_> = std::fs::read_dir(&current)
            .with_context(|| format!("reading directory {}", current.display()))?
            .collect::<std::io::Result<Vec<_>>>()
            .with_context(|| format!("reading directory entry in {}", current.display()))?;
        entries.sort_by_key(std::fs::DirEntry::path);

        let mut matches = Vec::new();
        for entry in entries {
            #[allow(
                clippy::unnecessary_debug_formatting,
                reason = "escape non-UTF-8 bytes so distinct filenames remain distinguishable"
            )]
            let capture = entry.file_name().into_string().map_err(|name| {
                anyhow::anyhow!(
                    "source glob entry name is not valid UTF-8: {:?}",
                    current.join(name)
                )
            })?;
            let mut next_captures = captures.to_vec();
            next_captures.push(capture.clone());
            matches.extend(expand_segments(
                base,
                tail,
                &relative.join(capture),
                &next_captures,
            )?);
        }
        return Ok(matches);
    }

    let next_relative = relative.join(segment);
    expand_segments(base, tail, &next_relative, captures)
}

fn path_to_config_string(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::test_helpers::symlink;

    #[test]
    fn glob_does_not_descend_through_symlinks_before_literal_segments() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let apps = root.join("symlinks/apps");
        let outside = root.join("outside");
        for directory in [apps.join("managed/nested"), outside.join("nested")] {
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join("file"), "fixture").unwrap();
        }
        std::os::unix::fs::symlink(&outside, apps.join("escaped")).unwrap();
        for pattern in ["apps/*/nested/file", "apps/*/nested/*"] {
            let entry = symlink(pattern, None);
            let expanded = expand_glob_patterns(&[entry], &root).unwrap();
            assert_eq!(expanded.len(), 1, "{pattern}: {expanded:?}");
            assert_eq!(expanded[0].source, "apps/managed/nested/file");
        }

        let entry = symlink("apps/escaped/nested/*", None);
        let error = expand_glob_patterns(&[entry], &root).unwrap_err();
        assert!(error.to_string().contains("matched no entries"), "{error}");
    }

    #[test]
    fn glob_still_includes_terminal_symlinks() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let apps = root.join("symlinks/apps");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(root.join("symlinks/file"), "fixture").unwrap();
        std::os::unix::fs::symlink("../file", apps.join("alias")).unwrap();
        let entry = symlink("apps/*", None);

        let expanded = expand_glob_patterns(&[entry], &root).unwrap();
        assert_eq!(expanded.len(), 1);
        assert_eq!(expanded[0].source, "apps/alias");
    }

    #[test]
    fn glob_retains_dangling_terminal_symlinks_alongside_valid_matches() {
        let fixture = tempfile::tempdir().unwrap();
        let apps = fixture.path().join("symlinks/apps");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(apps.join("valid"), "fixture").unwrap();
        std::os::unix::fs::symlink("missing", apps.join("dangling")).unwrap();
        for (source, expected) in [
            ("apps/*", vec!["apps/dangling", "apps/valid"]),
            ("*/dangling", vec!["apps/dangling"]),
        ] {
            let entries = expand_glob_patterns(&[symlink(source, None)], fixture.path()).unwrap();
            assert_eq!(
                entries
                    .iter()
                    .map(|entry| entry.source.as_str())
                    .collect::<Vec<_>>(),
                expected
            );
            let diagnostics = super::super::validate(&entries, fixture.path());
            assert!(
                diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message.contains("dangling"))
            );
        }
    }

    #[test]
    fn glob_reports_metadata_errors_even_when_another_branch_matches() {
        use std::os::unix::fs::PermissionsExt as _;

        let fixture = tempfile::tempdir().unwrap();
        let apps = fixture.path().join("symlinks/apps");
        let denied = apps.join("denied");
        for directory in [&denied, &apps.join("valid")] {
            std::fs::create_dir_all(directory.join("nested")).unwrap();
            std::fs::write(directory.join("nested/file"), "fixture").unwrap();
        }
        std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o000)).unwrap();
        let probe = std::fs::symlink_metadata(denied.join("nested"));
        let result = expand_glob_patterns(&[symlink("apps/*/nested/*", None)], fixture.path());
        std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o700)).unwrap();
        // Root can bypass permission bits; the permission-error case requires
        // an unprivileged runner (as used by normal local and CI tests).
        if probe.is_ok() {
            return;
        }
        assert_eq!(
            probe.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        let error = format!("{:#}", result.unwrap_err());
        assert!(
            error.contains("inspect glob source") && error.contains("denied/nested"),
            "{error}"
        );
    }

    #[test]
    fn glob_rejects_non_utf8_captures_instead_of_aliasing_another_entry() {
        use std::os::unix::ffi::OsStrExt as _;

        let fixture = tempfile::tempdir_in(".").unwrap();
        let apps = fixture.path().join("symlinks/apps");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(apps.join(std::ffi::OsStr::from_bytes(b"\xff")), "invalid").unwrap();
        std::fs::write(apps.join("\u{fffd}"), "valid Unicode").unwrap();
        let entry = symlink("apps/*", Some(".apps/*"));

        let error = expand_glob_patterns(&[entry], fixture.path()).unwrap_err();
        assert!(format!("{error:#}").contains("UTF-8"), "{error:#}");
    }
}
