#[cfg(windows)]
use super::stored_entry_name;
use super::{
    BTreeMap, BTreeSet, FILE_OWNERSHIP_FILE, FileInventory, OWNERSHIP_FILE, OwnedFile, Path,
    PathBuf, Result, SkillInventory, is_dir_like, reject_destination_link,
    symlink_metadata_optional, valid_skill_name, write_atomic,
};
use anyhow::Context as _;

pub(super) fn ownership_key(root: &Path, relative: &str) -> Result<String> {
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

pub(super) fn index_files<'a>(
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

pub(super) fn prior_files(
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

pub(super) fn write_file_inventory(target: &Path, inventory: &SkillInventory) -> Result<()> {
    write_atomic(
        &target.join(FILE_OWNERSHIP_FILE),
        serde_json::to_vec_pretty(inventory)?,
    )
}

pub(super) fn file_fingerprint(path: &Path) -> Result<Option<OwnedFile>> {
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

pub(super) fn inventory_files(root: &Path, path: &Path, files: &mut FileInventory) -> Result<()> {
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

pub(super) fn read_file_inventory(target: &Path) -> Result<SkillInventory> {
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

pub(super) fn check_active_files(
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

pub(super) fn prune_stale_files(
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

pub(super) fn read_owned_skills(target: &Path) -> Result<BTreeSet<String>> {
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

pub(super) fn write_owned_skills(target: &Path, names: &BTreeSet<String>) -> Result<()> {
    reject_destination_link(target)?;
    let path = target.join(OWNERSHIP_FILE);
    let content = serde_json::to_string_pretty(names).context("serializing Cowork ownership")?;
    write_atomic(&path, format!("{content}\n"))
        .with_context(|| format!("writing {}", path.display()))
}
