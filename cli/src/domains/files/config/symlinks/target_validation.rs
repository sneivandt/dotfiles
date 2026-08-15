use super::Symlink;
use anyhow::{Result, bail};

pub(super) fn validate_unique_targets(symlinks: &[Symlink]) -> Result<()> {
    let mut targets: Vec<(String, Vec<String>, String)> = Vec::new();
    for symlink in symlinks {
        let target = target_key(symlink);
        let segments = super::path_segments(&target);
        for (existing_target, existing_segments, existing_source) in &targets {
            if existing_segments == &segments {
                bail!(
                    "symlink target collision for '{target}': '{existing_source}' and '{}' both map to the same target",
                    symlink.source
                );
            }

            if is_ancestor(existing_segments, &segments) {
                bail!(
                    "symlink target overlap: '{existing_source}' maps to parent target '{existing_target}', which contains target '{target}' for '{}'",
                    symlink.source
                );
            }
            if is_ancestor(&segments, existing_segments) {
                bail!(
                    "symlink target overlap: '{}' maps to parent target '{target}', which contains target '{existing_target}' for '{existing_source}'",
                    symlink.source
                );
            }
        }
        targets.push((target, segments, symlink.source.clone()));
    }
    Ok(())
}

fn is_ancestor(parent: &[String], child: &[String]) -> bool {
    parent.len() < child.len() && child.starts_with(parent)
}

fn target_key(symlink: &Symlink) -> String {
    symlink
        .target
        .clone()
        .unwrap_or_else(|| format!(".{}", symlink.source))
        .replace('\\', "/")
}
