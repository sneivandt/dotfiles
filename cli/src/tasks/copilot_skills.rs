use anyhow::{Context as _, Result};
use std::path::Path;

use super::{Context, Task, TaskResult};
use crate::exec;

/// Install GitHub Copilot skills.
pub struct InstallCopilotSkills;

impl Task for InstallCopilotSkills {
    fn name(&self) -> &'static str {
        "Install Copilot skills"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        !ctx.config.copilot_skills.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let skills_dir = ctx.home.join(".copilot/skills");

        let mut changed = 0u32;
        let mut already_ok = 0u32;

        for skill in &ctx.config.copilot_skills {
            // Derive a directory name from the URL (last path segment)
            let dir_name = skill
                .url
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or(&skill.url);
            let dest = skills_dir.join(dir_name);

            if dest.exists() {
                ctx.log
                    .debug(&format!("ok: {} (already installed)", skill.url));
                already_ok += 1;
                continue;
            }

            if ctx.dry_run {
                ctx.log
                    .dry_run(&format!("would install skill: {}", skill.url));
                changed += 1;
                continue;
            }

            if !skills_dir.exists() {
                std::fs::create_dir_all(&skills_dir)?;
            }

            match download_github_folder(&skill.url, &dest) {
                Ok(()) => {
                    ctx.log.debug(&format!("installed skill: {}", skill.url));
                    changed += 1;
                }
                Err(e) => {
                    ctx.log
                        .warn(&format!("failed to install skill: {}: {e}", skill.url));
                }
            }
        }

        if ctx.dry_run {
            ctx.log
                .info(&format!("{changed} would change, {already_ok} already ok"));
            return Ok(TaskResult::DryRun);
        }

        ctx.log
            .info(&format!("{changed} changed, {already_ok} already ok"));
        Ok(TaskResult::Ok)
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
