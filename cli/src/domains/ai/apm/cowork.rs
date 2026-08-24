//! Microsoft 365 Copilot Cowork deployment repair.
//!
//! Cowork protects its `OneDrive` skill directories from deletion. Current APM
//! still replaces a colliding skill directory with `rmtree` + `copytree`, so
//! dotfiles copies the already-resolved shared skill tree into each existing
//! Cowork directory without replacing that directory.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::Deserialize;
use serde_yaml_ng::Value;

use super::targets::copilot_cowork_skills_path;
use crate::engine::Context;
use crate::infra::fs::{copy_dir_recursive, write_atomic};

const COWORK_TARGET: &str = "copilot-cowork";
const COWORK_URI_PREFIX: &str = "cowork://";

/// Reconcile APM's shared skill deployment into Cowork's protected skill tree.
///
/// # Errors
///
/// Returns an error when the configured Cowork path, shared APM skills, lock
/// state, or a managed file cannot be read or written.
pub(super) fn reconcile_cowork_skills(ctx: &Context) -> Result<()> {
    let (source, target) = cowork_skill_paths(ctx)?;
    let desired = desired_cowork_skill_names(ctx.home())?;
    std::fs::create_dir_all(&target)
        .with_context(|| format!("creating Copilot Cowork skill target {}", target.display()))?;

    for entry in
        std::fs::read_dir(&source).with_context(|| format!("reading {}", source.display()))?
    {
        let entry = entry.with_context(|| format!("reading entry in {}", source.display()))?;
        if !entry
            .file_type()
            .with_context(|| format!("reading type for {}", entry.path().display()))?
            .is_dir()
        {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let target_skill = target.join(&name);
        if desired.contains(&name) {
            copy_dir_recursive(&entry.path(), &target_skill, false).with_context(|| {
                format!(
                    "reconciling APM skill {name} into Copilot Cowork at {}",
                    target_skill.display()
                )
            })?;
        } else {
            remove_skill_entry_point(&target_skill)?;
        }
    }

    for entry in
        std::fs::read_dir(&target).with_context(|| format!("reading {}", target.display()))?
    {
        let entry = entry.with_context(|| format!("reading entry in {}", target.display()))?;
        if entry
            .file_type()
            .with_context(|| format!("reading type for {}", entry.path().display()))?
            .is_dir()
            && !desired.contains(&entry.file_name().to_string_lossy().into_owned())
        {
            remove_skill_entry_point(&entry.path())?;
        }
    }

    remove_legacy_cowork_lock_deployments(ctx.home())?;
    Ok(())
}

/// Remove records left by direct APM Cowork installs.
///
/// Dotfiles owns the ACL-sensitive Cowork copy. Leaving `cowork://` records in
/// APM's ledger makes later installs retry directory replacement.
pub(super) fn remove_legacy_cowork_lock_deployments(home: &Path) -> Result<bool> {
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
    let mut lock: Value = serde_yaml_ng::from_str(&text)
        .with_context(|| format!("parsing APM lockfile {}", lock_path.display()))?;
    if !strip_legacy_cowork_deployments(&mut lock) {
        return Ok(false);
    }
    let serialized = serde_yaml_ng::to_string(&lock)
        .with_context(|| format!("serializing APM lockfile {}", lock_path.display()))?;
    write_atomic(&lock_path, serialized)
        .with_context(|| format!("updating APM lockfile {}", lock_path.display()))?;
    Ok(true)
}

fn strip_legacy_cowork_deployments(lock: &mut Value) -> bool {
    let Some(root) = lock.as_mapping_mut() else {
        return false;
    };
    let mut changed = false;

    if let Some(dependencies) = root
        .get_mut(Value::String("dependencies".to_owned()))
        .and_then(Value::as_sequence_mut)
    {
        for dependency in dependencies {
            let Some(dependency) = dependency.as_mapping_mut() else {
                continue;
            };
            if let Some(files) = dependency
                .get_mut(Value::String("deployed_files".to_owned()))
                .and_then(Value::as_sequence_mut)
            {
                let before = files.len();
                files.retain(|file| !is_cowork_uri(file));
                changed |= files.len() != before;
            }
            if let Some(hashes) = dependency
                .get_mut(Value::String("deployed_file_hashes".to_owned()))
                .and_then(Value::as_mapping_mut)
            {
                let keys = hashes
                    .keys()
                    .filter(|key| is_cowork_uri(key))
                    .cloned()
                    .collect::<Vec<_>>();
                changed |= !keys.is_empty();
                for key in keys {
                    hashes.remove(&key);
                }
            }
        }
    }

    if let Some(deployments) = root.get_mut(Value::String("deployments".to_owned())) {
        match deployments {
            Value::Sequence(records) => {
                let before = records.len();
                records.retain(|record| !is_cowork_deployment(record));
                changed |= records.len() != before;
            }
            Value::Mapping(by_owner) => {
                for records in by_owner.values_mut().filter_map(Value::as_sequence_mut) {
                    let before = records.len();
                    records.retain(|record| !is_cowork_deployment(record));
                    changed |= records.len() != before;
                }
            }
            Value::Null
            | Value::Bool(_)
            | Value::Number(_)
            | Value::String(_)
            | Value::Tagged(_) => {}
        }
    }

    changed
}

fn is_cowork_deployment(value: &Value) -> bool {
    if is_cowork_uri(value) {
        return true;
    }
    let Some(record) = value.as_mapping() else {
        return false;
    };
    record
        .get(Value::String("target".to_owned()))
        .and_then(Value::as_str)
        .is_some_and(|target| target == COWORK_TARGET)
        || record
            .get(Value::String("value".to_owned()))
            .is_some_and(is_cowork_uri)
}

fn is_cowork_uri(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|value| value.starts_with(COWORK_URI_PREFIX))
}

