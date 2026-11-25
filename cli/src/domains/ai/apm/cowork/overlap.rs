//! Protect all shared skill directories, not just the files selected for copy.
//! A bind alias can hide its original parents; directory identities in the
//! visible tree, not inferred mount spellings, establish what is protected.

use anyhow::{Context as _, Result};
use std::collections::{BTreeSet, HashSet};
use std::fs::Metadata;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use super::{
    read_file_inventory, read_owned_skills, selected_names_from_lock,
    strip_legacy_cowork_deployments,
};
use crate::infra::cancellation::CancellationToken;
use crate::infra::exec::{ExecError, ExecResult};
use crate::infra::fs::{reject_destination_link, resolve_directory, symlink_metadata_optional};

type Id = (u64, u64);

struct WalkBudget<'a> {
    cancelled: &'a CancellationToken,
    remaining: usize,
}

impl WalkBudget<'_> {
    fn step(&mut self) -> Result<()> {
        if self.cancelled.is_cancelled() {
            return Err(ExecError::Cancelled {
                command: "Cowork directory preflight".into(),
                result: ExecResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: false,
                    code: None,
                },
            }
            .into());
        }
        self.remaining = self
            .remaining
            .checked_sub(1)
            .context("Cowork directory preflight exceeded its traversal limit")?;
        Ok(())
    }
}

pub(in crate::domains::ai::apm) fn check(
    home: &Path,
    destination: &Path,
    cancelled: &CancellationToken,
) -> Result<()> {
    check_with(home, destination, cancelled, |_, meta| {
        (meta.dev(), meta.ino())
    })
}

fn check_with(
    home: &Path,
    destination: &Path,
    cancelled: &CancellationToken,
    identify: impl Fn(&Path, &Metadata) -> Id,
) -> Result<()> {
    let mut budget = WalkBudget {
        cancelled,
        remaining: 1_000_000,
    };
    budget.step()?;
    reject_destination_link(destination)?;
    let source = resolve_directory(&home.join(".agents/skills"))?;
    let mut owned = BTreeSet::new();
    let lock_path = home.join(".apm/apm.lock.yaml");
    let selected = if symlink_metadata_optional(&lock_path, "inspect APM lock")?.is_some() {
        let mut lock: super::Value = crate::infra::yaml::from_slice(&std::fs::read(&lock_path)?)?;
        // Migration may add stale entry points to either ledger before pruning.
        strip_legacy_cowork_deployments(&mut lock, &mut owned);
        selected_names_from_lock(serde_json::from_value(lock)?)?
    } else {
        BTreeSet::new()
    };
    let mut candidates: BTreeSet<PathBuf> = selected.iter().map(PathBuf::from).collect();
    let target = resolve_directory(destination)?;
    // If the destination is missing, creating it touches its existing parent.
    let target_anchor = target
        .ancestors()
        .find_map(|ancestor| {
            symlink_metadata_optional(ancestor, "inspect Cowork target parent")
                .transpose()
                .map(|probe| probe.map(|meta| identify(ancestor, &meta)))
        })
        .transpose()?
        .context("Cowork destination has no existing ancestor")?;
    let mut source_ids = HashSet::new();
    let mut visit = |path: &Path, meta: &Metadata| -> Result<Id> {
        let id = identify(path, meta);
        anyhow::ensure!(
            id != target_anchor,
            "Cowork destination {} overlaps shared skills at {}",
            destination.display(),
            path.display()
        );
        source_ids.insert(id);
        Ok(id)
    };
    if let Some(meta) = symlink_metadata_optional(&source, "inspect shared skills")? {
        let mut parents = HashSet::from([visit(&source, &meta)?]);
        for entry in source.read_dir()? {
            budget.step()?;
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let name = entry.file_name();
                let copying = name.to_str().is_some_and(|name| selected.contains(name));
                collect(
                    &source,
                    Path::new(&name),
                    &mut visit,
                    &mut parents,
                    copying.then_some(&mut candidates),
                    &mut budget,
                )?;
            }
        }
    }
    // Do not open destination ledgers before ruling out a target root inside
    // even an unselected shared skill.
    owned.extend(read_owned_skills(destination)?);
    candidates.extend(owned.into_iter().map(PathBuf::from));
    for (name, files) in read_file_inventory(destination)? {
        candidates.insert(name.clone().into());
        for relative in files.keys() {
            budget.step()?;
            candidates.extend(
                Path::new(&name)
                    .join(relative)
                    .ancestors()
                    .map(Path::to_owned),
            );
        }
    }

    let mut directories = HashSet::from([PathBuf::new()]);
    // Parents sort before children. Do not follow a symlink or descend through
    // a file; the copier handles owned file/directory transitions separately.
    for relative in candidates {
        budget.step()?;
        if !relative
            .parent()
            .is_some_and(|parent| directories.contains(parent))
        {
            continue;
        }
        let path = target.join(&relative);
        if let Some(meta) = symlink_metadata_optional(&path, "inspect Cowork directory")?
            && meta.is_dir()
        {
            anyhow::ensure!(
                !source_ids.contains(&identify(&path, &meta)),
                "Cowork destination directory {} overlaps shared skills; choose a separate directory",
                path.display()
            );
            directories.insert(relative);
        }
    }
    budget.step()
}

