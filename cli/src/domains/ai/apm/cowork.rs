//! Microsoft 365 Copilot Cowork deployment repair.
//!
//! Cowork protects its `OneDrive` skill directories from deletion. Current APM
//! still replaces a colliding skill directory with `rmtree` + `copytree`, so
//! dotfiles copies the already-resolved shared skill tree into each existing
//! Cowork directory without replacing that directory. New files take their
//! install permissions from the source; later content updates keep destination
//! access or refuse the replacement if it cannot be preserved.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
pub(super) mod overlap;

use crate::infra::yaml::Value;
use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use super::targets::copilot_cowork_skills_path;
use crate::engine::Context;
#[cfg(windows)]
use crate::infra::fs::stored_entry_name;
use crate::infra::fs::{
    create_native_symlink, is_dir_like, reject_destination_link, symlink_metadata_optional,
    write_atomic,
};

const COWORK_TARGET: &str = "copilot-cowork";
const COWORK_URI_PREFIX: &str = "cowork://";
const OWNERSHIP_FILE: &str = ".dotfiles-apm-skills.json";
const FILE_OWNERSHIP_FILE: &str = ".dotfiles-apm-files.json";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct OwnedFile {
    hash: String,
    link: bool,
}

type FileInventory = BTreeMap<String, OwnedFile>;
type SkillInventory = BTreeMap<String, FileInventory>;

fn ownership_key(root: &Path, relative: &str) -> Result<String> {
    #[cfg(not(windows))]
    let _ = root;
    #[cfg(windows)]
    let mut directory = Some(root.to_owned());
    let mut key = Vec::new();
    for part in Path::new(relative) {
        let name = part.to_str().context("non-UTF-8 Cowork path")?;
        // Below a missing or non-directory prefix there is no stored spelling.
        // The ordinary fingerprint checks handle file/directory transitions.
        #[cfg(windows)]
        if let Some(parent) = directory.take()
            && symlink_metadata_optional(&parent, "inspect Cowork parent")?
                .is_some_and(|m| m.is_dir())
        {
            reject_destination_link(&parent)?;
            if let Some(stored) = stored_entry_name(&parent.join(name))? {
                directory = Some(parent.join(&stored));
                key.push(stored.to_str().context("non-UTF-8 Cowork path")?.to_owned());
                continue;
            }
        }
        key.push(name.to_owned());
    }
    Ok(key.join("/"))
}

fn index_files<'a>(
    target: &Path,
    inventories: impl Iterator<Item = &'a FileInventory>,
    prior: bool,
) -> Result<FileInventory> {
    let mut indexed = FileInventory::new();
    for (relative, file) in inventories.flatten() {
        let key = ownership_key(target, relative)?;
        if let Some(other) = indexed.insert(key, file.clone()) {
            anyhow::ensure!(
                prior && other == *file,
                "ambiguous Cowork file ownership at {}",
                target.join(relative).display()
            );
        }
    }
    Ok(indexed)
}

fn prior_files(
    target: &Path,
    inventory: &SkillInventory,
    aliases: &BTreeSet<String>,
) -> Result<Option<FileInventory>> {
    let mut records = aliases
        .iter()
        .filter_map(|alias| inventory.get(alias))
        .peekable();
    if records.peek().is_none() {
        return Ok(None);
    }
    index_files(target, records, true).map(Some)
}

fn write_file_inventory(target: &Path, inventory: &SkillInventory) -> Result<()> {
    write_atomic(
        &target.join(FILE_OWNERSHIP_FILE),
        serde_json::to_vec_pretty(inventory)?,
    )
}

fn file_fingerprint(path: &Path) -> Result<Option<OwnedFile>> {
    let Some(metadata) = symlink_metadata_optional(path, "inspect owned Cowork file")? else {
        return Ok(None);
    };
    if metadata.is_symlink() {
        return Ok(Some(OwnedFile {
            hash: crate::infra::fs::content_hash(
                std::fs::read_link(path)?.as_os_str().as_encoded_bytes(),
            ),
            link: true,
        }));
    }
    if !metadata.is_file() {
        return Ok(None);
    }
    Ok(Some(OwnedFile {
        hash: crate::infra::fs::content_hash(std::fs::read(path)?),
        link: false,
    }))
}

fn inventory_files(root: &Path, path: &Path, files: &mut FileInventory) -> Result<()> {
    let metadata = path.symlink_metadata()?;
    if metadata.is_dir() && !metadata.is_symlink() {
        for entry in std::fs::read_dir(path)? {
            inventory_files(root, &entry?.path(), files)?;
        }
    } else {
        let fingerprint = file_fingerprint(path)?.context("unsupported Cowork source file")?;
        let relative = path
            .strip_prefix(root)?
            .components()
            .map(|part| {
                let segment = part
                    .as_os_str()
                    .to_str()
                    .context("Cowork ownership paths must be UTF-8")?;
                anyhow::ensure!(
                    !segment.contains(['\\', ':']),
                    "unsupported Cowork ownership path {}",
                    path.display()
                );
                Ok(segment)
            })
            .collect::<Result<Vec<_>>>()?
            .join("/");
        anyhow::ensure!(
            !relative.is_empty(),
            "Cowork skill root must be a regular directory: {}",
            root.display()
        );
        files.insert(relative, fingerprint);
    }
    Ok(())
}

fn read_file_inventory(target: &Path) -> Result<SkillInventory> {
    let bytes = match std::fs::read(target.join(FILE_OWNERSHIP_FILE)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(error).context("reading Cowork file ownership"),
    };
    let inventory: SkillInventory =
        serde_json::from_slice(&bytes).context("parsing Cowork file ownership")?;
    for (name, files) in &inventory {
        anyhow::ensure!(
            valid_skill_name(name),
            "invalid skill name in Cowork file ownership"
        );
        for relative in files.keys() {
            let path = Path::new(relative);
            anyhow::ensure!(
                !relative.is_empty()
                    && path
                        .components()
                        .all(|part| matches!(part, std::path::Component::Normal(_))),
                "unsafe Cowork ownership path {}",
                path.display()
            );
            anyhow::ensure!(
                !relative.contains(['\\', ':']),
                "unsafe Cowork ownership path {}",
                path.display()
            );
        }
    }
    Ok(inventory)
}

// Inspect from the skill root down so even a linked grandparent is rejected
// before we read or delete anything beneath it.
fn checked_file_path(target: &Path, relative: &str) -> Result<PathBuf> {
    let mut components = Path::new(relative).components();
    let file = components.next_back().context("owned file has no name")?;
    let mut path = target.to_path_buf();
    reject_destination_link(&path)?;
    for component in components {
        path.push(component);
        reject_destination_link(&path)?;
    }
    Ok(path.join(file))
}

fn check_retiring_directory(
    target: &Path,
    relative: &str,
    previous: &FileInventory,
    directories: &mut Vec<PathBuf>,
) -> Result<()> {
    let path = target.join(relative);
    reject_destination_link(&path)?;
    let prefix = format!("{relative}/");
    anyhow::ensure!(
        previous.keys().any(|owned| owned.starts_with(&prefix)),
        "cannot replace unowned Cowork directory {}",
        path.display()
    );
    for entry in std::fs::read_dir(&path)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .context("Cowork ownership paths must be UTF-8")?;
        let child = format!("{prefix}{name}");
        let child_path = entry.path();
        let metadata = entry.file_type()?;
        if metadata.is_dir() {
            check_retiring_directory(target, &child, previous, directories)?;
        } else {
            anyhow::ensure!(
                previous.contains_key(&child)
                    && file_fingerprint(&child_path)?.as_ref() == previous.get(&child),
                "preserving unowned or modified Cowork file {}; move it aside and retry",
                child_path.display()
            );
        }
    }
    directories.push(path);
    Ok(())
}

fn ancestor_paths(relative: &str) -> impl Iterator<Item = &str> {
    relative
        .match_indices('/')
        .map(|(index, _)| &relative[..index])
}

fn check_active_files(
    target: &Path,
    previous: Option<&FileInventory>,
    desired: &FileInventory,
) -> Result<Vec<PathBuf>> {
    let mut directories = Vec::new();
    'files: for (relative, fingerprint) in desired {
        if let Some(previous) = previous {
            for ancestor in ancestor_paths(relative) {
                if let Some(expected) = previous.get(ancestor) {
                    let path = checked_file_path(target, ancestor)?;
                    if let Some(current) = file_fingerprint(&path)? {
                        anyhow::ensure!(
                            current == *expected,
                            "preserving modified Cowork file {}; move it aside and retry",
                            path.display()
                        );
                        // The unchanged owned entry will be pruned before copying.
                        continue 'files;
                    }
                }
            }
        }
        let path = checked_file_path(target, relative)?;
        let Some(metadata) = symlink_metadata_optional(&path, "inspect Cowork destination")? else {
            continue;
        };
        if !metadata.is_symlink() {
            reject_destination_link(&path)?;
            if metadata.is_dir()
                && let Some(previous) = previous
            {
                check_retiring_directory(target, relative, previous, &mut directories)?;
                continue;
            }
            anyhow::ensure!(
                metadata.is_file(),
                "cannot replace Cowork-owned directory or special file {}",
                path.display()
            );
        }
        let current = file_fingerprint(&path)?.context("unsupported Cowork destination file")?;
        anyhow::ensure!(
            current == *fingerprint
                || previous.and_then(|files| files.get(relative)) == Some(&current),
            "preserving unowned or modified Cowork file {}; move it aside and retry",
            path.display()
        );
    }
    Ok(directories)
}

