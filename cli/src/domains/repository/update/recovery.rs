//! Detect changed configuration inputs and record unverified worktree writes.
use anyhow::{Context as _, Result};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct CheckoutRevision {
    head: Vec<u8>,
    commit: Option<git2::Oid>,
}

pub(super) fn checkout_revision(root: &Path) -> Result<Option<CheckoutRevision>> {
    if crate::infra::fs::symlink_metadata_optional(&root.join(".git"), "checking checkout .git")?
        .is_none()
    {
        return Ok(None);
    }
    let repo = git2::Repository::open(root)?;
    let revision = match repo.head() {
        Ok(head) => CheckoutRevision {
            head: head.name_bytes().to_vec(),
            commit: Some(
                head.target()
                    .context("resolved Git HEAD has no object ID")?,
            ),
        },
        Err(error) if error.code() == git2::ErrorCode::UnbornBranch => {
            let head = repo.find_reference("HEAD")?;
            CheckoutRevision {
                head: head
                    .symbolic_target_bytes()
                    .context("unborn Git HEAD is not on a branch")?
                    .to_vec(),
                commit: None,
            }
        }
        Err(error) => return Err(error.into()),
    };
    Ok(Some(revision))
}

/// The committed checkout inputs seen before loading the configuration.
pub(crate) struct CheckoutSnapshot(Vec<(PathBuf, Option<CheckoutRevision>)>);

impl CheckoutSnapshot {
    pub(crate) fn capture(root: &Path, overlay: Option<&Path>) -> Result<Self> {
        let revisions = std::iter::once(root)
            .chain(overlay)
            .map(|path| {
                let revision = checkout_revision(path).with_context(|| {
                    format!("reading Git checkout revision in {}", path.display())
                })?;
                Ok((path.to_path_buf(), revision))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self(revisions))
    }

    pub(crate) fn ensure_current(&self) -> Result<()> {
        for (path, previous) in &self.0 {
            let current = checkout_revision(path).with_context(|| {
                format!("rechecking Git checkout revision in {}", path.display())
            })?;
            anyhow::ensure!(
                &current == previous,
                "Git checkout changed while preparing tasks in {}. Further tasks stopped. Review the checkout and retry",
                path.display()
            );
        }
        Ok(())
    }
}

// A merge can write files before it moves HEAD. Clear this marker only after a
// verified fast-forward.
pub(super) struct MergeMarker(PathBuf);

impl MergeMarker {
    pub(super) fn for_checkout(root: &Path) -> Result<Self> {
        let repo = git2::Repository::open(root)
            .with_context(|| format!("locating the Git directory for {}", root.display()))?;
        // path(), not commondir(): linked worktrees have independent checkouts and indexes.
        Ok(Self(repo.path().join("dotfiles-merge-in-progress")))
    }

    pub(super) fn pending_path(&self) -> Result<Option<&Path>> {
        Ok(
            crate::infra::fs::symlink_metadata_optional(&self.0, "checking recovery marker")?
                .map(|_| self.0.as_path()),
        )
    }

    pub(super) fn ensure_clear(&self, root: &Path) -> Result<()> {
        if self.pending_path()?.is_some() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkout_snapshot_distinguishes_exports_unborn_heads_and_branch_changes() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("root");
        let overlay = fixture.path().join("overlay");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&overlay).unwrap();
        let exported = CheckoutSnapshot::capture(&root, Some(&overlay)).unwrap();
        exported.ensure_current().unwrap();

        let repo = git2::Repository::init(&overlay).unwrap();
        assert!(exported.ensure_current().is_err());
        let unborn = CheckoutSnapshot::capture(&root, Some(&overlay)).unwrap();
        unborn.ensure_current().unwrap();
        let tree = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree).unwrap();
        let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
        let commit = repo
            .commit(Some("HEAD"), &signature, &signature, "initial", &tree, &[])
            .unwrap();
        assert!(unborn.ensure_current().is_err());

        let original = CheckoutSnapshot::capture(&root, Some(&overlay)).unwrap();
        repo.branch("other", &repo.find_commit(commit).unwrap(), false)
            .unwrap();
        repo.set_head("refs/heads/other").unwrap();
        assert!(original.ensure_current().is_err());

        let switched = CheckoutSnapshot::capture(&root, Some(&overlay)).unwrap();
        fs::write(repo.path().join("HEAD"), b"invalid head\n").unwrap();
        let error = switched.ensure_current().unwrap_err();
        assert!(format!("{error:#}").contains("rechecking Git checkout revision"));
    }

    #[test]
    fn merge_marker_and_checkout_recovery_checks_detect_and_describe_pending_merges() {
        let fixture = tempfile::tempdir().unwrap();
        let non_git = fixture.path().join("plain");
        let repo_dir = fixture.path().join("repo");
        fs::create_dir(&non_git).unwrap();
        fs::create_dir(&repo_dir).unwrap();
        git2::Repository::init(&repo_dir).unwrap();

        super::super::ensure_recovery_clear(&non_git).unwrap();
        assert!(
            super::super::pending_recovery_marker(&non_git)
                .unwrap()
                .is_none()
        );
        super::super::ensure_recovery_clear(&repo_dir).unwrap();
        assert!(
            super::super::pending_recovery_marker(&repo_dir)
                .unwrap()
                .is_none()
        );

        let marker = MergeMarker::for_checkout(&repo_dir)
            .unwrap()
            .begin("aaa111", "bbb222")
            .unwrap();
        let pending = marker
            .pending_path()
            .unwrap()
            .expect("marker should be pending")
            .to_path_buf();
        assert_eq!(
            super::super::pending_recovery_marker(&repo_dir).unwrap(),
            Some(pending.clone())
        );

        let error = super::super::ensure_recovery_clear(&repo_dir).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("Unverified repository merge"), "{message}");
        assert!(
            message.contains(&repo_dir.display().to_string()),
            "{message}"
        );
        assert!(
            message.contains(&pending.display().to_string()),
            "{message}"
        );
        assert!(
            message.contains("git status --short --ignored"),
            "{message}"
        );

        marker.finish().unwrap();
        super::super::ensure_recovery_clear(&repo_dir).unwrap();
        assert!(
            super::super::pending_recovery_marker(&repo_dir)
                .unwrap()
                .is_none()
        );
    }
}