fn collect(
    source: &Path,
    relative: &Path,
    visit: &mut impl FnMut(&Path, &Metadata) -> Result<Id>,
    parents: &mut HashSet<Id>,
    mut candidates: Option<&mut BTreeSet<PathBuf>>,
    budget: &mut WalkBudget<'_>,
) -> Result<()> {
    budget.step()?;
    anyhow::ensure!(
        parents.len() < 128,
        "Cowork directory preflight exceeded its depth limit"
    );
    let directory = source.join(relative);
    let meta = directory
        .symlink_metadata()
        .with_context(|| format!("inspect shared skill {}", directory.display()))?;
    anyhow::ensure!(
        meta.is_dir(),
        "shared directory changed at {}",
        directory.display()
    );
    let id = visit(&directory, &meta)?;
    anyhow::ensure!(
        parents.insert(id),
        "cyclic mount in shared skill {}",
        directory.display()
    );
    for entry in directory.read_dir()? {
        budget.step()?;
        let entry = entry?;
        let is_dir = entry.file_type()?.is_dir();
        if !is_dir && candidates.is_none() {
            continue;
        }
        let child = relative.join(entry.file_name());
        if is_dir {
            collect(
                source,
                &child,
                visit,
                parents,
                candidates.as_deref_mut(),
                budget,
            )?;
        }
        if let Some(paths) = candidates.as_mut() {
            paths.insert(child);
        }
    }
    parents.remove(&id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_nested_objects_block_both_copy_and_stale_directory_routes() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let source = fixture.path().join(".agents/skills");
        let target = fixture.path().join("cowork");
        let nested = source.join("selected/nested");
        let unselected = source.join("unselected/private/nested");
        let stale = target.join("retired/nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(&unselected).unwrap();
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::write(stale.join("keep"), "keep").unwrap();
        std::fs::create_dir(fixture.path().join(".apm")).unwrap();
        std::fs::write(
            fixture.path().join(".apm/apm.lock.yaml"),
            "dependencies:\n- deployed_files: [.agents/skills/selected/SKILL.md]\n",
        )
        .unwrap();
        // This unchanged fingerprint would qualify the stale file for removal.
        let ledger = target.join(super::super::FILE_OWNERSHIP_FILE);
        let previous = serde_json::to_vec(&serde_json::json!({
            "retired": {"nested/keep": {"hash": crate::infra::fs::content_hash(b"keep"), "link": false}}
        })).unwrap();
        std::fs::write(&ledger, &previous).unwrap();
        let cancel = CancellationToken::new();
        for (read, write) in [
            (&nested, &target),
            (&source, &stale),
            (&unselected, &target),
            (&unselected, &stale),
        ] {
            let write = resolve_directory(write).unwrap();
            let result = check_with(fixture.path(), &target, &cancel, |p, meta| {
                if p == read || p == write {
                    (0, 999)
                } else {
                    (meta.dev(), meta.ino())
                }
            });
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("overlaps shared skills")
            );
            assert_eq!(std::fs::read(stale.join("keep")).unwrap(), b"keep");
            assert_eq!(std::fs::read(&ledger).unwrap(), previous);
            assert!(!target.join("selected").exists());
        }
        // A symlink under an unselected skill is not a directory traversal.
        std::os::unix::fs::symlink(&target, unselected.join("unfollowed")).unwrap();
        check(fixture.path(), &target, &cancel).unwrap();

        // Real dev/ino overlap and symlink destinations are rejected by check().
        let err = check(fixture.path(), &source, &cancel).unwrap_err();
        assert!(
            err.to_string().contains("overlaps shared skills"),
            "{err:#}"
        );
        let linked_dest = fixture.path().join("linked-cowork");
        std::os::unix::fs::symlink(&target, &linked_dest).unwrap();
        assert!(check(fixture.path(), &linked_dest, &cancel).is_err());
    }

    #[test]
    fn unselected_directory_checks_stop_on_cycles_and_cancellation_before_ledger_reads() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let source = fixture.path().join(".agents/skills");
        let unselected = source.join("unselected");
        let target = fixture.path().join("destination");
        std::fs::create_dir_all(unselected.join("child")).unwrap();
        std::fs::create_dir(&target).unwrap();
        let ledger = target.join(super::super::OWNERSHIP_FILE);
        std::fs::write(&ledger, "not a ledger").unwrap();
        let cancel = CancellationToken::new();
        let cycle = check_with(fixture.path(), &target, &cancel, |p, meta| {
            if p.starts_with(&unselected) {
                (0, 999)
            } else {
                (meta.dev(), meta.ino())
            }
        })
        .unwrap_err();
        assert!(cycle.to_string().contains("cyclic mount"));
        let stopped = check_with(fixture.path(), &target, &cancel, |p, meta| {
            if p == unselected {
                cancel.cancel();
            }
            (meta.dev(), meta.ino())
        })
        .unwrap_err();
        assert!(matches!(
            stopped.downcast_ref::<ExecError>(),
            Some(ExecError::Cancelled { .. })
        ));
        assert_eq!(std::fs::read(ledger).unwrap(), b"not a ledger");
    }
}