fn prune_stale_files(
    target: &Path,
    previous: &FileInventory,
    desired: &FileInventory,
) -> Result<bool> {
    let mut changed = false;
    for (relative, fingerprint) in previous {
        if desired.contains_key(relative) {
            continue;
        }
        // A prior attempt can have published the new file before saving its inventory.
        let mut replaced_parent = false;
        for ancestor in ancestor_paths(relative) {
            if let Some(expected) = desired.get(ancestor) {
                let path = checked_file_path(target, ancestor)?;
                if file_fingerprint(&path)?.as_ref() == Some(expected) {
                    replaced_parent = true;
                    break;
                }
            }
        }
        if replaced_parent {
            continue;
        }
        let path = checked_file_path(target, relative)?;
        let Some(metadata) = symlink_metadata_optional(&path, "inspect stale Cowork file")? else {
            continue;
        };
        if metadata.is_dir() {
            reject_destination_link(&path)?;
            let prefix = format!("{relative}/");
            if desired.keys().any(|file| file.starts_with(&prefix)) {
                continue;
            }
        }
        anyhow::ensure!(
            file_fingerprint(&path)?.as_ref() == Some(fingerprint),
            "preserving modified stale Cowork file {}; move it aside and retry",
            path.display()
        );
        if metadata.is_symlink() && is_dir_like(&metadata) {
            std::fs::remove_dir(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
        changed = true;
    }
    Ok(changed)
}

fn prepare_skill_paths(
    target: &Path,
    previous: Option<&FileInventory>,
    desired: &FileInventory,
) -> Result<()> {
    let retiring_directories = check_active_files(target, previous, desired)?;
    if let Some(previous) = previous {
        prune_stale_files(target, previous, desired)?;
    }
    for directory in retiring_directories {
        reject_destination_link(&directory)?;
        std::fs::remove_dir(&directory)
            .with_context(|| format!("retiring Cowork directory {}", directory.display()))?;
    }
    Ok(())
}

fn selected_skill_inventory(source: &Path) -> Result<FileInventory> {
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
    let mut hops = 0;
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
                    // Bound expansion of cycles even when a payload adds a suffix.
                    hops += 1;
                    anyhow::ensure!(hops <= 63, "cyclic or excessively deep link chain");
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

fn validate_selected_links(source: &Path, target: &Path, names: &BTreeSet<String>) -> Result<()> {
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

/// Reconcile APM's shared skill deployment into Cowork's protected skill tree.
///
/// # Errors
///
/// Returns an error when the configured Cowork path, shared APM skills, lock
/// state, or a managed file cannot be read or written.
pub(super) fn reconcile_cowork_skills(ctx: &Context) -> Result<bool> {
    let (source, target) = cowork_skill_paths(ctx)?;
    reject_destination_link(&target)?;
    let desired = desired_cowork_skill_names(ctx.home())?;
    validate_selected_links(&source, &target, &desired)?;
    let mut changed = remove_legacy_cowork_lock_deployments(ctx)?;
    let mut owned = read_owned_skills(&target)?;
    let mut inventory = read_file_inventory(&target)?;
    changed |= !target.exists();
    std::fs::create_dir_all(&target)
        .with_context(|| format!("creating Copilot Cowork skill target {}", target.display()))?;

    let mut prior_names: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
    for name in owned.iter().chain(inventory.keys()) {
        prior_names
            .entry(ownership_key(&target, name)?)
            .or_default()
            .insert(name.clone());
    }
    let mut selected = BTreeSet::new();
    let mut selected_sources = BTreeSet::new();
    for name in &desired {
        anyhow::ensure!(
            selected.insert(ownership_key(&target, name)?)
                && selected_sources.insert(ownership_key(&source, name)?),
            "ambiguous selected Cowork skill {name}"
        );
    }

    let mut installed = BTreeSet::new();
    for name in &desired {
        let key = ownership_key(&target, name)?;
        anyhow::ensure!(
            !installed.contains(&key),
            "ambiguous selected Cowork skill {name}"
        );
        let aliases = prior_names.remove(&key).unwrap_or_default();
        changed |= reconcile_selected(
            ctx,
            name,
            &source,
            &target,
            &aliases,
            &mut owned,
            &mut inventory,
        )?;
        installed.insert(ownership_key(&target, name)?);
    }

    for (name, aliases) in prior_names {
        let skill = target.join(&name);
        // If the target directory was missing at discovery, its old spelling
        // can resolve to a newly installed directory only now. Retire the
        // obsolete record, not the files we just published.
        let removed = if installed.contains(&ownership_key(&target, &name)?) {
            false
        } else if let Some(previous) = prior_files(&skill, &inventory, &aliases)? {
            prune_stale_files(&skill, &previous, &FileInventory::new())?
        } else {
            check_legacy_entry_point(&skill)?;
            false
        };
        if removed {
            changed = true;
            ctx.log().action(
                "remove",
                &format!("Copilot Cowork skill {name}"),
                false,
                &format!("remove Copilot Cowork skill {name}"),
            );
        }
        let before = inventory.len();
        for alias in &aliases {
            inventory.remove(alias);
        }
        if inventory.len() != before {
            write_file_inventory(&target, &inventory)?;
            changed = true;
        }
    }
    if owned != desired {
        write_owned_skills(&target, &desired)?;
        changed = true;
    }

    Ok(changed)
}

fn reconcile_selected(
    ctx: &Context,
    name: &str,
    source: &Path,
    target: &Path,
    aliases: &BTreeSet<String>,
    owned: &mut BTreeSet<String>,
    inventory: &mut SkillInventory,
) -> Result<bool> {
    let source_skill = source.join(name);
    let target_skill = target.join(name);
    let files = selected_skill_inventory(&source_skill)?;
    let previous = prior_files(&target_skill, inventory, aliases)?;
    let active = index_files(&target_skill, std::iter::once(&files), false)?;
    prepare_skill_paths(&target_skill, previous.as_ref(), &active)?;
    let mut changed = !skill_files_match(&source_skill, &target_skill)?;
    if changed {
        copy_skill_files(&source_skill, &target_skill).with_context(|| {
            format!(
                "reconciling APM skill {name} into Copilot Cowork at {}",
                target_skill.display()
            )
        })?;
        anyhow::ensure!(
            skill_files_match(&source_skill, &target_skill)?,
            "Copilot Cowork skill {name} did not converge at {}",
            target_skill.display()
        );
        // A case-sensitive source can select two not-yet-existing names for
        // one destination. Equal bytes alone do not prove those entries are
        // distinct. Never publish an ambiguous inventory.
        #[cfg(windows)]
        index_files(&target_skill, std::iter::once(&files), false)?;
        ctx.log().action(
            "update",
            &format!("Copilot Cowork skill {name}"),
            false,
            &format!("update Copilot Cowork skill {name}"),
        );
    }
    // Only migrate the keys after active and stale fingerprints have been
    // checked and the copy succeeded. Unioning both ledgers on read also
    // makes the two atomic writes recoverable independently.
    let mut renamed = false;
    let mut retired = false;
    for alias in aliases.iter().filter(|alias| alias.as_str() != name) {
        renamed |= owned.remove(alias);
        retired |= inventory.remove(alias).is_some();
    }
    if owned.insert(name.to_owned()) || renamed {
        write_owned_skills(target, owned)?;
        changed = true;
    }
    if inventory.get(name) != Some(&files) || retired {
        inventory.insert(name.to_owned(), files);
        write_file_inventory(target, inventory)?;
        changed = true;
    }
    Ok(changed)
}

fn read_owned_skills(target: &Path) -> Result<BTreeSet<String>> {
    reject_destination_link(target)?;
    let path = target.join(OWNERSHIP_FILE);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("reading {}", path.display()));
        }
    };
    let names: BTreeSet<String> =
        serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))?;
    anyhow::ensure!(
        names.iter().all(|name| valid_skill_name(name)),
        "invalid skill name in {}",
        path.display()
    );
    Ok(names)
}

fn write_owned_skills(target: &Path, names: &BTreeSet<String>) -> Result<()> {
    reject_destination_link(target)?;
    let path = target.join(OWNERSHIP_FILE);
    let content = serde_json::to_string_pretty(names).context("serializing Cowork ownership")?;
    write_atomic(&path, format!("{content}\n"))
        .with_context(|| format!("writing {}", path.display()))
}

fn valid_skill_name(name: &str) -> bool {
    !name.is_empty() && !matches!(name, "." | "..") && !name.contains(['/', '\\', ':'])
}

