//! Repository file discovery for validation tasks.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

/// Discover local APM plugin directories.
pub(crate) fn discover_apm_plugin_dirs(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("reading APM plugins directory {}", dir.display()));
        }
    };

    let mut plugins = Vec::new();
    for entry in entries {
        let entry =
            entry.with_context(|| format!("reading directory entry in {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() && path.join("apm.yml").is_file() {
            plugins.push(path);
        }
    }
    plugins.sort();
    Ok(plugins)
}
