use super::ownership::{check_active_files, inventory_files, prune_stale_files};
use super::{
    BTreeMap, BTreeSet, FileInventory, Path, PathBuf, Result, create_native_symlink, is_dir_like,
    reject_destination_link, symlink_metadata_optional, write_atomic,
};
use anyhow::Context as _;

pub(super) fn prepare_skill_paths(
    target: &Path,
    previous: Option<&FileInventory>,
    desired: &FileInventory,
) -> Result<bool> {
    let retiring_directories = check_active_files(target, previous, desired)?;
    let mut pruned = false;
    if let Some(previous) = previous {
        pruned |= prune_stale_files(target, previous, desired)?;
    }
    for directory in retiring_directories {
        reject_destination_link(&directory)?;
        std::fs::remove_dir(&directory)
            .with_context(|| format!("retiring Cowork directory {}", directory.display()))?;
        pruned = true;
    }
    Ok(pruned)
}

pub(super) fn selected_skill_inventory(source: &Path) -> Result<FileInventory> {
    // The lock still selects this skill. An incomplete shared deployment
    // must not retire the installed entry point or its owned assets.
    let entry_point = source.join("SKILL.md");
    let entry_meta = std::fs::metadata(&entry_point).with_context(|| {
        format!(
            "APM shared skill entry point {} is missing or inaccessible",
            entry_point.display()
        )
    })?;
    anyhow::ensure!(
        entry_meta.is_file(),
        "APM shared skill entry point {} is not a file",
        entry_point.display()
    );
    let mut files = FileInventory::new();
    inventory_files(source, source, &mut files)?;
    Ok(files)
}

// Resolve payloads against the entries the copier will actually publish, not
// against the source's parent or extra files already present in Cowork.
fn collect_skill_entries(
    root: &Path,
    directory: &Path,
    entries: &mut BTreeMap<PathBuf, Option<PathBuf>>,
) -> Result<()> {
    reject_destination_link(directory)?;
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        let payload = if kind.is_symlink() {
            Some(std::fs::read_link(&path)?)
        } else {
            if kind.is_dir() {
                collect_skill_entries(root, &path, entries)?;
            } else if !kind.is_file() {
                continue;
            }
            None
        };
        entries.insert(path.strip_prefix(root)?.to_owned(), payload);
    }
    Ok(())
}

fn relocated_referent(
    link: &Path,
    payload: &Path,
    entries: &BTreeMap<PathBuf, Option<PathBuf>>,
) -> Result<PathBuf> {
    use std::path::Component;

    let mut path = link
        .parent()
        .context("skill link has no parent")?
        .to_owned();
    let mut pending: Vec<_> = payload.components().rev().collect();
    // A finite iterator supplies the budget even when a link adds components
    // to the pending path on each expansion.
    let mut hops = 0..63;
    while let Some(part) = pending.pop() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                anyhow::ensure!(path.pop(), "link escapes the skill directory");
            }
            Component::RootDir | Component::Prefix(_) => anyhow::bail!("link must be relative"),
            Component::Normal(name) => {
                path.push(name);
                if let Some(next) = entries
                    .get(&path)
                    .with_context(|| format!("{} is not in the copied skill", path.display()))?
                {
                    hops.next()
                        .context("cyclic or excessively deep link chain")?;
                    path.pop();
                    pending.extend(next.components().rev());
                }
            }
        }
    }
    Ok(path)
}

fn validate_skill_link(
    skill: &Path,
    link: &Path,
    payload: &Path,
    entries: &BTreeMap<PathBuf, Option<PathBuf>>,
) -> Result<()> {
    let referent = relocated_referent(link, payload, entries)?;
    anyhow::ensure!(
        std::fs::canonicalize(skill.join(link))? == std::fs::canonicalize(skill.join(referent))?,
        "source link resolves differently than its copied payload"
    );
    Ok(())
}

fn check_link_replacement(target: &Path, existing: Option<&std::fs::Metadata>) -> Result<()> {
    anyhow::ensure!(
        !existing.is_some_and(std::fs::Metadata::is_file),
        "cannot preserve access when replacing regular Cowork file {} with a symbolic link; move it aside and retry",
        target.display()
    );
    Ok(())
}

fn validate_link_destination(skill: &Path, relative: &Path) -> Result<()> {
    let mut path = skill.to_owned();
    for part in relative {
        // Below a missing or non-directory entry there is no destination file
        // to replace. Never follow a link to inspect its descendants here.
        if !symlink_metadata_optional(&path, "inspect Cowork link parent")?
            .is_some_and(|meta| meta.is_dir())
        {
            return Ok(());
        }
        reject_destination_link(&path)?;
        path.push(part);
    }
    check_link_replacement(
        &path,
        symlink_metadata_optional(&path, "inspect Cowork link destination")?.as_ref(),
    )
}

pub(super) fn validate_selected_links(
    source: &Path,
    target: &Path,
    names: &BTreeSet<String>,
) -> Result<()> {
    for name in names {
        let skill = source.join(name);
        if symlink_metadata_optional(&skill, "inspect shared skill")?.is_none() {
            // A missing skill still follows the normal per-skill retry path.
            continue;
        }
        let mut entries = BTreeMap::new();
        collect_skill_entries(&skill, &skill, &mut entries)?;
        for (link, payload) in &entries {
            if let Some(payload) = payload {
                validate_skill_link(&skill, link, payload, &entries).with_context(|| {
                    format!(
                        "cannot relocate Cowork skill link {} -> {}",
                        skill.join(link).display(),
                        payload.display()
                    )
                })?;
                validate_link_destination(&target.join(name), link)?;
            }
        }
    }
    Ok(())
}

