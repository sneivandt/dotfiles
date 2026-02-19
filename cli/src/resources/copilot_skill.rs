use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};

use super::{Resource, ResourceChange, ResourceState};
use crate::exec::Executor;

/// A GitHub Copilot skill resource that can be checked and installed.
#[derive(Debug)]
pub struct CopilotSkillResource<'a> {
    /// Source URL (GitHub blob/tree URL).
    pub url: String,
    /// Destination directory under `~/.copilot/skills/`.
    pub dest: PathBuf,
    /// Executor for running git commands.
    executor: &'a dyn Executor,
}

impl<'a> CopilotSkillResource<'a> {
    /// Create a new Copilot skill resource.
    #[must_use]
    pub const fn new(url: String, dest: PathBuf, executor: &'a dyn Executor) -> Self {
        Self {
            url,
            dest,
            executor,
        }
    }

    /// Create from a config entry and skills directory.
    #[must_use]
    pub fn from_entry(
        entry: &crate::config::copilot_skills::CopilotSkill,
        skills_dir: &Path,
        executor: &'a dyn Executor,
    ) -> Self {
        let dir_name = entry
            .url
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(&entry.url);
        Self::new(entry.url.clone(), skills_dir.join(dir_name), executor)
    }
}

impl Resource for CopilotSkillResource<'_> {
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
            std::fs::create_dir_all(parent).context("creating skills parent directory")?;
        }

        download_github_folder(&self.url, &self.dest, self.executor)
            .with_context(|| format!("downloading skill from {}", self.url))?;
        Ok(ResourceChange::Applied)
    }
}

/// Download a subdirectory from a GitHub blob URL using sparse checkout.
///
/// Parses URLs like:
///   `https://github.com/{owner}/{repo}/blob/{branch}/{path}`
/// and clones only the target folder.
fn download_github_folder(url: &str, dest: &Path, executor: &dyn Executor) -> Result<()> {
    let parts: Vec<&str> = url.trim_end_matches('/').split('/').collect();
    let blob_idx = parts
        .iter()
        .position(|&p| p == "blob" || p == "tree")
        .context("URL must contain /blob/ or /tree/")?;

    // Ensure we have enough parts before blob_idx
    anyhow::ensure!(
        blob_idx >= OWNER_OFFSET,
        "invalid GitHub URL format: too few parts before blob/tree"
    );

    let owner = parts
        .get(blob_idx - OWNER_OFFSET)
        .context("missing owner in URL")?;
    let repo = parts
        .get(blob_idx - REPO_OFFSET)
        .context("missing repo in URL")?;
    let branch = parts
        .get(blob_idx + BRANCH_OFFSET)
        .context("missing branch in URL")?;

    // Use safe slicing instead of unchecked indexing
    let subpath = parts
        .get(blob_idx + PATH_OFFSET..)
        .map(|slice| slice.join("/"))
        .unwrap_or_default();

    let repo_url = format!("https://github.com/{owner}/{repo}.git");

    // Use a hash of the full URL to avoid temp directory collisions when
    // skills from different repos share the same directory name.
    let url_hash = simple_hash(url);
    let tmp = std::env::temp_dir().join(format!("dotfiles-skill-{url_hash:016x}"));

    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).context("removing previous skill temp dir")?;
    }

    // Shallow clone with no checkout
    executor.run(
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
    executor.run_in(&tmp, "git", &["sparse-checkout", "init", "--cone"])?;
    executor.run_in(&tmp, "git", &["sparse-checkout", "set", &subpath])?;
    executor.run_in(&tmp, "git", &["checkout"])?;

    // Copy result to destination
    let src = tmp.join(&subpath);
    if !src.exists() {
        // Best effort cleanup - log warning if it fails
        if let Err(e) = std::fs::remove_dir_all(&tmp) {
            eprintln!(
                "warning: failed to cleanup temp dir {}: {}",
                tmp.display(),
                e
            );
        }
        anyhow::bail!("path '{subpath}' not found in repository");
    }

    copy_dir_recursive(&src, dest)?;

    // Best effort cleanup - log warning if it fails
    if let Err(e) = std::fs::remove_dir_all(&tmp) {
        eprintln!(
            "warning: failed to cleanup temp dir {}: {}",
            tmp.display(),
            e
        );
    }
    Ok(())
}

/// FNV-1a 64-bit hash constants.
/// See: <https://en.wikipedia.org/wiki/Fowler%E2%80%93Noll%E2%80%93Vo_hash_function>
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

/// GitHub URL structure constants for parsing.
/// Expected format: `https://github.com/{owner}/{repo}/blob/{branch}/{path}`
const OWNER_OFFSET: usize = 2; // Steps back from blob/tree to owner
const REPO_OFFSET: usize = 1; // Steps back from blob/tree to repo
const BRANCH_OFFSET: usize = 1; // Steps forward from blob/tree to branch
const PATH_OFFSET: usize = 2; // Steps forward from blob/tree to subpath

/// Simple non-cryptographic hash for generating unique temp directory names.
///
/// Uses FNV-1a algorithm for fast, collision-resistant hashing of skill URLs.
fn simple_hash(s: &str) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in s.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Recursively copy a directory tree.
fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)
        .with_context(|| format!("creating directory {}", dest.display()))?;
    for entry in
        std::fs::read_dir(src).with_context(|| format!("reading directory {}", src.display()))?
    {
        let entry = entry.with_context(|| format!("reading entry in {}", src.display()))?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if src_path.is_dir() {
            // Skip .git directories
            if entry.file_name() == ".git" {
                continue;
            }
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            std::fs::copy(&src_path, &dest_path).with_context(|| {
                format!("copying {} to {}", src_path.display(), dest_path.display())
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn description_returns_url() {
        let executor = crate::exec::SystemExecutor;
        let resource = CopilotSkillResource::new(
            "https://github.com/example/skills/tree/main/my-skill".to_string(),
            PathBuf::from("/home/user/.copilot/skills/my-skill"),
            &executor,
        );
        assert_eq!(
            resource.description(),
            "https://github.com/example/skills/tree/main/my-skill"
        );
    }

    #[test]
    fn missing_when_dest_does_not_exist() {
        let executor = crate::exec::SystemExecutor;
        let resource = CopilotSkillResource::new(
            "https://github.com/example/skills/tree/main/my-skill".to_string(),
            PathBuf::from("/nonexistent/path/my-skill"),
            &executor,
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

        let executor = crate::exec::SystemExecutor;
        let resource = CopilotSkillResource::new(
            "https://github.com/example/skills/tree/main/my-skill".to_string(),
            dest,
            &executor,
        );
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Correct
        ));
    }

    #[test]
    fn from_entry_derives_dir_name() {
        let executor = crate::exec::SystemExecutor;
        let entry = crate::config::copilot_skills::CopilotSkill {
            url: "https://github.com/example/skills/tree/main/my-skill".to_string(),
        };
        let skills_dir = PathBuf::from("/home/user/.copilot/skills");
        let resource = CopilotSkillResource::from_entry(&entry, &skills_dir, &executor);
        assert_eq!(
            resource.dest,
            PathBuf::from("/home/user/.copilot/skills/my-skill")
        );
    }
}
