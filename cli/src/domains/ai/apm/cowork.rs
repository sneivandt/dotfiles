//! Microsoft 365 Copilot Cowork deployment repair.
//!
//! Cowork protects its `OneDrive` skill directories from deletion. Current APM
//! still replaces a colliding skill directory with `rmtree` + `copytree`, so
//! dotfiles copies the already-resolved shared skill tree into each existing
//! Cowork directory without replacing that directory. New files take their
//! install permissions from the source; later content updates keep destination
//! access or refuse the replacement if it cannot be preserved.

mod legacy_lock;
mod ownership;
mod skill_files;

use legacy_lock::check_legacy_entry_point;
#[cfg(test)]
use legacy_lock::decode_sha256_hex;
#[cfg(test)]
use legacy_lock::legacy_entry_point_fingerprints;
#[cfg(test)]
use legacy_lock::record_legacy_cowork_deployment;
pub(super) use legacy_lock::remove_legacy_cowork_lock_deployments;
#[cfg(any(target_os = "linux", test))]
use legacy_lock::strip_legacy_cowork_deployments;
#[cfg(test)]
use ownership::file_fingerprint;
use ownership::{
    index_files, ownership_key, prior_files, prune_stale_files, read_file_inventory,
    read_owned_skills, write_file_inventory, write_owned_skills,
};
use skill_files::{
    copy_skill_files, prepare_skill_paths, selected_skill_inventory, skill_files_match,
    validate_selected_links,
};

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
        let before_owned = owned.len();
        let before_inventory = inventory.len();
        for alias in &aliases {
            owned.remove(alias);
            inventory.remove(alias);
        }
        if owned.len() != before_owned {
            write_owned_skills(&target, &owned)?;
            changed = true;
        }
        if inventory.len() != before_inventory {
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
    let pruned = prepare_skill_paths(&target_skill, previous.as_ref(), &active)?;
    let files_changed = !skill_files_match(&source_skill, &target_skill)?;
    if files_changed {
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
    }
    let mut changed = pruned || files_changed;
    if changed {
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

fn valid_skill_name(name: &str) -> bool {
    !name.is_empty() && !matches!(name, "." | "..") && !name.contains(['/', '\\', ':'])
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
mod tests;
