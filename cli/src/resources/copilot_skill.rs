//! GitHub Copilot skill resource.
use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{Applicable, Resource, ResourceChange, ResourceState};
use crate::error::ResourceError;
use crate::exec::Executor;

/// A GitHub Copilot skill resource that can be checked and installed.
#[derive(Debug)]
pub struct CopilotSkillResource {
    /// Source URL (GitHub blob/tree URL).
    pub url: String,
    /// Destination directory under `~/.copilot/skills/`.
    pub dest: PathBuf,
    /// Executor for running git commands.
    executor: Arc<dyn Executor>,
}

impl CopilotSkillResource {
    /// Create a new Copilot skill resource.
    #[must_use]
    pub fn new(url: String, dest: PathBuf, executor: Arc<dyn Executor>) -> Self {
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
        executor: Arc<dyn Executor>,
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

impl Applicable for CopilotSkillResource {
    fn description(&self) -> String {
        self.url.clone()
    }

    fn apply(&self) -> Result<ResourceChange> {
        crate::fs::ensure_parent_dir(&self.dest)?;

        download_github_folder(&self.url, &self.dest, &*self.executor)
            .with_context(|| format!("downloading skill from {}", self.url))?;
        Ok(ResourceChange::Applied)
    }
}

impl Resource for CopilotSkillResource {
    fn current_state(&self) -> Result<ResourceState> {
        if !self.dest.exists() {
            return Ok(ResourceState::Missing);
        }

        if !self.dest.is_dir() {
            return Ok(ResourceState::Invalid {
                reason: format!(
                    "skill destination is not a directory: {}",
                    self.dest.display()
                ),
            });
        }

        if skill_dir_has_entries(&self.dest)? {
            Ok(ResourceState::Correct)
        } else {
            Ok(ResourceState::Missing)
        }
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

    let result = (|| -> Result<()> {
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
            return Err(ResourceError::ConflictingState {
                resource: format!("skill path {subpath}"),
                expected: "path exists in repository".to_string(),
                actual: "path not found after checkout".to_string(),
            }
            .into());
        }

        crate::fs::copy_dir_recursive(&src, dest, true)?;
        Ok(())
    })();

    cleanup_temp_dir(&tmp);
    result
}

fn skill_dir_has_entries(path: &Path) -> Result<bool> {
    Ok(std::fs::read_dir(path)
        .with_context(|| format!("reading skill directory {}", path.display()))?
        .next()
        .transpose()
        .with_context(|| format!("reading entry in skill directory {}", path.display()))?
        .is_some())
}

fn cleanup_temp_dir(path: &Path) {
    if path.exists()
        && let Err(e) = std::fs::remove_dir_all(path)
    {
        tracing::warn!("failed to cleanup temp dir {}: {e}", path.display());
    }
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

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::exec::ExecResult;
    use anyhow::bail;

    #[derive(Debug)]
    struct CloneFixtureExecutor {
        skill_subpath: String,
    }

    impl CloneFixtureExecutor {
        fn new(skill_subpath: &str) -> Self {
            Self {
                skill_subpath: skill_subpath.to_string(),
            }
        }

        fn ok_result() -> ExecResult {
            ExecResult {
                stdout: String::new(),
                stderr: String::new(),
                success: true,
                code: Some(0),
            }
        }
    }

    impl Executor for CloneFixtureExecutor {
        fn run(&self, program: &str, args: &[&str]) -> Result<ExecResult> {
            if program == "git" && args.first() == Some(&"clone") {
                let dest = PathBuf::from(
                    args.last()
                        .copied()
                        .ok_or_else(|| anyhow::anyhow!("missing clone destination"))?,
                );
                let src = dest.join(&self.skill_subpath);
                std::fs::create_dir_all(&src)?;
                std::fs::write(src.join("SKILL.md"), "# skill\n")?;
                return Ok(Self::ok_result());
            }

            bail!("unexpected run call: {program} {args:?}");
        }

        fn run_in_with_env(
            &self,
            _: &Path,
            program: &str,
            _: &[&str],
            _: &[(&str, &str)],
        ) -> Result<ExecResult> {
            if program == "git" {
                Ok(Self::ok_result())
            } else {
                bail!("unexpected run_in_with_env call: {program}");
            }
        }

        fn run_unchecked(&self, _: &str, _: &[&str]) -> Result<ExecResult> {
            Ok(Self::ok_result())
        }

        fn which(&self, _: &str) -> bool {
            true
        }

        fn which_path(&self, program: &str) -> Result<PathBuf> {
            Ok(PathBuf::from(format!("/usr/bin/{program}")))
        }
    }

    #[test]
    fn description_returns_url() {
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let resource = CopilotSkillResource::new(
            "https://github.com/example/skills/tree/main/my-skill".to_string(),
            PathBuf::from("/home/user/.copilot/skills/my-skill"),
            Arc::clone(&executor),
        );
        assert_eq!(
            resource.description(),
            "https://github.com/example/skills/tree/main/my-skill"
        );
    }

    #[test]
    fn missing_when_dest_does_not_exist() {
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let resource = CopilotSkillResource::new(
            "https://github.com/example/skills/tree/main/my-skill".to_string(),
            PathBuf::from("/nonexistent/path/my-skill"),
            Arc::clone(&executor),
        );
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Missing
        ));
    }