/// Compare only APM-owned source entries; preserve Cowork-owned extra files.
pub(super) fn skill_files_match(source: &Path, target: &Path) -> Result<bool> {
    let source_meta = source.symlink_metadata()?;
    let target_meta = match target.symlink_metadata() {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).with_context(|| format!("reading {}", target.display())),
    };
    match (
        target_meta.file_type().is_symlink(),
        source_meta.file_type().is_symlink(),
    ) {
        (false, _) => reject_destination_link(target)?,
        (true, false) if source_meta.is_dir() || is_dir_like(&target_meta) => {
            reject_destination_link(target)?;
        }
        _ => {}
    }
    if source_meta.file_type() != target_meta.file_type() {
        return Ok(false);
    }
    if source_meta.file_type().is_symlink() {
        return Ok(std::fs::read_link(source)? == std::fs::read_link(target)?);
    }
    // Once installed, access belongs to the destination. Different access
    // modes alone must not trigger a rewrite or a convergence failure.
    if source_meta.is_dir() {
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            if !skill_files_match(&entry.path(), &target.join(entry.file_name()))? {
                return Ok(false);
            }
        }
        Ok(true)
    } else {
        Ok(std::fs::read(source)? == std::fs::read(target)?)
    }
}

/// Merge changed entries without replacing Cowork directories or following
/// destination links. A fresh-tree copier cannot update existing symlinks.
pub(super) fn copy_skill_files(source: &Path, target: &Path) -> Result<()> {
    if skill_files_match(source, target)? {
        return Ok(());
    }
    let source_meta = source.symlink_metadata()?;
    let target_meta = symlink_metadata_optional(target, "inspecting Cowork skill destination")?;
    if let Some(meta) = &target_meta {
        match (
            meta.file_type().is_symlink(),
            source_meta.file_type().is_symlink(),
        ) {
            (false, _) => reject_destination_link(target)?,
            (true, false) if is_dir_like(meta) => reject_destination_link(target)?,
            _ => {}
        }
    }
    if source_meta.is_dir() {
        anyhow::ensure!(
            target_meta.as_ref().is_none_or(std::fs::Metadata::is_dir),
            "Cowork skill directory is not a regular directory: {}",
            target.display()
        );
        if target_meta.is_none() {
            let builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            let builder = {
                use std::os::unix::fs::DirBuilderExt as _;
                let mut builder = builder;
                builder.mode(0o700);
                builder
            };
            builder.create(target)?;
        }
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            copy_skill_files(&entry.path(), &target.join(entry.file_name()))?;
        }
        #[cfg(unix)]
        if target_meta.is_none() {
            std::fs::set_permissions(target, source_meta.permissions())?;
        }
    } else {
        if let Some(meta) = &target_meta {
            anyhow::ensure!(
                !is_dir_like(meta) || meta.file_type().is_symlink(),
                "cannot replace Cowork-owned directory {}",
                target.display()
            );
        }
        if source_meta.file_type().is_symlink() {
            check_link_replacement(target, target_meta.as_ref())?;
            let parent = target.parent().context("Cowork symlink has no parent")?;
            let staged =
                crate::infra::fs::TempGuard::create_unique_dir(parent, ".dotfiles-cowork", "link")?;
            let link = staged.path().join("entry");
            create_native_symlink(
                &std::fs::read_link(source)?,
                &link,
                is_dir_like(&source_meta),
            )?;
            let current = symlink_metadata_optional(target, "inspect Cowork link publication")?;
            check_link_replacement(target, current.as_ref())?;
            match (target_meta, current) {
                (None, _) | (_, None) => crate::infra::fs::rename_noreplace(&link, target)?,
                (Some(_), Some(current)) => {
                    #[cfg(not(windows))]
                    let _ = &current;
                    #[cfg(windows)]
                    {
                        use std::os::windows::fs::FileTypeExt as _;
                        if current.file_type().is_symlink_dir() {
                            let previous = staged.path().join("previous");
                            std::fs::rename(target, &previous)?;
                            if let Err(error) = crate::infra::fs::rename_into_place(&link, target) {
                                if let Err(restore) = std::fs::rename(&previous, target) {
                                    let mut staged = staged;
                                    staged.persist();
                                    anyhow::bail!(
                                        "Cowork link publication failed: {error:#}; restore failed: {restore}; original preserved at {}",
                                        previous.display()
                                    );
                                }
                                return Err(error);
                            }
                            return Ok(());
                        }
                    }
                    crate::infra::fs::rename_into_place(&link, target)?;
                }
            }
        } else {
            copy_regular_skill_file(source, target, target_meta.as_ref())?;
        }
    }
    Ok(())
}

fn copy_regular_skill_file(
    source: &Path,
    target: &Path,
    existing: Option<&std::fs::Metadata>,
) -> Result<()> {
    if existing.is_some_and(std::fs::Metadata::is_file) {
        // The existing policy, not the shared skill's mode, must govern the
        // replacement. Let the shared writer check it before staging bytes.
        #[cfg(any(windows, target_os = "linux"))]
        return write_atomic(target, std::fs::read(source)?);
        #[cfg(not(any(windows, target_os = "linux")))]
        anyhow::bail!(
            "cannot preserve access for existing Cowork file {}",
            target.display()
        );
    }

    let parent = target.parent().context("Cowork file has no parent")?;
    let (mut staged, file) = crate::infra::fs::TempGuard::create_unique_file_with_mode(
        parent,
        ".dotfiles-cowork",
        "tmp",
        0o600,
    )?;
    drop(file);
    // std::fs::copy carries the install mode (including Unix execute bits).
    std::fs::copy(source, staged.path())?;
    if existing.is_none() {
        crate::infra::fs::rename_noreplace(staged.path(), target)?;
    } else {
        crate::infra::fs::rename_into_place(staged.path(), target)?;
    }
    staged.persist();
    Ok(())
}
