//! Microsoft 365 Copilot Cowork deployment repair.
//!
//! Cowork protects its `OneDrive` skill directories from deletion. APM currently
//! replaces colliding skill directories as a unit, so direct Cowork deployment
//! fails once those directories exist. Recopying the already-resolved shared APM
//! skill target converges allowed packages without deleting or replacing Cowork
//! directories.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::engine::Context;
use crate::infra::fs::{copy_dir_recursive, write_atomic};
use anyhow::{Context as _, Result};
use serde::Deserialize;
use serde_yaml_ng::Value;

use super::targets::copilot_cowork_skills_path;

const COWORK_TARGET: &str = "copilot-cowork";
const COWORK_URI_PREFIX: &str = "cowork://";

/// Reconcile APM's shared skill deployment into Cowork's protected skill tree.
///
/// # Errors
///
/// Returns an error when the `OneDrive` location is unavailable, the shared APM
/// skill target is missing, or a skill file cannot be copied.
pub(super) fn reconcile_cowork_skills(ctx: &Context) -> Result<()> {
    let (source, target) = cowork_skill_paths(ctx)?;
    let deployed = desired_cowork_skill_names(ctx.home())?;
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
        if deployed.contains(&name) {
            copy_dir_recursive(&entry.path(), &target_skill, false).with_context(|| {
                format!(
                    "reconciling APM skill {} into Copilot Cowork at {}",
                    name,
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
            && !deployed.contains(&entry.file_name().to_string_lossy().into_owned())
        {
            remove_skill_entry_point(&entry.path())?;
        }
    }
    remove_legacy_cowork_lock_deployments(ctx.home())?;
    Ok(())
}

/// Whether every resolved shared APM skill file is current in Cowork.
///
/// Cowork may retain extra placeholder files because its ACL intentionally
/// prevents deletion. Those do not make the managed files stale.
pub(super) fn cowork_skills_are_current(ctx: &Context) -> Result<bool> {
    let (source, target) = cowork_skill_paths(ctx)?;
    let deployed = desired_cowork_skill_names(ctx.home())?;
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
        if deployed.contains(&name) {
            if !target_skill.is_dir() || !skill_tree_is_current(&entry.path(), &target_skill)? {
                return Ok(false);
            }
        } else if target_skill
            .join("SKILL.md")
            .try_exists()
            .with_context(|| {
                format!(
                    "checking excluded Copilot Cowork skill {}",
                    target_skill.display()
                )
            })?
        {
            return Ok(false);
        }
    }
    if target.is_dir() {
        for entry in
            std::fs::read_dir(&target).with_context(|| format!("reading {}", target.display()))?
        {
            let entry = entry.with_context(|| format!("reading entry in {}", target.display()))?;
            if entry
                .file_type()
                .with_context(|| format!("reading type for {}", entry.path().display()))?
                .is_dir()
                && !deployed.contains(&entry.file_name().to_string_lossy().into_owned())
                && entry
                    .path()
                    .join("SKILL.md")
                    .try_exists()
                    .with_context(|| {
                        format!(
                            "checking excluded Copilot Cowork skill {}",
                            entry.path().display()
                        )
                    })?
            {
                return Ok(false);
            }
        }
    }
    Ok(!legacy_cowork_lock_deployments_exist(ctx.home())?)
}

/// Remove deployment records left by older direct Cowork APM installs.
///
/// Dotfiles owns Cowork file reconciliation now. Leaving those records in APM's
/// shared ledger makes unrelated installs retry directory deletion, which
/// `OneDrive` rejects even when the managed skill files are already current.
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

fn legacy_cowork_lock_deployments_exist(home: &Path) -> Result<bool> {
    let lock_path = home.join(".apm").join("apm.lock.yaml");
    let text = std::fs::read_to_string(&lock_path)
        .with_context(|| format!("reading APM lockfile {}", lock_path.display()))?;
    let mut lock: Value = serde_yaml_ng::from_str(&text)
        .with_context(|| format!("parsing APM lockfile {}", lock_path.display()))?;
    Ok(strip_legacy_cowork_deployments(&mut lock))
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
                let keys: Vec<Value> = hashes
                    .keys()
                    .filter(|key| is_cowork_uri(key))
                    .cloned()
                    .collect();
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

fn skill_tree_is_current(source: &Path, target: &Path) -> Result<bool> {
    for entry in
        std::fs::read_dir(source).with_context(|| format!("reading {}", source.display()))?
    {
        let entry = entry.with_context(|| format!("reading entry in {}", source.display()))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let metadata = source_path
            .symlink_metadata()
            .with_context(|| format!("reading metadata for {}", source_path.display()))?;

        if metadata.is_dir() {
            if !target_path.is_dir() || !skill_tree_is_current(&source_path, &target_path)? {
                return Ok(false);
            }
        } else if metadata.file_type().is_symlink() {
            let source_link = std::fs::read_link(&source_path)
                .with_context(|| format!("reading symlink {}", source_path.display()))?;
            match std::fs::read_link(&target_path) {
                Ok(target_link) if target_link == source_link => {}
                Ok(_) => return Ok(false),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(err) => {
                    return Err(err)
                        .with_context(|| format!("reading symlink {}", target_path.display()));
                }
            }
        } else {
            let source_bytes = std::fs::read(&source_path)
                .with_context(|| format!("reading shared APM skill {}", source_path.display()))?;
            match std::fs::read(&target_path) {
                Ok(target_bytes) if target_bytes == source_bytes => {}
                Ok(_) => return Ok(false),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(err) => {
                    return Err(err).with_context(|| {
                        format!("reading Copilot Cowork skill {}", target_path.display())
                    });
                }
            }
        }
    }
    Ok(true)
}

fn cowork_skill_paths(ctx: &Context) -> Result<(PathBuf, PathBuf)> {
    let source = ctx.home().join(".agents").join("skills");
    anyhow::ensure!(
        source.is_dir(),
        "APM shared skill target {} is missing",
        source.display()
    );

    let target = copilot_cowork_skills_path(ctx)
        .context("ONEDRIVECOMMERCIAL is not set; cannot locate Copilot Cowork skills")?;
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
            .is_some_and(|targets| !targets.iter().any(|target| target == "copilot-cowork"))
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

    use crate::infra::env::MapEnv;
    use crate::infra::exec::MockExecutor;
    use crate::infra::platform::{Os, Platform};

    use super::*;
    use crate::domains::ai::apm::targets::ONEDRIVE_COMMERCIAL;
    use crate::domains::ai::apm::test_fixture::make_context_with_home;

    #[test]
    fn reconcile_overwrites_files_without_replacing_cowork_directory() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let onedrive = dir.path().join("OneDrive - Test");
        let source_skill = dir.path().join(".agents").join("skills").join("example");
        let target_skill = onedrive
            .join("Documents")
            .join("Cowork")
            .join("skills")
            .join("example");
        std::fs::create_dir_all(&source_skill).expect("create source skill");
        std::fs::create_dir_all(&target_skill).expect("create target skill");
        std::fs::create_dir_all(dir.path().join(".apm")).expect("create APM directory");
        std::fs::write(
            dir.path().join(".apm").join("apm.lock.yaml"),
            "dependencies:\n\
             - deployed_files:\n\
             \x20 - .agents/skills/example\n\
             \x20 - .agents/skills/example/SKILL.md\n\
             \x20 target_subset: [agent-skills, copilot-cowork]\n",
        )
        .expect("write lock");
        std::fs::write(source_skill.join("SKILL.md"), "current").expect("write source");
        std::fs::write(target_skill.join("placeholder.txt"), "preserved")
            .expect("write placeholder");

        let ctx = make_context_with_home(
            dir.path(),
            Platform::new(Os::Windows, false),
            MockExecutor::new(),
        )
        .with_env(Arc::new(MapEnv::new().with(ONEDRIVE_COMMERCIAL, &onedrive)));

        reconcile_cowork_skills(&ctx).expect("reconcile skills");

        assert!(cowork_skills_are_current(&ctx).expect("compare skills"));
        assert_eq!(
            std::fs::read_to_string(target_skill.join("SKILL.md")).expect("read target"),
            "current"
        );
        assert!(target_skill.join("placeholder.txt").exists());

        std::fs::write(target_skill.join("SKILL.md"), "stale").expect("rewrite target");
        assert!(!cowork_skills_are_current(&ctx).expect("compare stale skills"));
    }

    #[test]
    fn reconcile_removes_entry_point_for_skill_excluded_from_cowork() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let onedrive = dir.path().join("OneDrive - Test");
        let source_skill = dir.path().join(".agents").join("skills").join("mcp-only");
        let target_skill = onedrive
            .join("Documents")
            .join("Cowork")
            .join("skills")
            .join("mcp-only");
        std::fs::create_dir_all(&source_skill).expect("create source skill");
        std::fs::create_dir_all(&target_skill).expect("create target skill");
        std::fs::create_dir_all(dir.path().join(".apm")).expect("create APM directory");
        std::fs::write(
            dir.path().join(".apm").join("apm.lock.yaml"),
            "dependencies:\n\
             - deployed_files:\n\
             \x20 - .agents/skills/mcp-only\n\
             \x20 - .agents/skills/mcp-only/SKILL.md\n\
             \x20 - cowork://skills/mcp-only\n\
             \x20 target_subset: [agent-skills]\n",
        )
        .expect("write lock");
        std::fs::write(source_skill.join("SKILL.md"), "requires MCP").expect("write source");
        std::fs::write(target_skill.join("SKILL.md"), "requires MCP").expect("write target");
        std::fs::write(target_skill.join("placeholder.txt"), "preserved")
            .expect("write placeholder");

        let ctx = make_context_with_home(
            dir.path(),
            Platform::new(Os::Windows, false),
            MockExecutor::new(),
        )
        .with_env(Arc::new(MapEnv::new().with(ONEDRIVE_COMMERCIAL, &onedrive)));

        assert!(!cowork_skills_are_current(&ctx).expect("compare skills"));
        reconcile_cowork_skills(&ctx).expect("reconcile skills");
        assert!(cowork_skills_are_current(&ctx).expect("compare skills"));
        assert!(!target_skill.join("SKILL.md").exists());
        assert!(target_skill.join("placeholder.txt").exists());
    }

    #[test]
    fn reconcile_removes_entry_point_for_package_no_longer_installed() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let onedrive = dir.path().join("OneDrive - Test");
        let target_skill = onedrive
            .join("Documents")
            .join("Cowork")
            .join("skills")
            .join("removed");
        std::fs::create_dir_all(dir.path().join(".agents").join("skills"))
            .expect("create source skills");
        std::fs::create_dir_all(&target_skill).expect("create target skill");
        std::fs::create_dir_all(dir.path().join(".apm")).expect("create APM directory");
        std::fs::write(
            dir.path().join(".apm").join("apm.lock.yaml"),
            "dependencies: []\n",
        )
        .expect("write lock");
        std::fs::write(target_skill.join("SKILL.md"), "removed").expect("write target");
        std::fs::write(target_skill.join("placeholder.txt"), "preserved")
            .expect("write placeholder");

        let ctx = make_context_with_home(
            dir.path(),
            Platform::new(Os::Windows, false),
            MockExecutor::new(),
        )
        .with_env(Arc::new(MapEnv::new().with(ONEDRIVE_COMMERCIAL, &onedrive)));

        assert!(!cowork_skills_are_current(&ctx).expect("compare skills"));
        reconcile_cowork_skills(&ctx).expect("reconcile skills");
        assert!(cowork_skills_are_current(&ctx).expect("compare skills"));
        assert!(!target_skill.join("SKILL.md").exists());
        assert!(target_skill.join("placeholder.txt").exists());
    }

    #[test]
    fn reconcile_removes_legacy_cowork_entries_from_apm_lock() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let onedrive = dir.path().join("OneDrive - Test");
        let source_skill = dir.path().join(".agents").join("skills").join("example");
        let target_skill = onedrive
            .join("Documents")
            .join("Cowork")
            .join("skills")
            .join("example");
        std::fs::create_dir_all(&source_skill).expect("create source skill");
        std::fs::create_dir_all(&target_skill).expect("create target skill");
        std::fs::create_dir_all(dir.path().join(".apm")).expect("create APM directory");
        std::fs::write(source_skill.join("SKILL.md"), "current").expect("write source");
        std::fs::write(target_skill.join("SKILL.md"), "current").expect("write target");
        std::fs::write(
            dir.path().join(".apm").join("apm.lock.yaml"),
            "dependencies:\n\
             - deployed_files:\n\
             \x20 - .agents/skills/example/SKILL.md\n\
             \x20 - cowork://skills/example\n\
             \x20 deployed_file_hashes:\n\
             \x20   cowork://skills/example: stale\n\
             \x20   copilot-app://workflows/example: current\n\
             \x20 target_subset: [agent-skills, copilot-cowork]\n\
             deployments:\n\
             - kind: uri\n\
             \x20 target: copilot-cowork\n\
             \x20 value: cowork://skills/example\n\
             - kind: uri\n\
             \x20 target: copilot-app\n\
             \x20 value: copilot-app://workflows/example\n",
        )
        .expect("write lock");

        let ctx = make_context_with_home(
            dir.path(),
            Platform::new(Os::Windows, false),
            MockExecutor::new(),
        )
        .with_env(Arc::new(MapEnv::new().with(ONEDRIVE_COMMERCIAL, &onedrive)));

        assert!(!cowork_skills_are_current(&ctx).expect("detect stale ledger"));
        reconcile_cowork_skills(&ctx).expect("reconcile skills");
        assert!(cowork_skills_are_current(&ctx).expect("compare reconciled deployment"));

        let lock = std::fs::read_to_string(dir.path().join(".apm").join("apm.lock.yaml"))
            .expect("read lock");
        assert!(!lock.contains("cowork://"));
        assert!(lock.contains(".agents/skills/example/SKILL.md"));
        assert!(lock.contains("copilot-app://workflows/example"));
    }

    #[test]
    fn reconcile_requires_cowork_onedrive_location() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::create_dir_all(dir.path().join(".agents").join("skills"))
            .expect("create shared skill target");
        let ctx = make_context_with_home(
            dir.path(),
            Platform::new(Os::Windows, false),
            MockExecutor::new(),
        )
        .with_env(Arc::new(MapEnv::new()));

        let err = reconcile_cowork_skills(&ctx).expect_err("missing OneDrive should fail");
        assert!(format!("{err:#}").contains(ONEDRIVE_COMMERCIAL));
    }
}
