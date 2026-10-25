//! Disposable, committed build inputs that Paru may safely reset and clean.

use super::{ReleaseSource, read_optional, safe_component};
use crate::engine::Context;
use crate::infra::{exec::CommandSpec, fs::TempGuard};
use anyhow::{Context as _, Result, ensure};
use std::path::{Path, PathBuf};

const MARKER: &str = "dotfiles local PKGBUILD snapshot v1\n";

pub(super) struct Snapshot {
    pub path: PathBuf,
    files: Vec<(&'static str, String)>,
    pending_pins: bool,
}

impl Snapshot {
    // PKGBUILDs are executable code. Evaluate only during apply, from copies of
    // the exact planned inputs, before changing the published build repository.
    pub fn validate_metadata(&self, ctx: &Context, source: &ReleaseSource) -> Result<()> {
        let staging =
            TempGuard::create_unique_dir(&std::env::temp_dir(), "dotfiles-pkgbuild-", "")?;
        for (name, content) in &self.files {
            std::fs::write(staging.path().join(name), content)?;
        }
        let output = ctx.executor().execute(
            CommandSpec::new("makepkg")
                .args(&["--printsrcinfo"])
                .current_dir(staging.path()),
        )?;
        source.validate_srcinfo(&output.stdout)
    }

    pub fn plan(
        build_root: &Path,
        package_base: &str,
        source: &Path,
        preview: bool,
    ) -> Result<Self> {
        ensure!(safe_component(package_base), "invalid local package base");
        let path = build_root.join(package_base);
        let recipe =
            std::fs::read_to_string(source.join("PKGBUILD")).context("reading local PKGBUILD")?;
        let pins = read_optional(&source.join("version.sh"))?;
        ensure!(
            preview || pins.is_some(),
            "local release pins missing; run local-package-versions first"
        );
        let pending_pins = pins.is_none();
        let mut files = vec![("PKGBUILD", recipe)];
        if let Some(pins) = pins {
            files.push(("version.sh", pins));
        }
        if let Some(script) = read_optional(&source.join("package.install"))? {
            files.push(("package.install", script));
        }
        let snapshot = Self {
            path,
            files,
            pending_pins,
        };
        snapshot.open_owned()?;
        Ok(snapshot)
    }

    fn open_owned(&self) -> Result<Option<git2::Repository>> {
        let Some(metadata) = crate::infra::fs::symlink_metadata_optional(
            &self.path,
            "inspect local build snapshot",
        )?
        else {
            return Ok(None);
        };
        ensure!(
            metadata.is_dir() && !metadata.is_symlink(),
            "local build snapshot is not a real directory: {}",
            self.path.display()
        );
        let git_dir = self.path.join(".git");
        let git_metadata = std::fs::symlink_metadata(&git_dir).with_context(|| {
            format!(
                "refusing to overwrite unmanaged build directory {}",
                self.path.display()
            )
        })?;
        ensure!(
            git_metadata.is_dir() && !git_metadata.is_symlink(),
            "local build snapshot must have its own Git directory"
        );
        ensure!(
            read_optional(&git_dir.join("dotfiles-local-build"))?.as_deref() == Some(MARKER),
            "refusing to overwrite unmanaged build repository {}",
            self.path.display()
        );
        let repo = git2::Repository::open(&self.path).context("opening local build snapshot")?;
        ensure!(
            repo.path().canonicalize()? == git_dir.canonicalize()?
                && repo.commondir().canonicalize()? == git_dir.canonicalize()?
                && repo
                    .workdir()
                    .map(Path::canonicalize)
                    .transpose()?
                    .as_deref()
                    == Some(self.path.canonicalize()?.as_path())
                && !repo.is_worktree(),
            "local build snapshot points outside its directory"
        );
        Ok(Some(repo))
    }

    pub fn needs_write(&self) -> Result<bool> {
        if self.pending_pins {
            return Ok(true);
        }
        let Some(repo) = self.open_owned()? else {
            return Ok(true);
        };
        let tree = match repo.head() {
            Ok(head) => Some(head.peel_to_tree()?),
            Err(error)
                if matches!(
                    error.code(),
                    git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
                ) =>
            {
                None
            }
            Err(error) => return Err(error).context("reading local build snapshot HEAD"),
        };
        if tree
            .as_ref()
            .is_some_and(|tree| tree.len() != self.files.len())
        {
            return Ok(true);
        }
        for (name, content) in &self.files {
            if read_optional(&self.path.join(name))?.as_deref() != Some(content) {
                return Ok(true);
            }
            let Some(tree) = &tree else { return Ok(true) };
            let Some(entry) = tree.get_name(name) else {
                return Ok(true);
            };
            if repo.find_blob(entry.id())?.content() != content.as_bytes() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn apply(&self) -> Result<()> {
        ensure!(
            !self.pending_pins,
            "cannot publish snapshot without release pins"
        );
        let repo = if let Some(repo) = self.open_owned()? {
            repo
        } else {
            let repo =
                git2::Repository::init(&self.path).context("initializing local build snapshot")?;
            crate::infra::fs::write_atomic(&repo.path().join("dotfiles-local-build"), MARKER)?;
            repo
        };
        // Keep package inputs byte-for-byte stable through Git reset/clean,
        // regardless of the host's global Windows line-ending preferences.
        repo.config()?.set_bool("core.autocrlf", false)?;
        let parent = match repo.head() {
            Ok(head) => Some(head.peel_to_commit()?),
            Err(error)
                if matches!(
                    error.code(),
                    git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
                ) =>
            {
                None
            }
            Err(error) => return Err(error).context("reading local build snapshot parent"),
        };
        for (name, content) in &self.files {
            crate::infra::fs::write_atomic(&self.path.join(name), content)?;
        }
        let mut index = repo.index()?;
        index.clear()?;
        for (name, _) in &self.files {
            index.add_path(Path::new(name))?;
        }
        index.write()?;
        let tree = repo.find_tree(index.write_tree()?)?;
        if parent
            .as_ref()
            .is_none_or(|commit| commit.tree_id() != tree.id())
        {
            let signature = git2::Signature::now("dotfiles", "dotfiles@localhost")?;
            repo.commit(
                Some("HEAD"),
                &signature,
                &signature,
                "Refresh local package build inputs",
                &tree,
                &parent.iter().collect::<Vec<_>>(),
            )?;
        }
        Ok(())
    }
}
