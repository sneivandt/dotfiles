use anyhow::{Context as _, Result, bail};
use std::path::{Path, PathBuf};

use super::target_capture::apply_target_captures;
use super::target_validation::validate_unique_targets;
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
    validate_unique_targets(&expanded)?;
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

/// Reports whether `path` is a real directory, without following symlinks.
///
/// Glob expansion recurses into directories via [`std::fs::read_dir`]. Using
/// [`Path::is_dir`] would follow a symlink-to-directory and recurse outside the
/// managed `symlinks/` tree. `symlink_metadata` inspects the entry itself, so a
/// symlink is never treated as a directory to descend into.
fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_dir())
}

fn expand_segments(
    base: &Path,
    remaining: &[String],
    relative: &Path,
    captures: &[String],
) -> Result<Vec<GlobMatch>> {
    let Some((segment, tail)) = remaining.split_first() else {
        return Ok(base
            .join(relative)
            .exists()
            .then(|| GlobMatch {
                relative_source: relative.to_path_buf(),
                captures: captures.to_vec(),
            })
            .into_iter()
            .collect());
    };

    if segment == "*" {
        let current = base.join(relative);
        if !is_real_dir(&current) {
            return Ok(Vec::new());
        }
        let mut entries: Vec<_> = std::fs::read_dir(&current)
            .with_context(|| format!("reading directory {}", current.display()))?
            .collect::<std::io::Result<Vec<_>>>()
            .with_context(|| format!("reading directory entry in {}", current.display()))?;
        entries.sort_by_key(std::fs::DirEntry::path);

        let mut matches = Vec::new();
        for entry in entries {
            let capture = entry.file_name().to_string_lossy().into_owned();
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
    if !base.join(&next_relative).exists() {
        return Ok(Vec::new());
    }
    expand_segments(base, tail, &next_relative, captures)
}

fn path_to_config_string(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