/// Compare only APM-owned source entries; preserve Cowork-owned extra files.
fn skill_files_match(source: &Path, target: &Path) -> Result<bool> {
    let source_meta = source.symlink_metadata()?;
    let target_meta = match target.symlink_metadata() {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).with_context(|| format!("reading {}", target.display())),
    };
    if !target_meta.file_type().is_symlink()
        || (!source_meta.file_type().is_symlink()
            && (source_meta.is_dir() || is_dir_like(&target_meta)))
    {
        reject_destination_link(target)?;
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
fn copy_skill_files(source: &Path, target: &Path) -> Result<()> {
    if skill_files_match(source, target)? {
        return Ok(());
    }
    let source_meta = source.symlink_metadata()?;
    let target_meta = symlink_metadata_optional(target, "inspecting Cowork skill destination")?;
    if target_meta.as_ref().is_some_and(|meta| {
        !meta.file_type().is_symlink()
            || (is_dir_like(meta) && !source_meta.file_type().is_symlink())
    }) {
        reject_destination_link(target)?;
    }
    if source_meta.is_dir() && !source_meta.file_type().is_symlink() {
        anyhow::ensure!(
            target_meta
                .as_ref()
                .is_none_or(|meta| meta.is_dir() && !meta.file_type().is_symlink()),
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
            let mut staged =
                crate::infra::fs::TempGuard::create_unique_dir(parent, ".dotfiles-cowork", "link")?;
            let link = staged.path().join("entry");
            create_native_symlink(
                &std::fs::read_link(source)?,
                &link,
                is_dir_like(&source_meta),
            )?;
            let current = symlink_metadata_optional(target, "inspect Cowork link publication")?;
            check_link_replacement(target, current.as_ref())?;
            if target_meta.is_none() || current.is_none() {
                crate::infra::fs::rename_noreplace(&link, target)?;
            } else if current
                .is_some_and(|metadata| metadata.is_symlink() && is_dir_like(&metadata))
            {
                let previous = staged.path().join("previous");
                std::fs::rename(target, &previous)?;
                if let Err(error) = crate::infra::fs::rename_into_place(&link, target) {
                    if let Err(restore) = std::fs::rename(&previous, target) {
                        staged.persist();
                        anyhow::bail!(
                            "Cowork link publication failed: {error:#}; restore failed: {restore}; original preserved at {}",
                            previous.display()
                        );
                    }
                    return Err(error);
                }
            } else {
                crate::infra::fs::rename_into_place(&link, target)?;
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

/// Remove records left by direct APM Cowork installs.
///
/// Dotfiles owns the ACL-sensitive Cowork copy. Leaving `cowork://` records in
/// APM's ledger makes later installs retry directory replacement.
pub(super) fn remove_legacy_cowork_lock_deployments(ctx: &Context) -> Result<bool> {
    let home = ctx.home();
    let lock_path = home.join(".apm").join("apm.lock.yaml");
    let text = match std::fs::read_to_string(&lock_path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => {
            return Err(err)
                .with_context(|| format!("reading APM lockfile {}", lock_path.display()));
        }
    };
    if !text.contains(COWORK_URI_PREFIX) && !text.contains(COWORK_TARGET) {
        return Ok(false);
    }
    let mut lock: Value = crate::infra::yaml::from_str(&text)
        .with_context(|| format!("parsing APM lockfile {}", lock_path.display()))?;
    let fingerprints = legacy_entry_point_fingerprints(&lock);
    let mut legacy_skills = BTreeSet::new();
    if !strip_legacy_cowork_deployments(&mut lock, &mut legacy_skills) {
        return Ok(false);
    }
    if let Some(target) = copilot_cowork_skills_path(ctx)?
        && target
            .try_exists()
            .with_context(|| format!("checking {}", target.display()))?
    {
        reject_destination_link(&target)?;
        let mut inventory = read_file_inventory(&target)?;
        let mut imported = false;
        for (name, fingerprint) in fingerprints {
            if !inventory.contains_key(&name)
                && let Some(fingerprint) = fingerprint
            {
                inventory.insert(
                    name,
                    FileInventory::from([("SKILL.md".into(), fingerprint)]),
                );
                imported = true;
            }
        }
        if imported {
            write_atomic(
                &target.join(FILE_OWNERSHIP_FILE),
                serde_json::to_vec_pretty(&inventory)?,
            )?;
        }
        let mut owned = read_owned_skills(&target)?;
        let before = owned.len();
        owned.extend(legacy_skills);
        if owned.len() != before {
            // Preserve explicit native deployment ownership before APM rewrites
            // the lock. Shared .agents entries alone are not Cowork ownership.
            write_owned_skills(&target, &owned)?;
        }
    }
    let serialized = crate::infra::yaml::to_string(&lock)
        .with_context(|| format!("serializing APM lockfile {}", lock_path.display()))?;
    write_atomic(&lock_path, serialized)
        .with_context(|| format!("updating APM lockfile {}", lock_path.display()))?;
    Ok(true)
}

fn legacy_entry_point_fingerprints(lock: &Value) -> BTreeMap<String, Option<OwnedFile>> {
    use base64::Engine as _;

    let mut fingerprints = BTreeMap::new();
    for dependency in lock
        .get("dependencies")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(hashes) = dependency
            .get("deployed_file_hashes")
            .and_then(Value::as_object)
        else {
            continue;
        };
        for (uri, hash) in hashes {
            let Some(name) = uri
                .strip_prefix("cowork://skills/")
                .and_then(|path| path.strip_suffix("/SKILL.md"))
                .filter(|name| valid_skill_name(name))
            else {
                continue;
            };
            let candidate = hash
                .as_str()
                .and_then(|hash| hash.strip_prefix("sha256:"))
                .and_then(decode_sha256_hex)
                .map(|bytes| OwnedFile {
                    hash: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes),
                    link: false,
                });
            fingerprints
                .entry(name.to_owned())
                .and_modify(|current| {
                    if *current != candidate {
                        *current = None;
                    }
                })
                .or_insert(candidate);
        }
    }
    fingerprints
}

fn decode_sha256_hex(hash: &str) -> Option<[u8; 32]> {
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = [0; 32];
    for (pair, byte) in hash.as_bytes().as_chunks::<2>().0.iter().zip(&mut bytes) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(bytes)
}

fn strip_legacy_cowork_deployments(lock: &mut Value, names: &mut BTreeSet<String>) -> bool {
    let Some(root) = lock.as_object_mut() else {
        return false;
    };
    let mut changed = false;

    if let Some(dependencies) = root.get_mut("dependencies").and_then(Value::as_array_mut) {
        for dependency in dependencies {
            let Some(dependency) = dependency.as_object_mut() else {
                continue;
            };
            if let Some(files) = dependency
                .get_mut("deployed_files")
                .and_then(Value::as_array_mut)
            {
                let before = files.len();
                files.retain(|file| !record_legacy_cowork_uri(file, names));
                changed |= files.len() != before;
            }
            if let Some(hashes) = dependency
                .get_mut("deployed_file_hashes")
                .and_then(Value::as_object_mut)
            {
                let before = hashes.len();
                hashes
                    .retain(|key, _| !record_legacy_cowork_uri(&Value::String(key.clone()), names));
                changed |= hashes.len() != before;
            }
        }
    }

    if let Some(deployments) = root.get_mut("deployments") {
        match deployments {
            Value::Array(records) => {
                let before = records.len();
                records.retain(|record| !record_legacy_cowork_deployment(record, names));
                changed |= records.len() != before;
            }
            Value::Object(by_owner) => {
                for records in by_owner.values_mut().filter_map(Value::as_array_mut) {
                    let before = records.len();
                    records.retain(|record| !record_legacy_cowork_deployment(record, names));
                    changed |= records.len() != before;
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }

    changed
}

#[cfg(test)]
#[test]
fn apm_033_lock_adapter_preserves_unknown_metadata_and_workflow_ownership() {
    let text = include_str!("../../../../tests/fixtures/apm-0.33.0-lock.yaml");
    let mut lock: Value = crate::infra::yaml::from_str(text).unwrap();
    let before = lock.clone();
    let mut names = BTreeSet::new();
    assert!(strip_legacy_cowork_deployments(&mut lock, &mut names));
    assert_eq!(names, BTreeSet::from(["example".to_string()]));
    assert_eq!(
        lock["dependencies"][0]["future_metadata"],
        before["dependencies"][0]["future_metadata"]
    );
    let serialized = crate::infra::yaml::to_string(&lock).unwrap();
    assert_eq!(
        lock["dependencies"][0]["deployed_files"],
        serde_json::json!([
            "copilot-app-db://workflows/apm--example--fixture--review",
            ".agents/skills/example/SKILL.md"
        ])
    );
    assert!(!strip_legacy_cowork_deployments(&mut lock, &mut names));
    assert_eq!(
        crate::infra::yaml::from_str::<Value>(&serialized).unwrap(),
        lock
    );
}

fn record_legacy_cowork_deployment(value: &Value, names: &mut BTreeSet<String>) -> bool {
    if record_legacy_cowork_uri(value, names) {
        return true;
    }
    let owned_uri = value
        .get("value")
        .is_some_and(|uri| record_legacy_cowork_uri(uri, names));
    owned_uri || value.get("target").and_then(Value::as_str) == Some(COWORK_TARGET)
}

fn record_legacy_cowork_uri(value: &Value, names: &mut BTreeSet<String>) -> bool {
    if let Some(path) = value
        .as_str()
        .and_then(|uri| uri.strip_prefix("cowork://skills/"))
        && let Some(name) = path.split(['/', '\\']).next()
        && valid_skill_name(name)
    {
        names.insert(name.to_string());
    }
    is_cowork_uri(value)
}

fn is_cowork_uri(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|value| value.starts_with(COWORK_URI_PREFIX))
}

fn check_legacy_entry_point(target_skill: &Path) -> Result<()> {
    reject_destination_link(target_skill)?;
    let entry_point = target_skill.join("SKILL.md");
    anyhow::ensure!(
        symlink_metadata_optional(&entry_point, "inspect legacy Cowork entry point")?.is_none(),
        "preserving legacy Cowork file {} without a verified fingerprint; move it aside and retry",
        entry_point.display()
    );
    Ok(())
}

fn cowork_skill_paths(ctx: &Context) -> Result<(PathBuf, PathBuf)> {
    let source = ctx.home().join(".agents").join("skills");
    let target = copilot_cowork_skills_path(ctx)?.context(
        "Copilot Cowork skills path is not configured; set \
         APM_COPILOT_COWORK_SKILLS_DIR or `apm config set copilot-cowork-skills-dir`",
    )?;
    Ok((source, target))
}

#[derive(Debug, Deserialize)]
struct ApmLock {
    #[serde(default)]
    dependencies: Vec<LockedDependency>,
}

#[derive(Debug, Deserialize)]
struct LockedDependency {
    #[serde(default)]
    deployed_files: Vec<String>,
    target_subset: Option<Vec<String>>,
}

fn desired_cowork_skill_names(home: &Path) -> Result<BTreeSet<String>> {
    let lock_path = home.join(".apm").join("apm.lock.yaml");
    let lock = std::fs::read_to_string(&lock_path)
        .with_context(|| format!("reading APM lockfile {}", lock_path.display()))?;
    let lock: ApmLock = crate::infra::yaml::from_str(&lock)
        .with_context(|| format!("parsing APM lockfile {}", lock_path.display()))?;
    selected_names_from_lock(lock)
}

fn selected_names_from_lock(lock: ApmLock) -> Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    for dependency in lock.dependencies {
        if dependency
            .target_subset
            .as_ref()
            .is_some_and(|targets| !targets.iter().any(|target| target == COWORK_TARGET))
        {
            continue;
        }
        for deployed_file in dependency.deployed_files {
            let normalized = deployed_file.replace('\\', "/");
            if let Some(path) = normalized.strip_prefix(".agents/skills/")
                && let Some(name) = path.split('/').next()
            {
                anyhow::ensure!(
                    valid_skill_name(name),
                    "invalid APM skill path {deployed_file}"
                );
                names.insert(name.to_owned());
            }
        }
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::domains::ai::apm::targets::ONEDRIVE_COMMERCIAL;
    use crate::domains::ai::apm::test_fixture::make_context_with_home;
    use crate::infra::env::MapEnv;
    use crate::infra::exec::MockExecutor;
    use crate::infra::platform::{Os, Platform};

    fn setup_skill(home: &Path, target_subset: &str) -> (PathBuf, PathBuf, Context) {
        let onedrive = home.join("OneDrive - Test");
        let source_skill = home.join(".agents").join("skills").join("example");
        let target_skill = onedrive
            .join("Documents")
            .join("Cowork")
            .join("skills")
            .join("example");
        std::fs::create_dir_all(&source_skill).expect("create source skill");
        std::fs::create_dir_all(&target_skill).expect("create target skill");
        std::fs::create_dir_all(home.join(".apm")).expect("create APM directory");
        std::fs::write(
            home.join(".apm").join("apm.lock.yaml"),
            format!(
                "dependencies:\n  - deployed_files:\n      - \
                 .agents/skills/example/SKILL.md\n    target_subset: {target_subset}\n"
            ),
        )
        .expect("write lock");
        std::fs::write(source_skill.join("SKILL.md"), "current").expect("write source");
        let ctx =
            make_context_with_home(home, Platform::new(Os::Windows, false), MockExecutor::new())
                .with_env(Arc::new(MapEnv::new().with(ONEDRIVE_COMMERCIAL, &onedrive)));
        (source_skill, target_skill, ctx)
    }

    #[cfg(any(unix, windows))]
    fn create_directory_link(target: &Path, link: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
        #[cfg(windows)]
        {
            use crate::infra::exec::{ProcessExecutor, windows::CmdCommand};

            let result = CmdCommand::new("mklink")
                .arg("/J")
                .arg(std::path::absolute(link).unwrap().to_str().unwrap())
                .arg(std::path::absolute(target).unwrap().to_str().unwrap())
                .run_unchecked(&ProcessExecutor::system())
                .unwrap();
            assert!(result.success, "fixture junction creation: {result:?}");
        }
    }

    #[cfg(any(unix, windows))]
    fn remove_directory_link(link: &Path) {
        if is_dir_like(&link.symlink_metadata().unwrap()) {
            std::fs::remove_dir(link).unwrap();
        } else {
            std::fs::remove_file(link).unwrap();
        }
    }

    #[test]
    #[cfg(any(unix, windows))]
    fn matching_and_copying_reject_root_and_nested_directory_links() {
        for nested in [false, true] {
            let dir = tempfile::tempdir_in(".").unwrap();
            let root = std::fs::canonicalize(dir.path()).unwrap();
            let source = root.join("source");
            let target = root.join("target");
            let unrelated = root.join("unrelated");
            std::fs::create_dir(&source).unwrap();
            std::fs::create_dir(&unrelated).unwrap();
            std::fs::write(unrelated.join("SKILL.md"), "preserve").unwrap();
            let permissions = unrelated.metadata().unwrap().permissions();
            let link = if nested {
                std::fs::create_dir(source.join("nested")).unwrap();
                std::fs::write(source.join("nested/SKILL.md"), "desired").unwrap();
                std::fs::create_dir(&target).unwrap();
                target.join("nested")
            } else {
                std::fs::write(source.join("SKILL.md"), "desired").unwrap();
                target.clone()
            };
            create_directory_link(&unrelated, &link);

            assert!(
                skill_files_match(&source, &target).is_err(),
                "nested={nested}"
            );
            assert!(
                copy_skill_files(&source, &target).is_err(),
                "nested={nested}"
            );
            assert_eq!(
                std::fs::read_to_string(unrelated.join("SKILL.md")).unwrap(),
                "preserve"
            );
            assert_eq!(unrelated.metadata().unwrap().permissions(), permissions);
            assert!(std::fs::read_link(&link).is_ok());
            remove_directory_link(&link);
        }
    }

    #[test]
    #[cfg(any(unix, windows))]
    fn reconciliation_rejects_a_linked_cowork_root_before_reading_ownership() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let (_, target, ctx) = setup_skill(&root, "[agent-skills, copilot-cowork]");
        let cowork_root = target.parent().unwrap();
        let unrelated = root.join("unrelated");
        std::fs::rename(cowork_root, &unrelated).unwrap();
        std::fs::write(unrelated.join(OWNERSHIP_FILE), "[\"other\"]").unwrap();
        create_directory_link(&unrelated, cowork_root);

        assert!(reconcile_cowork_skills(&ctx).is_err());
        assert_eq!(
            std::fs::read_to_string(unrelated.join(OWNERSHIP_FILE)).unwrap(),
            "[\"other\"]"
        );
        assert!(!unrelated.join("example/SKILL.md").exists());
        remove_directory_link(cowork_root);
    }

    #[test]
    fn overlapping_cowork_destination_never_claims_or_prunes_shared_skills() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let home = fixture.path();
        let (source, target, ctx) = setup_skill(home, "[agent-skills, copilot-cowork]");
        let shared = source.parent().unwrap();
        let lock = home.join(".apm/apm.lock.yaml");
        let before = std::fs::read(&lock).unwrap();
        let nested = source.join("missing/new");
        #[cfg(unix)]
        std::os::unix::fs::symlink(shared, home.join("alias")).unwrap();
        for destination in [
            shared.to_path_buf(),
            home.join(".agents"),
            nested.clone(),
            #[cfg(unix)]
            home.join("alias/example/nested"),
        ] {
            let bad = make_context_with_home(home, ctx.platform(), MockExecutor::new()).with_env(
                MapEnv::new()
                    .with("APM_COPILOT_COWORK_SKILLS_DIR", destination)
                    .into_handle(),
            );
            assert!(super::super::targets::ApmTargets::detect(&bad).is_err());
            assert!(reconcile_cowork_skills(&bad).is_err());
            assert!(!shared.join(OWNERSHIP_FILE).exists());
            assert!(!shared.join(FILE_OWNERSHIP_FILE).exists());
            assert!(!nested.exists());
            assert_eq!(std::fs::read(&lock).unwrap(), before);
            assert_eq!(std::fs::read(source.join("SKILL.md")).unwrap(), b"current");
        }
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        std::fs::write(&lock, "dependencies: []\n").unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        assert!(!target.join("SKILL.md").exists());
        assert_eq!(std::fs::read(source.join("SKILL.md")).unwrap(), b"current");
    }

    #[test]
    #[cfg(windows)]
    fn matching_and_copying_reject_a_junction_at_a_file_destination() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let source = dir.path().join("SKILL.md");
        let target = dir.path().join("target");
        let unrelated = dir.path().join("unrelated");
        std::fs::write(&source, "desired").unwrap();
        std::fs::create_dir(&unrelated).unwrap();
        std::fs::write(unrelated.join("preserved"), "unmanaged").unwrap();
        create_directory_link(&unrelated, &target);

        assert!(skill_files_match(&source, &target).is_err());
        assert!(copy_skill_files(&source, &target).is_err());
        assert_eq!(
            std::fs::read_to_string(unrelated.join("preserved")).unwrap(),
            "unmanaged"
        );
        assert!(std::fs::read_link(&target).is_ok());
        remove_directory_link(&target);
    }

    #[test]
    fn reconcile_updates_files_without_replacing_cowork_directory() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let (_, target_skill, ctx) = setup_skill(dir.path(), "[agent-skills, copilot-cowork]");
        std::fs::write(target_skill.join("placeholder.txt"), "preserved")
            .expect("write placeholder");

        reconcile_cowork_skills(&ctx).expect("reconcile skills");

        assert_eq!(
            std::fs::read_to_string(target_skill.join("SKILL.md")).expect("read target"),
            "current"
        );
        assert!(target_skill.join("placeholder.txt").exists());
    }

    #[test]
    fn deleted_source_files_are_pruned_but_unowned_files_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let (source, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
        std::fs::create_dir(source.join("scripts")).unwrap();
        std::fs::write(source.join("scripts/old.py"), "old managed script").unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        std::fs::write(target.join("scripts/personal.py"), "unmanaged").unwrap();
        std::fs::remove_file(source.join("scripts/old.py")).unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        assert!(!target.join("scripts/old.py").exists());
        assert_eq!(
            std::fs::read_to_string(target.join("scripts/personal.py")).unwrap(),
            "unmanaged"
        );
        assert!(!reconcile_cowork_skills(&ctx).unwrap());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn updating_an_owned_script_preserves_its_destination_acl_owner_and_execute_mode() {
        use rustix::fs::{XattrFlags, fgetxattr, fsetxattr};
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        let dir = tempfile::tempdir_in(".").unwrap();
        let (source, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
        let script = source.join("run");
        let installed = target.join("run");
        std::fs::write(&script, "first").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        assert_eq!(installed.metadata().unwrap().mode() & 0o777, 0o755);

        // A named ACL on this synthetic target cannot be retained just by
        // copying Unix mode bits. No account lookup or ownership change.
        let mut acl = 2_u32.to_le_bytes().to_vec();
        for (tag, rights, id) in [
            (1_u16, 7_u16, u32::MAX),
            (2, 4, 12_345),
            (4, 0, u32::MAX),
            (16, 4, u32::MAX),
            (32, 0, u32::MAX),
        ] {
            acl.extend(tag.to_le_bytes());
            acl.extend(rights.to_le_bytes());
            acl.extend(id.to_le_bytes());
        }
        let attr = "system.posix_acl_access";
        fsetxattr(
            std::fs::File::open(&installed).unwrap(),
            attr,
            &acl,
            XattrFlags::empty(),
        )
        .unwrap();
        let before = installed.metadata().unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o750)).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::write(&script, "updated").unwrap();

        assert!(reconcile_cowork_skills(&ctx).unwrap());
        assert_eq!(std::fs::read(&installed).unwrap(), b"updated");
        let after = installed.metadata().unwrap();
        assert_eq!((after.uid(), after.gid()), (before.uid(), before.gid()));
        assert_eq!(after.mode() & 0o777, 0o740);
        let mut retained = vec![0; acl.len()];
        assert_eq!(
            fgetxattr(
                std::fs::File::open(&installed).unwrap(),
                attr,
                retained.as_mut_slice()
            )
            .unwrap(),
            acl.len()
        );
        assert_eq!(retained, acl);
        assert_eq!(target.metadata().unwrap().mode() & 0o777, 0o750);
        assert!(!reconcile_cowork_skills(&ctx).unwrap());
        std::fs::write(dir.path().join(".apm/apm.lock.yaml"), "dependencies: []\n").unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        assert!(!installed.exists());
    }

    #[cfg(windows)]
    #[test]
    fn update_refuses_a_read_only_cowork_target_without_replacing_or_disowning_it() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let (source, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        let installed = target.join("SKILL.md");
        let original_policy = installed.metadata().unwrap().permissions();
        let mut restricted = original_policy.clone();
        restricted.set_readonly(true);
        std::fs::set_permissions(&installed, restricted).unwrap();
        let root = target.parent().unwrap();
        let ledger = std::fs::read(root.join(FILE_OWNERSHIP_FILE)).unwrap();
        std::fs::write(source.join("SKILL.md"), "updated").unwrap();

        let error = reconcile_cowork_skills(&ctx).unwrap_err();
        assert!(
            format!("{error:#}").contains("read-only target"),
            "{error:#}"
        );
        assert_eq!(std::fs::read(&installed).unwrap(), b"current");
        assert!(installed.metadata().unwrap().permissions().readonly());
        assert_eq!(
            std::fs::read(root.join(FILE_OWNERSHIP_FILE)).unwrap(),
            ledger
        );
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 1);
        std::fs::set_permissions(&installed, original_policy).unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        assert_eq!(std::fs::read(&installed).unwrap(), b"updated");
        assert!(!reconcile_cowork_skills(&ctx).unwrap());
    }

    #[test]
    fn interrupted_shared_skill_preserves_the_installed_entry_point_until_retry() {
        let dir = tempfile::tempdir().unwrap();
        let (source, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
        std::fs::write(source.join("guide"), "previous").unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        std::fs::write(target.join("notes"), "my customization").unwrap();
        let names = target.parent().unwrap().join(OWNERSHIP_FILE);
        let files = target.parent().unwrap().join(FILE_OWNERSHIP_FILE);
        let before_names = std::fs::read(&names).unwrap();
        let before_files = std::fs::read(&files).unwrap();
        std::fs::remove_file(source.join("SKILL.md")).unwrap();
        std::fs::remove_file(source.join("guide")).unwrap();

        assert!(reconcile_cowork_skills(&ctx).is_err());
        assert_eq!(std::fs::read(target.join("SKILL.md")).unwrap(), b"current");
        assert_eq!(std::fs::read(target.join("guide")).unwrap(), b"previous");
        assert_eq!(std::fs::read(&names).unwrap(), before_names);
        assert_eq!(std::fs::read(&files).unwrap(), before_files);
        std::fs::write(source.join("SKILL.md"), "restored").unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        assert_eq!(std::fs::read(target.join("SKILL.md")).unwrap(), b"restored");
        assert!(!target.join("guide").exists());
        assert_eq!(
            std::fs::read(target.join("notes")).unwrap(),
            b"my customization"
        );
        assert!(!reconcile_cowork_skills(&ctx).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn disconnected_apm_config_does_not_redirect_skills_to_onedrive() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let (_, fallback, ctx) = setup_skill(home, "[copilot-cowork]");
        let selected = home.join("selected");
        let settings = home.join("settings.json");
        let link = home.join(".apm/config.json");
        std::os::unix::fs::symlink(&settings, &link).unwrap();
        assert!(super::super::targets::ApmTargets::detect(&ctx).is_err());
        assert!(reconcile_cowork_skills(&ctx).is_err());
        assert!(!fallback.join("SKILL.md").exists());
        assert!(!fallback.parent().unwrap().join(OWNERSHIP_FILE).exists());
        let explicit = make_context_with_home(home, ctx.platform(), MockExecutor::new()).with_env(
            MapEnv::new()
                .with("APM_COPILOT_COWORK_SKILLS_DIR", &selected)
                .into_handle(),
        );
        assert_eq!(
            copilot_cowork_skills_path(&explicit).unwrap(),
            Some(selected.clone())
        );

        std::fs::write(
            &settings,
            serde_json::json!({ "copilot_cowork_skills_dir": selected }).to_string(),
        )
        .unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        assert_eq!(
            std::fs::read(selected.join("example/SKILL.md")).unwrap(),
            b"current"
        );
        assert_eq!(std::fs::read_link(&link).unwrap(), settings);
        assert!(!fallback.join("SKILL.md").exists());
        assert!(!reconcile_cowork_skills(&ctx).unwrap());
    }

    #[test]
    fn differing_unowned_and_modified_active_files_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let (source, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
        let skill = target.join("SKILL.md");
        for owned in [false, true] {
            if owned {
                std::fs::remove_file(&skill).unwrap();
                assert!(reconcile_cowork_skills(&ctx).unwrap());
                std::fs::write(source.join("SKILL.md"), "new upstream content").unwrap();
            }
            std::fs::write(&skill, "local content").unwrap();
            let error = reconcile_cowork_skills(&ctx).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("preserving unowned or modified Cowork file")
            );
            assert_eq!(std::fs::read_to_string(&skill).unwrap(), "local content");
            assert_eq!(
                target.parent().unwrap().join(FILE_OWNERSHIP_FILE).exists(),
                owned
            );
        }
    }

    #[test]
    #[cfg(any(unix, windows))]
    fn stale_cleanup_refuses_a_linked_grandparent() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let (source, target, ctx) = setup_skill(&root, "[copilot-cowork]");
        let stale = source.join("nested/scripts/old.py");
        std::fs::create_dir_all(stale.parent().unwrap()).unwrap();
        std::fs::write(&stale, "managed").unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());

        let outside = root.join("outside");
        let linked = target.join("nested");
        std::fs::rename(&linked, &outside).unwrap();
        create_directory_link(&outside, &linked);
        std::fs::remove_file(stale).unwrap();

        let error = reconcile_cowork_skills(&ctx).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("refusing to copy through destination link")
        );
        assert_eq!(
            std::fs::read_to_string(outside.join("scripts/old.py")).unwrap(),
            "managed"
        );
        remove_directory_link(&linked);
    }

    #[test]
    fn modified_stale_files_are_reported_and_retained_until_resolved() {
        for whole_skill in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let (source, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
            std::fs::write(source.join("old.txt"), "managed").unwrap();
            reconcile_cowork_skills(&ctx).unwrap();
            std::fs::write(target.join("old.txt"), "my changes").unwrap();
            if whole_skill {
                std::fs::write(dir.path().join(".apm/apm.lock.yaml"), "dependencies: []\n")
                    .unwrap();
            } else {
                std::fs::remove_file(source.join("old.txt")).unwrap();
            }
            let error = reconcile_cowork_skills(&ctx).unwrap_err();
            assert!(
                error.to_string().contains("modified stale Cowork file"),
                "{error:#}"
            );
            assert_eq!(
                std::fs::read_to_string(target.join("old.txt")).unwrap(),
                "my changes"
            );
            std::fs::remove_file(target.join("old.txt")).unwrap();
            assert!(reconcile_cowork_skills(&ctx).unwrap());
            assert!(!reconcile_cowork_skills(&ctx).unwrap());
        }
    }

    #[test]
    fn unsafe_file_ownership_is_rejected_before_deployment() {
        let dir = tempfile::tempdir().unwrap();
        let (_, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
        std::fs::write(target.join("SKILL.md"), "preserve").unwrap();
        std::fs::write(
            target.parent().unwrap().join(FILE_OWNERSHIP_FILE),
            r#"{"example":{"../outside":{"hash":"x","link":false}}}"#,
        )
        .unwrap();
        assert!(reconcile_cowork_skills(&ctx).is_err());
        assert_eq!(
            std::fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "preserve"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn interrupted_copy_keeps_the_existing_file_and_cleans_staging() {
        const CHILD_ROOT: &str = "DOTFILES_TEST_COWORK_COPY_ROOT";
        if let Some(root) = std::env::var_os(CHILD_ROOT) {
            let root = PathBuf::from(root);
            assert!(copy_skill_files(&root.join("source"), &root.join("target")).is_err());
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("source"), "x".repeat(65536)).unwrap();
        std::fs::write(dir.path().join("target"), "previous deployment").unwrap();
        let output = std::process::Command::new("sh")
            .args(["-c", "trap '' XFSZ; ulimit -f 1; exec \"$@\"", "limited-cowork-copy"])
            .arg(std::env::current_exe().unwrap())
            .args(["--exact", "domains::ai::apm::cowork::tests::interrupted_copy_keeps_the_existing_file_and_cleans_staging"])
            .env(CHILD_ROOT, dir.path()).env("LLVM_PROFILE_FILE", "/dev/null")
            .output().unwrap();
        assert!(output.status.success(), "limited copy failed: {output:?}");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("target")).unwrap(),
            "previous deployment"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[cfg(windows)]
    #[test]
    fn ownership_key_distinguishes_missing_paths_from_inspection_errors() {
        let fixture = tempfile::tempdir().unwrap();
        assert_eq!(ownership_key(fixture.path(), "missing").unwrap(), "missing");
        assert!(ownership_key(fixture.path(), "invalid\0name").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn skill_spelling_and_content_change_migrates_ownership_through_deselection() {
        for (before, after) in [("example", "Example"), ("İExample", "İEXAMPLE")] {
            let dir = tempfile::tempdir_in(".").unwrap();
            let (source, target, ctx) = setup_skill(dir.path(), "[agent-skills, copilot-cowork]");
            if before != "example" {
                std::fs::rename(&source, source.with_file_name(before)).unwrap();
            }
            let target = target.with_file_name(before);
            let lock_path = dir.path().join(".apm").join("apm.lock.yaml");
            let lock = std::fs::read_to_string(&lock_path).unwrap();
            std::fs::write(
                &lock_path,
                lock.replace("skills/example/", &format!("skills/{before}/")),
            )
            .unwrap();
            assert!(reconcile_cowork_skills(&ctx).unwrap());
            std::fs::write(target.join("cowork-owned.txt"), "preserve").unwrap();
            std::fs::write(source.with_file_name(before).join("SKILL.md"), "updated").unwrap();
            std::fs::write(
                &lock_path,
                lock.replace("skills/example/", &format!("skills/{after}/")),
            )
            .unwrap();

            assert!(reconcile_cowork_skills(&ctx).unwrap());

            assert_eq!(
                std::fs::read_to_string(target.join("SKILL.md")).unwrap(),
                "updated",
                "the previous casing aliases the desired skill, not an orphan"
            );
            assert_eq!(
                std::fs::read_to_string(target.join("cowork-owned.txt")).unwrap(),
                "preserve"
            );
            let root = target.parent().unwrap();
            assert_eq!(
                read_file_inventory(root)
                    .unwrap()
                    .into_keys()
                    .collect::<Vec<_>>(),
                [after]
            );
            assert_eq!(
                read_owned_skills(root).unwrap(),
                BTreeSet::from([after.to_owned()])
            );
            assert!(!reconcile_cowork_skills(&ctx).unwrap());
            std::fs::write(lock_path, "dependencies: []\n").unwrap();
            assert!(reconcile_cowork_skills(&ctx).unwrap());
            assert!(!target.join("SKILL.md").exists());
            assert!(read_file_inventory(root).unwrap().is_empty());
            assert_eq!(
                std::fs::read(target.join("cowork-owned.txt")).unwrap(),
                b"preserve"
            );
            assert!(!reconcile_cowork_skills(&ctx).unwrap());
        }
    }

    #[cfg(windows)]
    #[test]
    fn asset_spelling_and_content_change_preserves_local_edits_until_retry() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let (source, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
        std::fs::create_dir(source.join("Docs")).unwrap();
        std::fs::write(source.join("Docs/Guide"), "prior").unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        let inventory = target.parent().unwrap().join(FILE_OWNERSHIP_FILE);
        let before = std::fs::read(&inventory).unwrap();
        // Recreate rather than rely on a case-only rename changing the stored name.
        std::fs::remove_dir_all(source.join("Docs")).unwrap();
        std::fs::create_dir(source.join("DOCS")).unwrap();
        std::fs::write(source.join("DOCS/GUIDE"), "updated").unwrap();
        let guide = target.join("Docs/Guide");
        std::fs::write(&guide, "edited").unwrap();
        let error = reconcile_cowork_skills(&ctx).unwrap_err();
        assert!(
            error.to_string().contains("modified Cowork file"),
            "{error:#}"
        );
        assert_eq!(std::fs::read(&guide).unwrap(), b"edited");
        assert_eq!(std::fs::read(&inventory).unwrap(), before);

        std::fs::write(&guide, "prior").unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        assert_eq!(std::fs::read(&guide).unwrap(), b"updated");
        assert!(!reconcile_cowork_skills(&ctx).unwrap());
        std::fs::write(dir.path().join(".apm/apm.lock.yaml"), "dependencies: []\n").unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        assert!(!guide.exists());
        assert!(!reconcile_cowork_skills(&ctx).unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn competing_selected_spellings_do_not_claim_a_skill() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let (_, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
        std::fs::remove_dir(&target).unwrap();
        std::fs::write(dir.path().join(".apm/apm.lock.yaml"), "dependencies:\n- deployed_files: [.agents/skills/example/SKILL.md, .agents/skills/EXAMPLE/SKILL.md]\n").unwrap();
        let error = reconcile_cowork_skills(&ctx).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("ambiguous selected Cowork skill"),
            "{error:#}"
        );
        assert!(!target.exists());
        let root = target.parent().unwrap();
        assert!(!root.join(OWNERSHIP_FILE).exists());
        assert!(!root.join(FILE_OWNERSHIP_FILE).exists());
    }

    #[test]
    fn conflicting_alias_fingerprints_block_updates_and_agreeing_aliases_retire() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let (source, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
        std::fs::create_dir(source.join("docs")).unwrap();
        std::fs::write(source.join("docs/guide"), "old").unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        let root = target.parent().unwrap();
        let mut inventory = read_file_inventory(root).unwrap();
        // Older runs could leave a second skill key only in the file ledger.
        #[cfg(windows)]
        let alias = "EXAMPLE";
        #[cfg(not(windows))]
        let alias = "example";
        let aliases = inventory.entry(alias.to_owned()).or_default();
        let guide = target.join("docs/guide");
        let old = file_fingerprint(&guide).unwrap().unwrap();
        aliases.insert(
            "docs/./guide".to_owned(),
            file_fingerprint(&target.join("SKILL.md")).unwrap().unwrap(),
        );
        std::fs::write(target.join("retired"), "old asset").unwrap();
        aliases.insert(
            "retired".to_owned(),
            file_fingerprint(&target.join("retired")).unwrap().unwrap(),
        );
        write_file_inventory(root, &inventory).unwrap();
        std::fs::write(source.join("docs/guide"), "new").unwrap();

        let error = reconcile_cowork_skills(&ctx).unwrap_err();
        assert!(error.to_string().contains("ambiguous Cowork"), "{error:#}");
        assert_eq!(std::fs::read(&guide).unwrap(), b"old");
        assert_eq!(read_file_inventory(root).unwrap(), inventory);
        assert!(target.join("retired").exists());

        inventory
            .get_mut(alias)
            .unwrap()
            .insert("docs/./guide".to_owned(), old);
        write_file_inventory(root, &inventory).unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        assert_eq!(std::fs::read(&guide).unwrap(), b"new");
        assert!(!target.join("retired").exists());
        assert_eq!(
            read_file_inventory(root).unwrap(),
            SkillInventory::from([(
                "example".to_owned(),
                selected_skill_inventory(&source).unwrap()
            )])
        );
        assert!(!reconcile_cowork_skills(&ctx).unwrap());
    }

    #[test]
    fn current_and_legacy_skill_ownership_preserve_configured_names() {
        let dir = tempfile::tempdir_in(".").unwrap();
        std::fs::write(
            dir.path().join(OWNERSHIP_FILE),
            "[\"Example\", \"example\"]",
        )
        .unwrap();
        let expected = BTreeSet::from(["Example".to_string(), "example".to_string()]);
        assert_eq!(read_owned_skills(dir.path()).unwrap(), expected);

        let content = "dependencies:\n- deployed_files: \
            [.agents/skills/Example/SKILL.md, .agents/skills/example/SKILL.md]\n";
        std::fs::create_dir(dir.path().join(".apm")).unwrap();
        std::fs::write(dir.path().join(".apm").join("apm.lock.yaml"), content).unwrap();
        assert_eq!(desired_cowork_skill_names(dir.path()).unwrap(), expected);

        let mut lock: Value =
            crate::infra::yaml::from_str(&content.replace(".agents/skills/", "cowork://skills/"))
                .unwrap();
        let mut legacy = BTreeSet::new();
        assert!(strip_legacy_cowork_deployments(&mut lock, &mut legacy));
        assert_eq!(legacy, expected);
    }

    #[test]
    #[cfg(any(unix, windows))]
    fn file_to_link_requires_move_aside_before_any_reconciliation_writes() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let (first, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
        let last = first.with_file_name("z-last");
        let guide = last.join("docs/guide");
        std::fs::create_dir_all(guide.parent().unwrap()).unwrap();
        std::fs::write(last.join("SKILL.md"), "new reference").unwrap();
        std::fs::write(&guide, "installed guide").unwrap();
        std::fs::write(first.join("stale"), "keep until retry").unwrap();
        let lock = dir.path().join(".apm/apm.lock.yaml");
        let selection = "dependencies:\n- deployed_files:\n  - .agents/skills/example/SKILL.md\n  - .agents/skills/z-last/SKILL.md\n";
        std::fs::write(&lock, selection).unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        let root = target.parent().unwrap();
        let installed = root.join("z-last").join("docs").join("guide");
        let names = std::fs::read(root.join(OWNERSHIP_FILE)).unwrap();
        let files = std::fs::read(root.join(FILE_OWNERSHIP_FILE)).unwrap();
        let legacy = format!("{selection}  - cowork://skills/retired/SKILL.md\n");
        std::fs::write(&lock, &legacy).unwrap();
        std::fs::remove_file(first.join("stale")).unwrap();
        std::fs::write(first.join("fresh"), "pending").unwrap();
        std::fs::remove_file(&guide).unwrap();
        create_native_symlink(&Path::new("..").join("SKILL.md"), &guide, false).unwrap();

        let error = reconcile_cowork_skills(&ctx).unwrap_err().to_string();
        assert!(error.contains(&installed.display().to_string()), "{error}");
        assert!(error.contains("move it aside and retry"), "{error}");
        assert!(installed.symlink_metadata().unwrap().is_file());
        assert_eq!(std::fs::read(&installed).unwrap(), b"installed guide");
        assert_eq!(
            std::fs::read(target.join("stale")).unwrap(),
            b"keep until retry"
        );
        assert!(!target.join("fresh").exists());
        assert_eq!(std::fs::read(root.join(OWNERSHIP_FILE)).unwrap(), names);
        assert_eq!(
            std::fs::read(root.join(FILE_OWNERSHIP_FILE)).unwrap(),
            files
        );
        assert_eq!(std::fs::read_to_string(&lock).unwrap(), legacy);

        let saved = dir.path().join("saved-guide");
        std::fs::rename(&installed, &saved).unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        assert!(installed.symlink_metadata().unwrap().is_symlink());
        assert_eq!(std::fs::read(&installed).unwrap(), b"new reference");
        assert_eq!(std::fs::read(&saved).unwrap(), b"installed guide");
        assert!(!target.join("stale").exists());
        assert_eq!(std::fs::read(target.join("fresh")).unwrap(), b"pending");
        assert!(!reconcile_cowork_skills(&ctx).unwrap());
    }

    #[test]
    #[cfg(any(unix, windows))]
    fn relocated_skill_entry_and_asset_links_read_from_the_cowork_copy() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let (source, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
        std::fs::create_dir(source.join("docs")).unwrap();
        std::fs::create_dir(source.join("assets")).unwrap();
        std::fs::rename(source.join("SKILL.md"), source.join("docs/entry.md")).unwrap();
        std::fs::write(source.join("assets/guide"), "copied guide").unwrap();
        create_native_symlink(
            &Path::new("docs").join("entry.md"),
            &source.join("SKILL.md"),
            false,
        )
        .unwrap();
        create_native_symlink(Path::new("assets"), &source.join("library"), true).unwrap();
        let reference = Path::new("..").join("library").join("guide");
        create_native_symlink(&reference, &source.join("docs/reference"), false).unwrap();

        assert!(reconcile_cowork_skills(&ctx).unwrap());
        assert!(!reconcile_cowork_skills(&ctx).unwrap());
        std::fs::rename(&source, dir.path().join("source-offline")).unwrap();
        assert_eq!(std::fs::read(target.join("SKILL.md")).unwrap(), b"current");
        assert_eq!(
            std::fs::read(target.join("docs/reference")).unwrap(),
            b"copied guide"
        );
        assert_eq!(
            std::fs::read_link(target.join("docs/reference")).unwrap(),
            reference
        );
    }

    #[test]
    #[cfg(any(unix, windows))]
    fn nonportable_skill_links_reject_the_batch_before_any_cowork_writes() {
        for (entry, payload) in [
            ("SKILL.md", "../common/SKILL.md"),
            ("asset", "../common/SKILL.md"),
            ("asset", "../z-last/SKILL.md"),
            ("asset", "absolute"),
            ("asset", "missing"),
            ("asset", "asset"),
        ] {
            let dir = tempfile::tempdir_in(".").unwrap();
            let (first, target, ctx) = setup_skill(dir.path(), "[copilot-cowork]");
            let last = first.with_file_name("z-last");
            let common = first.with_file_name("common");
            std::fs::create_dir(&last).unwrap();
            std::fs::create_dir(&common).unwrap();
            std::fs::write(common.join("SKILL.md"), "external").unwrap();
            std::fs::write(last.join("SKILL.md"), "installed").unwrap();
            std::fs::write(first.join("old"), "retain until safe").unwrap();
            let lock = dir.path().join(".apm/apm.lock.yaml");
            let selection = "dependencies:\n- deployed_files:\n  - .agents/skills/example/SKILL.md\n  - .agents/skills/z-last/SKILL.md\n";
            std::fs::write(&lock, selection).unwrap();
            assert!(reconcile_cowork_skills(&ctx).unwrap());
            let root = target.parent().unwrap();
            let names = std::fs::read(root.join(OWNERSHIP_FILE)).unwrap();
            let files = std::fs::read(root.join(FILE_OWNERSHIP_FILE)).unwrap();
            let legacy = format!("{selection}  - cowork://skills/old/SKILL.md\n");
            std::fs::write(&lock, &legacy).unwrap();
            std::fs::write(first.join("SKILL.md"), "pending update").unwrap();
            std::fs::remove_file(first.join("old")).unwrap();
            if entry == "SKILL.md" {
                std::fs::remove_file(last.join(entry)).unwrap();
            }
            let destination = if payload == "absolute" {
                std::fs::canonicalize(last.join("SKILL.md")).unwrap()
            } else {
                payload.split('/').collect()
            };
            create_native_symlink(&destination, &last.join(entry), false).unwrap();
            if payload == "../common/SKILL.md" {
                assert_eq!(std::fs::read(last.join(entry)).unwrap(), b"external");
            }

            let error = reconcile_cowork_skills(&ctx).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("cannot relocate Cowork skill link"),
                "{entry} -> {payload}: {error:#}"
            );
            assert_eq!(std::fs::read(target.join("SKILL.md")).unwrap(), b"current");
            assert_eq!(
                std::fs::read(target.join("old")).unwrap(),
                b"retain until safe"
            );
            assert_eq!(
                std::fs::read(root.join("z-last/SKILL.md")).unwrap(),
                b"installed"
            );
            assert!(
                root.join("z-last/asset")
                    .symlink_metadata()
                    .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
            );
            assert_eq!(std::fs::read(root.join(OWNERSHIP_FILE)).unwrap(), names);
            assert_eq!(
                std::fs::read(root.join(FILE_OWNERSHIP_FILE)).unwrap(),
                files
            );
            assert_eq!(std::fs::read_to_string(&lock).unwrap(), legacy);
        }
    }

    #[test]
    #[cfg(unix)]
    fn reconcile_updates_skill_files_with_existing_symlinks() {
        let dir = tempfile::tempdir_in(".").expect("create fixture");
        let (source, target, ctx) = setup_skill(dir.path(), "[agent-skills, copilot-cowork]");
        std::os::unix::fs::symlink("SKILL.md", source.join("reference")).unwrap();
        std::fs::write(target.join("cowork-owned.txt"), "preserve").unwrap();
        assert!(reconcile_cowork_skills(&ctx).expect("first deployment"));

        std::fs::write(source.join("SKILL.md"), "updated").unwrap();
        assert!(reconcile_cowork_skills(&ctx).expect("update with retained symlink"));
        assert_eq!(
            std::fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "updated"
        );
        assert_eq!(
            std::fs::read_link(target.join("reference")).unwrap(),
            Path::new("SKILL.md")
        );
        assert_eq!(
            std::fs::read_to_string(target.join("cowork-owned.txt")).unwrap(),
            "preserve"
        );
        assert!(!reconcile_cowork_skills(&ctx).expect("repeat deployment"));
    }

    #[test]
    #[cfg(unix)]
    fn reconcile_retargets_owned_links_and_preserves_modified_destination_links() {
        let dir = tempfile::tempdir_in(".").expect("create fixture");
        let (source, target, ctx) = setup_skill(dir.path(), "[agent-skills, copilot-cowork]");
        std::os::unix::fs::symlink("SKILL.md", source.join("reference")).unwrap();
        assert!(reconcile_cowork_skills(&ctx).expect("first deployment"));
        let external = std::fs::canonicalize(dir.path())
            .unwrap()
            .join("external.md");
        std::fs::write(&external, "unmanaged").unwrap();
        std::fs::remove_file(target.join("SKILL.md")).unwrap();
        std::os::unix::fs::symlink(&external, target.join("SKILL.md")).unwrap();
        std::fs::remove_file(source.join("reference")).unwrap();
        std::os::unix::fs::symlink("new.md", source.join("reference")).unwrap();
        std::fs::write(source.join("new.md"), "new reference").unwrap();

        let error = reconcile_cowork_skills(&ctx).expect_err("keep the modified link");
        assert!(error.to_string().contains("modified Cowork file"));
        assert_eq!(
            std::fs::read_link(target.join("SKILL.md")).unwrap(),
            external
        );
        std::fs::remove_file(target.join("SKILL.md")).unwrap();
        assert!(
            reconcile_cowork_skills(&ctx).expect("restore missing file and retarget owned link")
        );
        assert_eq!(std::fs::read_to_string(&external).unwrap(), "unmanaged");
        assert!(
            !target
                .join("SKILL.md")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read_link(target.join("reference")).unwrap(),
            Path::new("new.md")
        );
        assert!(!reconcile_cowork_skills(&ctx).expect("repeat repair"));
    }

    #[test]
    fn reconcile_never_replaces_a_cowork_directory_with_a_file() {
        let dir = tempfile::tempdir_in(".").expect("create fixture");
        let (_, target, ctx) = setup_skill(dir.path(), "[agent-skills, copilot-cowork]");
        let directory = target.join("SKILL.md");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("preserved"), "unmanaged").unwrap();

        let error = reconcile_cowork_skills(&ctx).expect_err("directory must be preserved");
        assert!(format!("{error:#}").contains("cannot replace Cowork-owned directory"));
        assert_eq!(
            std::fs::read_to_string(directory.join("preserved")).unwrap(),
            "unmanaged"
        );
    }

    #[test]
    fn reconcile_removes_entry_point_when_package_excludes_cowork() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let (_, target_skill, ctx) = setup_skill(dir.path(), "[agent-skills, copilot-cowork]");
        reconcile_cowork_skills(&ctx).expect("establish ownership");
        std::fs::write(target_skill.join("placeholder.txt"), "preserved")
            .expect("write placeholder");
        std::fs::write(
            dir.path().join(".apm").join("apm.lock.yaml"),
            "dependencies:\n  - deployed_files:\n      - .agents/skills/example/SKILL.md\n\
             \x20   target_subset: [agent-skills]\n",
        )
        .expect("exclude Cowork");

        assert!(reconcile_cowork_skills(&ctx).expect("reconcile skills"));

        assert!(!target_skill.join("SKILL.md").exists());
        assert!(target_skill.join("placeholder.txt").exists());
        assert!(!reconcile_cowork_skills(&ctx).expect("repeat reconciliation"));
    }

    #[test]
    fn reconcile_preserves_unmanaged_skills_including_excluded_shared_names() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let (_, target_skill, ctx) = setup_skill(dir.path(), "[agent-skills]");
        std::fs::write(target_skill.join("SKILL.md"), "independent example")
            .expect("write independent skill");
        let personal = target_skill.parent().unwrap().join("personal");
        std::fs::create_dir_all(&personal).expect("create personal skill");
        std::fs::write(personal.join("SKILL.md"), "independent personal")
            .expect("write personal skill");

        assert!(!reconcile_cowork_skills(&ctx).expect("reconcile skills"));
        assert_eq!(
            std::fs::read_to_string(target_skill.join("SKILL.md")).unwrap(),
            "independent example"
        );
        assert_eq!(
            std::fs::read_to_string(personal.join("SKILL.md")).unwrap(),
            "independent personal"
        );
        assert!(!target_skill.parent().unwrap().join(OWNERSHIP_FILE).exists());
    }

    #[test]
    fn reconcile_removes_owned_skills_after_shared_source_and_lock_entry_disappear() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let (source_skill, target_skill, ctx) =
            setup_skill(dir.path(), "[agent-skills, copilot-cowork]");
        assert!(reconcile_cowork_skills(&ctx).expect("install skill"));
        assert!(!reconcile_cowork_skills(&ctx).expect("repeat install"));
        std::fs::remove_dir_all(source_skill.parent().unwrap()).expect("remove shared skills");
        std::fs::write(
            dir.path().join(".apm").join("apm.lock.yaml"),
            "dependencies: []\n",
        )
        .expect("remove dependency");

        assert!(reconcile_cowork_skills(&ctx).expect("remove owned skill"));
        assert!(!target_skill.join("SKILL.md").exists());
        assert!(target_skill.is_dir());
        assert!(!reconcile_cowork_skills(&ctx).expect("repeat removal"));
    }

    #[test]
    fn reconcile_retains_partial_progress_and_stale_ownership_until_retry_converges() {
        let dir = tempfile::tempdir_in(".").expect("create fixture");
        let (source_skill, target_skill, ctx) =
            setup_skill(dir.path(), "[agent-skills, copilot-cowork]");
        let target = target_skill.parent().unwrap();
        let stale_skill = target.join("old");
        std::fs::create_dir_all(&stale_skill).unwrap();
        std::fs::write(stale_skill.join("SKILL.md"), "stale managed entry").unwrap();
        std::fs::write(stale_skill.join("notes.txt"), "Cowork-owned notes").unwrap();
        write_owned_skills(target, &BTreeSet::from(["old".to_string()])).unwrap();
        let old_files = FileInventory::from([(
            "SKILL.md".to_owned(),
            file_fingerprint(&stale_skill.join("SKILL.md"))
                .unwrap()
                .unwrap(),
        )]);
        write_atomic(
            &target.join(FILE_OWNERSHIP_FILE),
            serde_json::to_vec(&SkillInventory::from([("old".to_owned(), old_files)])).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".apm").join("apm.lock.yaml"),
            "dependencies:\n  - deployed_files:\n      - .agents/skills/example/SKILL.md\n\
             \x20     - .agents/skills/z-missing/SKILL.md\n",
        )
        .expect("write missing source dependency");

        let error = reconcile_cowork_skills(&ctx).expect_err("missing source must fail");
        assert!(error.to_string().contains("z-missing"));
        assert_eq!(
            read_owned_skills(target).unwrap(),
            BTreeSet::from(["example".to_string(), "old".to_string()])
        );
        let completed = std::fs::read(target_skill.join("SKILL.md")).unwrap();
        assert_eq!(
            completed,
            std::fs::read(source_skill.join("SKILL.md")).unwrap()
        );
        assert!(stale_skill.join("SKILL.md").is_file());
        assert!(!target.join("z-missing").exists());
        let ownership = std::fs::read(target.join(OWNERSHIP_FILE)).unwrap();

        assert!(
            reconcile_cowork_skills(&ctx)
                .unwrap_err()
                .to_string()
                .contains("z-missing")
        );
        assert_eq!(
            std::fs::read(target.join(OWNERSHIP_FILE)).unwrap(),
            ownership
        );
        assert_eq!(
            std::fs::read(target_skill.join("SKILL.md")).unwrap(),
            completed
        );

        let recovered_source = source_skill.parent().unwrap().join("z-missing");
        std::fs::create_dir_all(&recovered_source).unwrap();
        std::fs::write(recovered_source.join("SKILL.md"), "recovered source").unwrap();
        assert!(reconcile_cowork_skills(&ctx).expect("retry after repairing source"));
        assert_eq!(
            std::fs::read_to_string(target.join("z-missing/SKILL.md")).unwrap(),
            "recovered source"
        );
        assert_eq!(
            read_owned_skills(target).unwrap(),
            BTreeSet::from(["example".to_string(), "z-missing".to_string()])
        );
        assert!(!stale_skill.join("SKILL.md").exists());
        assert_eq!(
            std::fs::read_to_string(stale_skill.join("notes.txt")).unwrap(),
            "Cowork-owned notes"
        );
        assert!(!reconcile_cowork_skills(&ctx).expect("repeat converged reconciliation"));
    }

    #[test]
    fn reconcile_rejects_corrupt_or_unsafe_ownership_without_mutating_skills() {
        for inventory in ["not json", "[\"..\"]", "[\"../personal\"]"] {
            let dir = tempfile::tempdir().expect("create temp dir");
            let (_, target_skill, ctx) = setup_skill(dir.path(), "[agent-skills]");
            std::fs::write(target_skill.join("SKILL.md"), "preserve").unwrap();
            std::fs::write(
                target_skill.parent().unwrap().join(OWNERSHIP_FILE),
                inventory,
            )
            .unwrap();

            assert!(reconcile_cowork_skills(&ctx).is_err());
            assert_eq!(
                std::fs::read_to_string(target_skill.join("SKILL.md")).unwrap(),
                "preserve"
            );
        }
    }

    #[test]
    fn legacy_ownership_comes_only_from_explicit_cowork_deployment_records() {
        for record in [
            "dependencies:\n- deployed_files: [cowork://skills/example/SKILL.md]\n",
            "dependencies:\n- deployed_file_hashes:\n    cowork://skills/example/SKILL.md: old\n",
            "deployments:\n- target: copilot-cowork\n  value: cowork://skills/example/SKILL.md\n",
            "deployments:\n  owner:\n  - target: copilot-cowork\n    value: cowork://skills/example/SKILL.md\n",
        ] {
            let mut lock: Value = crate::infra::yaml::from_str(record).unwrap();
            let mut names = BTreeSet::new();
            assert!(strip_legacy_cowork_deployments(&mut lock, &mut names));
            assert_eq!(names, BTreeSet::from(["example".to_string()]));
        }
        let mut lock: Value = crate::infra::yaml::from_str(
            "dependencies:\n- deployed_files: [.agents/skills/personal/SKILL.md]\n\
             deployments:\n- value: cowork://skills/../SKILL.md\n",
        )
        .unwrap();
        let mut names = BTreeSet::new();
        assert!(strip_legacy_cowork_deployments(&mut lock, &mut names));
        assert!(names.is_empty());
    }

    #[test]
    fn legacy_hash_cleanup_preserves_other_targets_and_is_idempotent() {
        let retained = "dependencies:\n- deployed_file_hashes:\n    \
            .agents/skills/personal/SKILL.md: shared\n    \
            42: [unrelated, values]\n    \
            copilot-app-db://workflows/example: workflow\n";
        let mut lock: Value = crate::infra::yaml::from_str(&format!(
            "{retained}    cowork://skills/example/SKILL.md: old\n    \
             cowork://skills/../SKILL.md: unsafe\n    \
             cowork://other/record: legacy\n"
        ))
        .unwrap();
        let mut names = BTreeSet::new();

        assert!(strip_legacy_cowork_deployments(&mut lock, &mut names));
        assert_eq!(
            lock,
            crate::infra::yaml::from_str::<Value>(retained).unwrap()
        );
        assert_eq!(names, BTreeSet::from(["example".to_string()]));
        assert!(!strip_legacy_cowork_deployments(&mut lock, &mut names));
    }

    #[test]
    fn legacy_deployment_classification_does_not_depend_on_a_cowork_uri() {
        for (record, expected) in [
            ("target: copilot-cowork", true),
            (
                "target: copilot\nvalue: .github/skills/example/SKILL.md",
                false,
            ),
            ("value: .agents/skills/example/SKILL.md", false),
            ("{}", false),
        ] {
            let value = crate::infra::yaml::from_str::<Value>(record).unwrap();
            let mut names = BTreeSet::new();
            assert_eq!(
                record_legacy_cowork_deployment(&value, &mut names),
                expected,
                "{record}"
            );
            assert!(
                names.is_empty(),
                "only a Cowork URI establishes named ownership"
            );
        }
    }

    #[test]
    fn removes_legacy_cowork_lock_entries() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let (_, target_skill, ctx) = setup_skill(dir.path(), "[agent-skills]");
        std::fs::write(target_skill.join("SKILL.md"), "legacy deployment").unwrap();
        std::fs::write(
            dir.path().join(".apm").join("apm.lock.yaml"),
            "dependencies:\n\
             - deployed_files:\n\
             \x20 - .agents/skills/example/SKILL.md\n\
             \x20 - cowork://skills/example/SKILL.md\n\
             \x20 deployed_file_hashes:\n\
             \x20   cowork://skills/example/SKILL.md: stale\n\
             deployments:\n\
             - target: copilot-cowork\n\
             \x20 value: cowork://skills/example/SKILL.md\n",
        )
        .expect("write lock");

        assert!(remove_legacy_cowork_lock_deployments(&ctx).expect("remove legacy records"));
        let lock = std::fs::read_to_string(dir.path().join(".apm").join("apm.lock.yaml"))
            .expect("read lock");
        assert!(!lock.contains("cowork://"));
        assert!(lock.contains(".agents/skills/example/SKILL.md"));
        assert_eq!(
            read_owned_skills(target_skill.parent().unwrap()).unwrap(),
            BTreeSet::from(["example".to_string()])
        );
        assert!(!remove_legacy_cowork_lock_deployments(&ctx).expect("repeat migration"));

        std::fs::write(
            dir.path().join(".apm").join("apm.lock.yaml"),
            "dependencies: []\n",
        )
        .expect("simulate native removal of dependency");
        let error = reconcile_cowork_skills(&ctx).unwrap_err();
        assert!(error.to_string().contains("without a verified fingerprint"));
        assert_eq!(
            std::fs::read_to_string(target_skill.join("SKILL.md")).unwrap(),
            "legacy deployment"
        );
        std::fs::remove_file(target_skill.join("SKILL.md")).unwrap();
        assert!(reconcile_cowork_skills(&ctx).expect("retry after moving the local file"));
    }

    #[test]
    fn legacy_migration_does_not_discard_lock_evidence_if_inventory_cannot_be_written() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let (_, target_skill, ctx) = setup_skill(dir.path(), "[agent-skills]");
        let lock = "dependencies:\n  - deployed_files:\n      - cowork://skills/example/SKILL.md\n";
        let lock_path = dir.path().join(".apm").join("apm.lock.yaml");
        std::fs::write(&lock_path, lock).unwrap();
        std::fs::create_dir(target_skill.parent().unwrap().join(OWNERSHIP_FILE)).unwrap();

        assert!(remove_legacy_cowork_lock_deployments(&ctx).is_err());
        assert_eq!(std::fs::read_to_string(lock_path).unwrap(), lock);
    }

    #[test]
    fn legacy_fingerprints_require_valid_agreeing_hashes() {
        let first = "ab".repeat(32);
        let other = "cd".repeat(32);
        let signed = format!("+b{}", "ab".repeat(31));
        for (second, verified) in [
            (first.as_str(), true),
            (other.as_str(), false),
            ("ab", false),
            (signed.as_str(), false),
        ] {
            let lock: Value = crate::infra::yaml::from_str(&format!(
                "dependencies:\n- deployed_file_hashes:\n    cowork://skills/example/SKILL.md: sha256:{first}\n- deployed_file_hashes:\n    cowork://skills/example/SKILL.md: sha256:{second}\n"
            )).unwrap();
            let fingerprints = legacy_entry_point_fingerprints(&lock);
            assert_eq!(
                fingerprints.get("example").map(Option::is_some),
                Some(verified),
                "{second}"
            );
        }
        assert_eq!(decode_sha256_hex(&first), Some([0xab; 32]));
        assert_eq!(decode_sha256_hex("ab"), None);
        assert_eq!(decode_sha256_hex(&signed), None);
    }

    #[test]
    fn legacy_cleanup_requires_a_matching_deployment_hash() {
        use sha2::{Digest as _, Sha256};

        for modified in [false, true] {
            let dir = tempfile::tempdir().expect("create temp dir");
            let (_, target_skill, ctx) = setup_skill(dir.path(), "[agent-skills]");
            let lock_path = dir.path().join(".apm").join("apm.lock.yaml");
            let hash = Sha256::digest(b"deployed")
                .iter()
                .fold(String::new(), |mut hex, byte| {
                    use std::fmt::Write as _;
                    write!(hex, "{byte:02x}").unwrap();
                    hex
                });
            std::fs::write(
                &lock_path,
                format!("dependencies:\n  - deployed_file_hashes:\n      cowork://skills/example/SKILL.md: sha256:{hash}\n"),
            ).unwrap();
            let entry = target_skill.join("SKILL.md");
            std::fs::write(&entry, if modified { "edited" } else { "deployed" }).unwrap();

            let result = reconcile_cowork_skills(&ctx);
            if modified {
                assert!(result.unwrap_err().to_string().contains("modified stale"));
                assert_eq!(std::fs::read_to_string(entry).unwrap(), "edited");
            } else {
                assert!(result.unwrap());
                assert!(!entry.exists());
                assert!(!reconcile_cowork_skills(&ctx).unwrap());
            }
            assert!(
                !std::fs::read_to_string(lock_path)
                    .unwrap()
                    .contains("cowork://")
            );
        }
    }

    #[test]
    fn owned_file_and_directory_transitions_preserve_local_content_and_retry() {
        let dir = tempfile::tempdir().unwrap();
        let (source_skill, target_skill, ctx) =
            setup_skill(dir.path(), "[agent-skills, copilot-cowork]");
        let source = source_skill.join("asset");
        let target = target_skill.join("asset");
        std::fs::write(&source, "file").unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());

        std::fs::remove_file(&source).unwrap();
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("nested"), "nested").unwrap();
        std::fs::write(&target, "edited").unwrap();
        assert!(reconcile_cowork_skills(&ctx).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "edited");
        std::fs::write(&target, "file").unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        assert_eq!(
            std::fs::read_to_string(target.join("nested")).unwrap(),
            "nested"
        );

        std::fs::remove_dir_all(&source).unwrap();
        std::fs::write(&source, "new file").unwrap();
        std::fs::write(target.join("local"), "keep").unwrap();
        assert!(reconcile_cowork_skills(&ctx).is_err());
        assert_eq!(
            std::fs::read_to_string(target.join("local")).unwrap(),
            "keep"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("nested")).unwrap(),
            "nested"
        );
        std::fs::remove_file(target.join("local")).unwrap();
        assert!(reconcile_cowork_skills(&ctx).unwrap());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "new file");
        assert!(!reconcile_cowork_skills(&ctx).unwrap());
    }
}
