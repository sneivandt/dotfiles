use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};

use super::{Resource, ResourceChange, ResourceState};
use crate::exec;

/// A GitHub Copilot skill resource that can be checked and installed.
#[derive(Debug, Clone)]
pub struct CopilotSkillResource {
    /// Source URL (GitHub blob/tree URL).
    pub url: String,
    /// Destination directory under `~/.copilot/skills/`.
    pub dest: PathBuf,
}

impl CopilotSkillResource {
    /// Create a new Copilot skill resource.
    #[must_use]
    pub const fn new(url: String, dest: PathBuf) -> Self {
        Self { url, dest }
    }

    /// Create from a config entry and skills directory.
    #[must_use]
    pub fn from_entry(
        entry: &crate::config::copilot_skills::CopilotSkill,
        skills_dir: &Path,
    ) -> Self {
        let dir_name = entry
            .url
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(&entry.url);
        Self::new(entry.url.clone(), skills_dir.join(dir_name))
    }
}

impl Resource for CopilotSkillResource {
    fn description(&self) -> String {
        self.url.clone()
    }

    fn current_state(&self) -> Result<ResourceState> {
        if self.dest.exists() {
            Ok(ResourceState::Correct)
        } else {
            Ok(ResourceState::Missing)
        }
    }

    fn apply(&self) -> Result<ResourceChange> {
        // Ensure parent directory exists
        if let Some(parent) = self.dest.parent() {
            std::fs::create_dir_all(parent)?;
        }

        download_github_folder(&self.url, &self.dest)?;
        Ok(ResourceChange::Applied)
    }
}

/// Download a subdirectory from a GitHub blob URL using sparse checkout.
///
/// Parses URLs like:
///   `https://github.com/{owner}/{repo}/blob/{branch}/{path}`
/// and clones only the target folder.
fn download_github_folder(url: &str, dest: &Path) -> Result<()> {
    let parts: Vec<&str> = url.trim_end_matches('/').split('/').collect();
    let blob_idx = parts
        .iter()
        .position(|&p| p == "blob" || p == "tree")
        .context("URL must contain /blob/ or /tree/")?;

    let owner = parts.get(blob_idx - 2).context("missing owner in URL")?;
    let repo = parts.get(blob_idx - 1).context("missing repo in URL")?;
    let branch = parts.get(blob_idx + 1).context("missing branch in URL")?;
    let subpath = parts[blob_idx + 2..].join("/");

    let repo_url = format!("https://github.com/{owner}/{repo}.git");

    let dir_name = dest
        .file_name()
        .map_or_else(|| "skill".to_string(), |n| n.to_string_lossy().to_string());
    let tmp = std::env::temp_dir().join(format!("dotfiles-skill-{dir_name}"));

    if tmp.exists() {
        std::fs::remove_dir_all(&tmp)?;
    }

    // Shallow clone with no checkout
    exec::run(
        "git",
        &[
            "clone",
            "--filter=blob:none",
            "--no-checkout",
            "--depth",
            "1",
            "--branch",
            branch,
            &repo_url,
            &tmp.to_string_lossy(),
        ],
    )?;

    // Sparse checkout just the target path
    exec::run_in(&tmp, "git", &["sparse-checkout", "init", "--cone"])?;
    exec::run_in(&tmp, "git", &["sparse-checkout", "set", &subpath])?;
    exec::run_in(&tmp, "git", &["checkout"])?;

    // Copy result to destination
    let src = tmp.join(&subpath);
    if !src.exists() {
        std::fs::remove_dir_all(&tmp).ok(); // Cleanup on error (best effort)
        anyhow::bail!("path '{subpath}' not found in repository");
    }

    copy_dir_recursive(&src, dest)?;

    std::fs::remove_dir_all(&tmp).ok(); // Cleanup (best effort)
    Ok(())
}

/// Recursively copy a directory tree.
fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if src_path.is_dir() {
            // Skip .git directories
            if entry.file_name() == ".git" {
                continue;
            }
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            std::fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn description_returns_url() {
        let resource = CopilotSkillResource::new(
            "https://github.com/example/skills/tree/main/my-skill".to_string(),
            PathBuf::from("/home/user/.copilot/skills/my-skill"),
        );
        assert_eq!(
            resource.description(),
            "https://github.com/example/skills/tree/main/my-skill"
        );
    }

    #[test]
    fn missing_when_dest_does_not_exist() {
        let resource = CopilotSkillResource::new(
            "https://github.com/example/skills/tree/main/my-skill".to_string(),
            PathBuf::from("/nonexistent/path/my-skill"),
        );
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Missing
        ));
    }

    #[test]
    fn correct_when_dest_exists() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("my-skill");
        std::fs::create_dir(&dest).unwrap();

        let resource = CopilotSkillResource::new(
            "https://github.com/example/skills/tree/main/my-skill".to_string(),
            dest,
        );
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Correct
        ));
    }

    #[test]
    fn from_entry_derives_dir_name() {
        let entry = crate::config::copilot_skills::CopilotSkill {
            url: "https://github.com/example/skills/tree/main/my-skill".to_string(),
        };
        let skills_dir = PathBuf::from("/home/user/.copilot/skills");
        let resource = CopilotSkillResource::from_entry(&entry, &skills_dir);
        assert_eq!(
            resource.dest,
            PathBuf::from("/home/user/.copilot/skills/my-skill")
        );
    }
}
