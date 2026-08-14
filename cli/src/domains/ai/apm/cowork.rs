//! Microsoft 365 Copilot Cowork deployment repair.
//!
//! Cowork protects its `OneDrive` skill directories from deletion. APM currently
//! replaces colliding skill directories as a unit, so previously-created
//! Cowork placeholders can be left without their managed files. Recopying the
//! already-resolved shared APM skill target converges those directories without
//! deleting or replacing them.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::engine::Context;
use crate::infra::fs::copy_dir_recursive;
use anyhow::{Context as _, Result};

const ONEDRIVE_COMMERCIAL: &str = "ONEDRIVECOMMERCIAL";

/// Reconcile APM's shared skill deployment into Cowork's protected skill tree.
///
/// # Errors
///
/// Returns an error when the `OneDrive` location is unavailable, the shared APM
/// skill target is missing, or a skill file cannot be copied.
pub(super) fn reconcile_cowork_skills(ctx: &Context) -> Result<()> {
    let (source, target) = cowork_skill_paths(ctx)?;
    let deployed = deployed_cowork_skill_names(ctx.home())?;
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
            let entry_point = target_skill.join("SKILL.md");
            match std::fs::remove_file(&entry_point) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => {
                    return Err(err).with_context(|| {
                        format!(
                            "removing excluded Copilot Cowork skill entry point {}",
                            entry_point.display()
                        )
                    });
                }
            }
        }
    }
    Ok(())
}

/// Whether every resolved shared APM skill file is current in Cowork.
///
/// Cowork may retain extra placeholder files because its ACL intentionally
/// prevents deletion. Those do not make the managed files stale.
pub(super) fn cowork_skills_are_current(ctx: &Context) -> Result<bool> {
    let (source, target) = cowork_skill_paths(ctx)?;
    let deployed = deployed_cowork_skill_names(ctx.home())?;
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
    Ok(true)
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

    let onedrive = ctx
        .env()
        .var_os(ONEDRIVE_COMMERCIAL)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .context("ONEDRIVECOMMERCIAL is not set; cannot locate Copilot Cowork skills")?;
    let target = onedrive.join("Documents").join("Cowork").join("skills");
    Ok((source, target))
}

fn deployed_cowork_skill_names(home: &Path) -> Result<BTreeSet<String>> {
    let lock_path = home.join(".apm").join("apm.lock.yaml");
    let lock = std::fs::read_to_string(&lock_path)
        .with_context(|| format!("reading APM lockfile {}", lock_path.display()))?;
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&lock)
        .with_context(|| format!("parsing APM lockfile {}", lock_path.display()))?;
    let mut names = BTreeSet::new();
    collect_cowork_skill_names(&value, &mut names);
    Ok(names)
}

fn collect_cowork_skill_names(value: &serde_yaml_ng::Value, names: &mut BTreeSet<String>) {
    use serde_yaml_ng::Value;

    match value {
        Value::String(value) => {
            if let Some(path) = value.strip_prefix("cowork://skills/")
                && let Some(name) = path.split('/').next()
                && !name.is_empty()
            {
                names.insert(name.to_owned());
            }
        }
        Value::Sequence(items) => {
            for item in items {
                collect_cowork_skill_names(item, names);
            }
        }
        Value::Mapping(entries) => {
            for (key, item) in entries {
                collect_cowork_skill_names(key, names);
                collect_cowork_skill_names(item, names);
            }
        }
        Value::Tagged(tagged) => collect_cowork_skill_names(&tagged.value, names),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::infra::env::MapEnv;
    use crate::infra::exec::MockExecutor;
    use crate::infra::platform::{Os, Platform};

    use super::*;
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
            "deployments:\n- cowork://skills/example/SKILL.md\n",
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
            "deployments: []\n",
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
