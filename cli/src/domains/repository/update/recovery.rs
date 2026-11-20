//! A merge can write tracked, untracked, or ignored files before it moves HEAD.
//! Only a verified fast-forward, not process exit or a clean status, disarms this marker.
use anyhow::{Context as _, Result};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

pub(super) struct MergeMarker(PathBuf);

impl MergeMarker {
    pub(super) fn for_checkout(root: &Path) -> Result<Self> {
        let repo = git2::Repository::open(root)
            .with_context(|| format!("locating the Git directory for {}", root.display()))?;
        // path(), not commondir(): linked worktrees have independent checkouts and indexes.
        Ok(Self(repo.path().join("dotfiles-merge-in-progress")))
    }

    pub(super) fn ensure_clear(&self, root: &Path) -> Result<()> {
        if crate::infra::fs::symlink_metadata_optional(&self.0, "checking recovery marker")?
            .is_some()
        {
            anyhow::bail!("Unverified repository merge. {}", self.recovery_steps(root));
        }
        Ok(())
    }

    pub(super) fn recovery_steps(&self, root: &Path) -> String {
        format!(
            "Further tasks stopped. In {}, inspect 'git status --short --ignored', 'git diff', and 'git diff --cached'. Preserve needed files and repair the checkout, then remove only the recovery marker {} and retry",
            root.display(),
            self.0.display()
        )
    }

    pub(super) fn begin(self, before: &str, intended: &str) -> Result<Self> {
        let mut file = fs::File::create_new(&self.0)
            .with_context(|| format!("creating the recovery marker {}", self.0.display()))?;
        writeln!(file, "before {before}\nintended {intended}")
            .and_then(|()| file.sync_all())
            .with_context(|| format!("saving the recovery marker {}", self.0.display()))?;
        Ok(self)
    }

    pub(super) fn finish(self) -> Result<()> {
        fs::remove_file(&self.0)
            .with_context(|| format!("clearing the recovery marker {}", self.0.display()))
    }
}
