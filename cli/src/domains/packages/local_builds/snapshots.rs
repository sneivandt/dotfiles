//! Disposable, committed build inputs that Paru may safely reset and clean.

use super::{ReleaseSource, read_optional, safe_component};
use crate::engine::Context;
use crate::infra::{exec::CommandSpec, fs::TempGuard};
use anyhow::{Context as _, Result, ensure};
use std::path::{Path, PathBuf};

const MARKER: &str = "dotfiles local PKGBUILD snapshot v1\n";
const ATTRIBUTES: &str = "* -text\n";

pub(super) struct Snapshot {
    pub path: PathBuf,
    files: Vec<(&'static str, String)>,
    pending_pins: bool,
    srcinfo: Option<String>,
}

impl Snapshot {
    fn validation_key(&self, source: &ReleaseSource, srcinfo: &str) -> Result<String> {
        use base64::Engine as _;
        use sha2::{Digest as _, Sha256};
        // Include the validation protocol and descriptor, not just PKGBUILD bytes.
        let inputs = serde_json::to_vec(&(2_u32, &self.files, source, srcinfo))?;
        Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(inputs)))
    }

    fn validation_path(&self) -> PathBuf {
        self.path.join(".git/dotfiles-validated-metadata")
    }

    pub fn record_validation(&self, source: &ReleaseSource, srcinfo: &str) -> Result<()> {
        let key = self.validation_key(source, srcinfo)?;
        let path = self.validation_path();
        if read_optional(&path)?.as_deref() != Some(&key) {
            crate::infra::fs::write_atomic(&path, &key)?;
        }
        Ok(())
    }

    pub fn validated_metadata(&self, source: &ReleaseSource) -> Result<Option<&str>> {
        if let Some(srcinfo) = self.srcinfo.as_deref()
            && !self.needs_write()?
            && read_optional(&self.validation_path())?.as_deref()
                == Some(self.validation_key(source, srcinfo)?.as_str())
        {
            return Ok(Some(srcinfo));
        }
        Ok(None)
    }

    // PKGBUILDs are executable code. Evaluate only during apply, from copies of
    // the exact planned inputs, before changing the published build repository.
    pub fn validate_metadata(&self, ctx: &Context, source: &ReleaseSource) -> Result<String> {
        if let Some(srcinfo) = self.validated_metadata(source)? {
            return Ok(srcinfo.to_owned());
        }
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
        source.validate_srcinfo(&output.stdout)?;
        Ok(output.stdout)
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
        for name in ["package.install", "LICENSE", "NOTICE"] {
            if let Some(content) = read_optional(&source.join(name))? {
                files.push((name, content));
            }
        }
        let mut snapshot = Self {
            path,
            files,
            pending_pins,
            srcinfo: None,
        };
        if let Some(repo) = snapshot.open_owned()? {
            match repo.head() {
                Ok(head) => {
                    let tree = head.peel_to_tree()?;
                    if let Some(entry) = tree.get_name(".SRCINFO") {
                        let blob = repo.find_blob(entry.id())?;
                        snapshot.srcinfo = Some(std::str::from_utf8(blob.content())?.to_owned());
                    }
                }
                Err(error)
                    if matches!(
                        error.code(),
                        git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
                    ) => {}
                Err(error) => return Err(error).context("reading local build snapshot HEAD"),
            }
        }
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
        let Some(srcinfo) = self.srcinfo.as_deref() else {
            return Ok(true);
        };
        let Some(repo) = self.open_owned()? else {
            return Ok(true);
        };
        if read_optional(&repo.path().join("info/attributes"))?.as_deref() != Some(ATTRIBUTES) {
            return Ok(true);
        }
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
            .is_some_and(|tree| tree.len() != self.files.len() + 1)
        {
            return Ok(true);
        }
        for (name, content) in self.inputs(srcinfo) {
            let metadata = crate::infra::fs::symlink_metadata_optional(
                &self.path.join(name),
                "inspect local build snapshot input",
            )?;
            if metadata.is_none_or(|metadata| !metadata.is_file()) {
                return Ok(true);
            }
            if read_optional(&self.path.join(name))?.as_deref() != Some(content) {
                return Ok(true);
            }
            let Some(tree) = &tree else { return Ok(true) };
            let Some(entry) = tree.get_name(name) else {
                return Ok(true);
            };
            if !matches!(entry.filemode(), 0o100_644 | 0o100_755) {
                return Ok(true);
            }
            if repo.find_blob(entry.id())?.content() != content.as_bytes() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn metadata_changed(&self, srcinfo: &str) -> bool {
        self.srcinfo.as_deref() != Some(srcinfo)
    }

    fn inputs<'a>(&'a self, srcinfo: &'a str) -> impl Iterator<Item = (&'a str, &'a str)> {
        self.files
            .iter()
            .map(|(name, content)| (*name, content.as_str()))
            .chain(std::iter::once((".SRCINFO", srcinfo)))
    }

    pub fn apply(&self, srcinfo: &str) -> Result<()> {
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
        // including hosts whose global attributes enable text normalization.
        repo.config()?.set_bool("core.autocrlf", false)?;
        crate::infra::fs::write_atomic(&repo.path().join("info/attributes"), ATTRIBUTES)?;
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
        for (name, content) in self.inputs(srcinfo) {
            crate::infra::fs::write_atomic(&self.path.join(name), content)?;
        }
        let mut index = repo.index()?;
        index.clear()?;
        for (name, _) in self.inputs(srcinfo) {
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
