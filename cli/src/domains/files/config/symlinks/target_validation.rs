use anyhow::{Result, bail};
use std::collections::HashMap;
use std::collections::hash_map::Entry;

use super::Symlink;

pub(super) fn validate_unique_targets(symlinks: &[Symlink]) -> Result<()> {
    let mut targets = HashMap::new();
    for symlink in symlinks {
        let target = target_key(symlink);
        match targets.entry(target) {
            Entry::Vacant(entry) => {
                entry.insert(symlink.source.clone());
            }
            Entry::Occupied(entry) => {
                bail!(
                    "symlink target collision for '{}': '{}' and '{}' both map to the same target",
                    entry.key(),
                    entry.get(),
                    symlink.source
                );
            }
        }
    }
    Ok(())
}

fn target_key(symlink: &Symlink) -> String {
    symlink
        .target
        .clone()
        .unwrap_or_else(|| format!(".{}", symlink.source))
        .replace('\\', "/")
}