fn remove_skill_entry_point(target_skill: &Path) -> Result<()> {
    let entry_point = target_skill.join("SKILL.md");
    match std::fs::remove_file(&entry_point) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| {
            format!(
                "removing excluded Copilot Cowork skill entry point {}",
                entry_point.display()
            )
        }),
    }
}

fn cowork_skill_paths(ctx: &Context) -> Result<(PathBuf, PathBuf)> {
    let source = ctx.home().join(".agents").join("skills");
    anyhow::ensure!(
        source.is_dir(),
        "APM shared skill target {} is missing",
        source.display()
    );
    let target = copilot_cowork_skills_path(ctx).context(
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
    let lock: ApmLock = serde_yaml_ng::from_str(&lock)
        .with_context(|| format!("parsing APM lockfile {}", lock_path.display()))?;
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
                && !name.is_empty()
            {
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
    fn reconcile_removes_entry_point_when_package_excludes_cowork() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let (_, target_skill, ctx) = setup_skill(dir.path(), "[agent-skills]");
        std::fs::write(target_skill.join("SKILL.md"), "old").expect("write target");
        std::fs::write(target_skill.join("placeholder.txt"), "preserved")
            .expect("write placeholder");

        reconcile_cowork_skills(&ctx).expect("reconcile skills");

        assert!(!target_skill.join("SKILL.md").exists());
        assert!(target_skill.join("placeholder.txt").exists());
    }

    #[test]
    fn removes_legacy_cowork_lock_entries() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::create_dir_all(dir.path().join(".apm")).expect("create APM directory");
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

        assert!(remove_legacy_cowork_lock_deployments(dir.path()).expect("remove legacy records"));
        let lock = std::fs::read_to_string(dir.path().join(".apm").join("apm.lock.yaml"))
            .expect("read lock");
        assert!(!lock.contains("cowork://"));
        assert!(lock.contains(".agents/skills/example/SKILL.md"));
    }
}