    #[test]
    fn missing_when_dest_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("my-skill");
        std::fs::create_dir(&dest).unwrap();

        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let resource = CopilotSkillResource::new(
            "https://github.com/example/skills/tree/main/my-skill".to_string(),
            dest,
            Arc::clone(&executor),
        );
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Missing
        ));
    }

    #[test]
    fn correct_when_dest_has_contents() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("my-skill");
        std::fs::create_dir(&dest).unwrap();
        std::fs::write(dest.join("SKILL.md"), "# skill\n").unwrap();

        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let resource = CopilotSkillResource::new(
            "https://github.com/example/skills/tree/main/my-skill".to_string(),
            dest,
            Arc::clone(&executor),
        );
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Correct
        ));
    }

    #[test]
    fn from_entry_derives_dir_name() {
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let entry = crate::config::copilot_skills::CopilotSkill {
            url: "https://github.com/example/skills/tree/main/my-skill".to_string(),
        };
        let skills_dir = PathBuf::from("/home/user/.copilot/skills");
        let resource = CopilotSkillResource::from_entry(&entry, &skills_dir, Arc::clone(&executor));
        assert_eq!(
            resource.dest,
            PathBuf::from("/home/user/.copilot/skills/my-skill")
        );
    }

    #[test]
    fn simple_hash_is_deterministic() {
        let url = "https://github.com/example/skills/tree/main/my-skill";
        assert_eq!(simple_hash(url), simple_hash(url));
    }

    #[test]
    fn simple_hash_differs_for_different_inputs() {
        let url1 = "https://github.com/owner1/repo/tree/main/skill-a";
        let url2 = "https://github.com/owner2/repo/tree/main/skill-b";
        assert_ne!(simple_hash(url1), simple_hash(url2));
    }

    #[test]
    fn simple_hash_empty_string() {
        // Should not panic; returns the FNV offset basis for empty input
        let h = simple_hash("");
        assert_eq!(h, FNV_OFFSET_BASIS);
    }

    #[test]
    fn from_entry_trims_trailing_slash_in_url() {
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let entry = crate::config::copilot_skills::CopilotSkill {
            url: "https://github.com/example/skills/tree/main/my-skill/".to_string(),
        };
        let skills_dir = PathBuf::from("/home/user/.copilot/skills");
        let resource = CopilotSkillResource::from_entry(&entry, &skills_dir, Arc::clone(&executor));
        // The trailing slash should be stripped so the dir name is "my-skill", not "".
        assert_eq!(
            resource.dest,
            PathBuf::from("/home/user/.copilot/skills/my-skill")
        );
    }

    #[test]
    fn from_entry_uses_full_string_when_no_slash_in_url() {
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
        let entry = crate::config::copilot_skills::CopilotSkill {
            url: "simple-name".to_string(),
        };
        let skills_dir = PathBuf::from("/home/user/.copilot/skills");
        let resource = CopilotSkillResource::from_entry(&entry, &skills_dir, Arc::clone(&executor));
        assert_eq!(
            resource.dest,
            PathBuf::from("/home/user/.copilot/skills/simple-name")
        );
    }

    #[test]
    fn apply_returns_error_for_url_without_blob_or_tree() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("skill");
        // TestExecutor would panic if called; but URL parsing fails before any
        // executor call so this tests the validation path only.
        let executor: Arc<dyn Executor> = Arc::new(crate::exec::test_helpers::TestExecutor::ok(""));
        let resource = CopilotSkillResource::new(
            "https://github.com/owner/repo/main/path".to_string(),
            dest,
            Arc::clone(&executor),
        );
        let err = resource.apply().unwrap_err();
        // The error is wrapped by `with_context`, so check the full chain.
        let chain = format!("{err:#}");
        assert!(
            chain.contains("/blob/ or /tree/"),
            "expected URL format error, got: {chain}"
        );
    }

    #[test]
    fn download_github_folder_cleans_up_temp_dir_on_copy_failure() {
        let temp_dir = tempfile::tempdir().unwrap();
        let dest = temp_dir.path().join("skill");
        std::fs::write(&dest, "not a directory").unwrap();

        let url = "https://github.com/example/skills/tree/main/my-skill";
        let tmp = std::env::temp_dir().join(format!("dotfiles-skill-{:016x}", simple_hash(url)));
        if tmp.exists() {
            std::fs::remove_dir_all(&tmp).unwrap();
        }

        let executor = CloneFixtureExecutor::new("my-skill");
        let err = download_github_folder(url, &dest, &executor).unwrap_err();

        let chain = format!("{err:#}");
        assert!(
            chain.contains("creating directory"),
            "expected copy failure, got: {chain}"
        );
        assert!(
            !tmp.exists(),
            "temporary clone directory should be removed on copy failure"
        );
    }
}
