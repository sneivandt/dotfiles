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
use crate::infra::logging::OutputExt as _;

const COWORK_TARGET: &str = "copilot-cowork";
const COWORK_URI_PREFIX: &str = "cowork://";
const OWNERSHIP_FILE: &str = ".dotfiles-apm-skills.json";

/// Reconcile APM's shared skill deployment into Cowork's protected skill tree.
///
/// # Errors
///
/// Returns an error when the configured Cowork path, shared APM skills, lock
/// state, or a managed file cannot be read or written.
pub(super) fn reconcile_cowork_skills(ctx: &Context) -> Result<bool> {
    let (source, target) = cowork_skill_paths(ctx)?;
    let mut changed = remove_legacy_cowork_lock_deployments(ctx)?;
    let desired = desired_cowork_skill_names(ctx.home())?;
    let mut owned = read_owned_skills(&target)?;
    changed |= !target.exists();
    std::fs::create_dir_all(&target)
        .with_context(|| format!("creating Copilot Cowork skill target {}", target.display()))?;

    for name in &desired {
        let source_skill = source.join(name);
        let target_skill = target.join(name);
        anyhow::ensure!(
            source_skill.is_dir(),
            "APM shared skill {} is missing",
            source_skill.display()
        );
        if !skill_files_match(&source_skill, &target_skill)? {
            copy_dir_recursive(&source_skill, &target_skill, false).with_context(|| {
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
            changed = true;
            ctx.log()
                .info(format!("updated: Copilot Cowork skill {name}"));
        }
        if owned.insert(name.clone()) {
            // Record each completed copy before another skill can fail.
            write_owned_skills(&target, &owned)?;
            changed = true;
        }
    }

    for name in owned.difference(&desired) {
        if remove_skill_entry_point(&target.join(name))? {
            changed = true;
            ctx.log()
                .info(format!("removed: Copilot Cowork skill {name}"));
        }
    }
    if owned != desired {
        write_owned_skills(&target, &desired)?;
        changed = true;
    }

    Ok(changed)
}

fn read_owned_skills(target: &Path) -> Result<BTreeSet<String>> {
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
    if source_meta.file_type() != target_meta.file_type() {
        return Ok(false);
    }
    if source_meta.file_type().is_symlink() {
        return Ok(std::fs::read_link(source)? == std::fs::read_link(target)?);
    }
    // The shared copier preserves directory modes on Unix only. Windows
    // directory attributes belong to Cowork and do not indicate file drift.
    #[cfg(unix)]
    let compare_permissions = true;
    #[cfg(not(unix))]
    let compare_permissions = source_meta.is_file();
    if compare_permissions && source_meta.permissions() != target_meta.permissions() {
        return Ok(false);
    }
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
    let mut lock: Value = serde_yaml_ng::from_str(&text)
        .with_context(|| format!("parsing APM lockfile {}", lock_path.display()))?;
    let mut legacy_skills = BTreeSet::new();
    if !strip_legacy_cowork_deployments(&mut lock, &mut legacy_skills) {
        return Ok(false);
    }
    if let Some(target) = copilot_cowork_skills_path(ctx)
        && target
            .try_exists()
            .with_context(|| format!("checking {}", target.display()))?
    {
        let mut owned = read_owned_skills(&target)?;
        let before = owned.clone();
        owned.extend(legacy_skills);
        if owned != before {
            // Preserve explicit native deployment ownership before APM rewrites
            // the lock. Shared .agents entries alone are not Cowork ownership.
            write_owned_skills(&target, &owned)?;
        }
    }
    let serialized = serde_yaml_ng::to_string(&lock)
        .with_context(|| format!("serializing APM lockfile {}", lock_path.display()))?;
    write_atomic(&lock_path, serialized)
        .with_context(|| format!("updating APM lockfile {}", lock_path.display()))?;
    Ok(true)
}

fn strip_legacy_cowork_deployments(lock: &mut Value, names: &mut BTreeSet<String>) -> bool {
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
                files.retain(|file| !record_legacy_cowork_uri(file, names));
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
                    record_legacy_cowork_uri(&key, names);
                    hashes.remove(&key);
                }
            }
        }
    }

    if let Some(deployments) = root.get_mut(Value::String("deployments".to_owned())) {
        match deployments {
            Value::Sequence(records) => {
                let before = records.len();
                records.retain(|record| !record_legacy_cowork_deployment(record, names));
                changed |= records.len() != before;
            }
            Value::Mapping(by_owner) => {
                for records in by_owner.values_mut().filter_map(Value::as_sequence_mut) {
                    let before = records.len();
                    records.retain(|record| !record_legacy_cowork_deployment(record, names));
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

fn record_legacy_cowork_deployment(value: &Value, names: &mut BTreeSet<String>) -> bool {
    if record_legacy_cowork_uri(value, names) {
        return true;
    }
    if let Some(uri) = value.get("value") {
        record_legacy_cowork_uri(uri, names);
    }
    is_cowork_deployment(value)
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

fn remove_skill_entry_point(target_skill: &Path) -> Result<bool> {
    let entry_point = target_skill.join("SKILL.md");
    match std::fs::remove_file(&entry_point) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
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
    fn reconcile_keeps_ownership_of_completed_copies_when_a_later_skill_fails() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let (_, target_skill, ctx) = setup_skill(dir.path(), "[agent-skills, copilot-cowork]");
        std::fs::write(
            dir.path().join(".apm").join("apm.lock.yaml"),
            "dependencies:\n  - deployed_files:\n      - .agents/skills/example/SKILL.md\n\
             \x20     - .agents/skills/z-missing/SKILL.md\n",
        )
        .expect("write missing source dependency");

        let error = reconcile_cowork_skills(&ctx).expect_err("missing source must fail");
        assert!(error.to_string().contains("z-missing"));
        assert_eq!(
            read_owned_skills(target_skill.parent().unwrap()).unwrap(),
            BTreeSet::from(["example".to_string()])
        );
        assert!(target_skill.join("SKILL.md").exists());
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
            let mut lock: Value = serde_yaml_ng::from_str(record).unwrap();
            let mut names = BTreeSet::new();
            assert!(strip_legacy_cowork_deployments(&mut lock, &mut names));
            assert_eq!(names, BTreeSet::from(["example".to_string()]));
        }
        let mut lock: Value = serde_yaml_ng::from_str(
            "dependencies:\n- deployed_files: [.agents/skills/personal/SKILL.md]\n\
             deployments:\n- value: cowork://skills/../SKILL.md\n",
        )
        .unwrap();
        let mut names = BTreeSet::new();
        assert!(strip_legacy_cowork_deployments(&mut lock, &mut names));
        assert!(names.is_empty());
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
        assert!(reconcile_cowork_skills(&ctx).expect("remove legacy skill"));
        assert!(!target_skill.join("SKILL.md").exists());
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
    fn reconcile_migrates_legacy_records_created_by_the_native_command() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let (_, target_skill, ctx) = setup_skill(dir.path(), "[agent-skills]");
        let lock_path = dir.path().join(".apm").join("apm.lock.yaml");
        std::fs::write(
            &lock_path,
            "dependencies:\n  - deployed_files:\n      - cowork://skills/example/SKILL.md\n",
        )
        .unwrap();
        std::fs::write(target_skill.join("SKILL.md"), "legacy").unwrap();

        assert!(reconcile_cowork_skills(&ctx).expect("migrate and reconcile"));
        assert!(
            !std::fs::read_to_string(lock_path)
                .unwrap()
                .contains("cowork://")
        );
        assert!(!target_skill.join("SKILL.md").exists());
        assert!(!reconcile_cowork_skills(&ctx).expect("repeat reconciliation"));
    }
}
