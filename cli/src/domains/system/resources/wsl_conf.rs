//! WSL configuration resource.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};

use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::infra::exec::Executor;

const DESIRED: &str = "[boot]\nsystemd=true\n\n[interop]\nappendWindowsPath=false\n";

/// Converges `/etc/wsl.conf` while preserving unrelated sections and keys.
pub struct WslConfResource {
    target: PathBuf,
    executor: Arc<dyn Executor>,
}

impl std::fmt::Debug for WslConfResource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WslConfResource")
            .field("target", &self.target)
            .finish_non_exhaustive()
    }
}

impl WslConfResource {
    /// Create the system WSL configuration resource.
    #[must_use]
    pub fn system(executor: Arc<dyn Executor>) -> Self {
        Self::new("/etc/wsl.conf", executor)
    }

    /// Create a resource targeting a specific configuration path.
    #[must_use]
    pub fn new(target: impl Into<PathBuf>, executor: Arc<dyn Executor>) -> Self {
        Self {
            target: target.into(),
            executor,
        }
    }

    fn write(&self) -> Result<()> {
        let current = read_wsl_conf(&self.target)?.unwrap_or_default();
        let merged = merge_wsl_conf(&current);

        if let Some(parent) = self.target.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }

        match fs::write(&self.target, &merged) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::PermissionDenied => {
                let tmp = format!("/tmp/dotfiles-wsl-conf-{}", std::process::id());
                fs::write(&tmp, merged).context("writing temporary wsl.conf")?;
                let target = self.target.to_string_lossy();
                let copy_result = self.executor.run("sudo", &["cp", &tmp, &target]);
                drop(fs::remove_file(&tmp));
                copy_result.context("installing wsl.conf with sudo")?;
                Ok(())
            }
            Err(error) => Err(error).with_context(|| format!("writing {}", self.target.display())),
        }
    }
}

impl Resource for WslConfResource {
    fn description(&self) -> String {
        format!("WSL configuration {}", self.target.display())
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        self.write()?;
        Ok(ResourceChange::Applied)
    }
}

impl IntrinsicState for WslConfResource {
    fn current_state(&self) -> Result<ResourceState> {
        match read_wsl_conf(&self.target)? {
            Some(current) if has_desired_settings(&current) => Ok(ResourceState::Correct),
            Some(current) => Ok(ResourceState::Incorrect { current }),
            None => Ok(ResourceState::Missing),
        }
    }
}

fn read_wsl_conf(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

fn has_desired_settings(content: &str) -> bool {
    let mut section = "";
    let mut systemd_is_enabled = None;
    let mut windows_path_is_disabled = None;

    for raw in content.lines() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line;
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if section.eq_ignore_ascii_case("[boot]") && key.trim().eq_ignore_ascii_case("systemd") {
            systemd_is_enabled = Some(value.trim().eq_ignore_ascii_case("true"));
        } else if section.eq_ignore_ascii_case("[interop]")
            && key.trim().eq_ignore_ascii_case("appendWindowsPath")
        {
            windows_path_is_disabled = Some(value.trim().eq_ignore_ascii_case("false"));
        }
    }

    systemd_is_enabled == Some(true) && windows_path_is_disabled == Some(true)
}

fn merge_wsl_conf(current: &str) -> String {
    if current.trim().is_empty() {
        return DESIRED.to_string();
    }

    let mut lines: Vec<String> = current.lines().map(String::from).collect();
    ensure_section_key(&mut lines, "[boot]", "systemd", "true");
    ensure_section_key(&mut lines, "[interop]", "appendWindowsPath", "false");
    let mut result = lines.join("\n");
    result.push('\n');
    result
}

fn ensure_section_key(lines: &mut Vec<String>, section: &str, key: &str, value: &str) {
    let section_index = lines
        .iter()
        .position(|line| line.trim().eq_ignore_ascii_case(section));

    if let Some(start) = section_index {
        let content_start = start.saturating_add(1);
        let end = lines
            .iter()
            .enumerate()
            .skip(content_start)
            .find(|(_, line)| {
                let trimmed = line.trim();
                trimmed.starts_with('[') && trimmed.ends_with(']')
            })
            .map_or(lines.len(), |(index, _)| index);

        let key_indices: Vec<usize> = (content_start..end)
            .filter(|&index| {
                lines
                    .get(index)
                    .and_then(|line| line.split_once('='))
                    .is_some_and(|(candidate, _)| candidate.trim().eq_ignore_ascii_case(key))
            })
            .collect();

        if let Some((&first, duplicates)) = key_indices.split_first() {
            if let Some(line) = lines.get_mut(first) {
                *line = format!("{key}={value}");
            }
            for &index in duplicates.iter().rev() {
                lines.remove(index);
            }
        } else {
            let insert_at = (content_start..end)
                .rev()
                .find(|&index| lines.get(index).is_some_and(|line| !line.trim().is_empty()))
                .map_or(content_start, |index| index.saturating_add(1));
            lines.insert(insert_at, format!("{key}={value}"));
        }
    } else {
        if !lines.last().is_some_and(String::is_empty) {
            lines.push(String::new());
        }
        lines.push(section.to_string());
        lines.push(format!("{key}={value}"));
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::infra::exec::{Executor, SystemExecutor};
    use std::sync::Arc;
    use tempfile::TempDir;

    fn resource(path: &Path) -> WslConfResource {
        let executor: Arc<dyn Executor> = Arc::new(SystemExecutor);
        WslConfResource::new(path, executor)
    }

    #[test]
    fn missing_file_has_missing_state() {
        let temp = TempDir::new().unwrap();
        assert_eq!(
            resource(&temp.path().join("wsl.conf"))
                .current_state()
                .unwrap(),
            ResourceState::Missing
        );
    }

    #[test]
    fn apply_preserves_unrelated_configuration() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("wsl.conf");
        fs::write(
            &path,
            "[boot]\ncommand=service docker start\n\n[network]\ngenerateHosts=false\n",
        )
        .unwrap();

        resource(&path).apply().unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("command=service docker start"));
        assert!(content.contains("[network]\ngenerateHosts=false"));
        assert!(content.contains("[boot]\ncommand=service docker start\nsystemd=true"));
        assert!(content.contains("[interop]\nappendWindowsPath=false"));
    }

    #[test]
    fn apply_replaces_existing_keys_without_duplication() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("wsl.conf");
        fs::write(
            &path,
            "[boot]\nsystemd=true\nsystemd=false\n\n[interop]\nappendWindowsPath=false\nappendWindowsPath=true\n",
        )
        .unwrap();

        resource(&path).apply().unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content.matches("systemd=").count(), 1);
        assert_eq!(content.matches("appendWindowsPath=").count(), 1);
        assert!(content.contains("systemd=true"));
        assert!(content.contains("appendWindowsPath=false"));
    }

    #[test]
    fn invalid_utf8_is_reported_without_overwriting_the_file() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("wsl.conf");
        let original = [0xff, 0xfe, 0xfd];
        fs::write(&path, original).unwrap();

        assert!(resource(&path).current_state().is_err());
        assert!(resource(&path).apply().is_err());
        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[test]
    fn desired_configuration_has_correct_state() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("wsl.conf");
        fs::write(&path, DESIRED).unwrap();

        assert_eq!(
            resource(&path).current_state().unwrap(),
            ResourceState::Correct
        );
    }
}
