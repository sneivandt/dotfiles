use super::ownership::{read_file_inventory, read_owned_skills, write_owned_skills};
use super::{
    BTreeMap, BTreeSet, COWORK_TARGET, COWORK_URI_PREFIX, Context, FILE_OWNERSHIP_FILE,
    FileInventory, OwnedFile, Path, Result, Value, copilot_cowork_skills_path,
    reject_destination_link, symlink_metadata_optional, valid_skill_name, write_atomic,
};
use anyhow::Context as _;

/// Remove records left by direct APM Cowork installs.
///
/// Dotfiles owns the ACL-sensitive Cowork copy. Leaving `cowork://` records in
/// APM's ledger makes later installs retry directory replacement.
pub(in crate::domains::ai::apm) fn remove_legacy_cowork_lock_deployments(
    ctx: &Context,
) -> Result<bool> {
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

pub(super) fn legacy_entry_point_fingerprints(lock: &Value) -> BTreeMap<String, Option<OwnedFile>> {
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

pub(super) fn decode_sha256_hex(hash: &str) -> Option<[u8; 32]> {
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = [0; 32];
    for (pair, byte) in hash.as_bytes().as_chunks::<2>().0.iter().zip(&mut bytes) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(bytes)
}

pub(super) fn strip_legacy_cowork_deployments(
    lock: &mut Value,
    names: &mut BTreeSet<String>,
) -> bool {
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
    let text = include_str!("../../../../../tests/fixtures/apm-0.33.0-lock.yaml");
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

pub(super) fn record_legacy_cowork_deployment(value: &Value, names: &mut BTreeSet<String>) -> bool {
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

pub(super) fn check_legacy_entry_point(target_skill: &Path) -> Result<()> {
    reject_destination_link(target_skill)?;
    let entry_point = target_skill.join("SKILL.md");
    anyhow::ensure!(
        symlink_metadata_optional(&entry_point, "inspect legacy Cowork entry point")?.is_none(),
        "preserving legacy Cowork file {} without a verified fingerprint; move it aside and retry",
        entry_point.display()
    );
    Ok(())
}
