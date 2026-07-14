//! APM lockfile scoping for dotfiles-managed Copilot App workflows.

use std::collections::BTreeSet;
use std::io::ErrorKind;

use crate::engine::Context;

/// Lockfile URI prefix under which APM records a deployed Copilot App workflow.
///
/// Each `deployed_files` entry of this shape encodes the workflow's database
/// primary key after the prefix, i.e.
/// `copilot-app-db://workflows/apm--<owner>--<pkg>--<prompt>`.
const COPILOT_APP_WORKFLOW_URI_PREFIX: &str = "copilot-app-db://workflows/";

/// Read the workflow ids this dotfiles install deployed from
/// `~/.apm/apm.lock.yaml`.
///
/// Returns `None` when the lockfile is absent or cannot be read (treated like a
/// first install / nothing-to-do), and `Some(set)` -- possibly empty -- when it
/// was parsed. Only `deployed_files` entries under the
/// [`COPILOT_APP_WORKFLOW_URI_PREFIX`] count; agents, skills, and other
/// primitives are ignored. Best-effort: a malformed lockfile yields an empty
/// set rather than an error so the fixup simply does nothing.
pub(super) fn read_deployed_workflow_ids(ctx: &Context) -> Option<BTreeSet<String>> {
    let lock = ctx.home().join(".apm").join("apm.lock.yaml");
    let content = match std::fs::read_to_string(&lock) {
        Ok(content) => content,
        Err(e) => {
            if e.kind() != ErrorKind::NotFound {
                ctx.debug_fmt(|| {
                    format!(
                        "autopilot scope: cannot read {} (treating as no workflows): {e}",
                        lock.display()
                    )
                });
            }
            return None;
        }
    };
    Some(parse_deployed_workflow_ids(&content))
}

/// Extract the dotfiles-deployed workflow ids from APM lockfile text.
///
/// Walks `dependencies[*].deployed_files[*]` and collects every entry that
/// starts with [`COPILOT_APP_WORKFLOW_URI_PREFIX`], stripped to the bare
/// workflow id. Any parse failure or unexpected shape yields an empty set.
pub(super) fn parse_deployed_workflow_ids(lockfile: &str) -> BTreeSet<String> {
    use serde_yaml_ng::Value;

    let mut ids = BTreeSet::new();
    let Ok(value) = serde_yaml_ng::from_str::<Value>(lockfile) else {
        return ids;
    };
    let Some(deps) = value.get("dependencies").and_then(Value::as_sequence) else {
        return ids;
    };
    for dep in deps {
        let Some(files) = dep.get("deployed_files").and_then(Value::as_sequence) else {
            continue;
        };
        for file in files {
            if let Some(id) = file
                .as_str()
                .and_then(|s| s.strip_prefix(COPILOT_APP_WORKFLOW_URI_PREFIX))
                && !id.is_empty()
            {
                ids.insert(id.to_owned());
            }
        }
    }
    ids
}
