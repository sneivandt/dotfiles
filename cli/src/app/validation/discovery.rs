//! Repository file discovery for validation tasks.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

/// Discover local APM plugin directories.
pub(crate) fn discover_apm_plugin_dirs(dir: &Path) -> Result<Vec<PathBuf>> {
    if crate::infra::fs::symlink_metadata_optional(dir, "inspect APM plugins directory")?.is_none()
    {
        return Ok(Vec::new());
    }
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("reading APM plugins directory {}", dir.display()))?;

    let mut plugins = Vec::new();
    for entry in entries {
        let entry =
            entry.with_context(|| format!("reading directory entry in {}", dir.display()))?;
        let path = entry.path();
        if !path
            .metadata()
            .with_context(|| format!("inspecting {}", path.display()))?
            .is_dir()
        {
            continue;
        }
        let manifest = path.join("apm.yml");
        if crate::infra::fs::symlink_metadata_optional(&manifest, "inspect APM manifest")?.is_none()
        {
            continue;
        }
        anyhow::ensure!(
            manifest
                .metadata()
                .with_context(|| format!("inspecting {}", manifest.display()))?
                .is_file(),
            "expected a regular APM manifest at {}",
            manifest.display()
        );
        plugins.push(path);
    }
    plugins.sort();
    Ok(plugins)
}
