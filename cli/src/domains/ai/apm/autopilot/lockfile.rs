//! APM lockfile scoping for dotfiles-managed Copilot App workflows.

use anyhow::{Context as _, Result};
use std::collections::BTreeSet;
use std::io::ErrorKind;

use crate::engine::Context;

/// Lockfile URI prefix under which APM records a deployed Copilot App workflow.
///
/// Each `deployed_files` entry of this shape encodes the workflow's database
/// primary key after the prefix, i.e.
/// `copilot-app-db://workflows/apm--<owner>--<pkg>--<prompt>`.
pub(super) const COPILOT_APP_WORKFLOW_URI_PREFIX: &str = "copilot-app-db://workflows/";

/// Read the workflow ids this dotfiles install deployed from
/// `~/.apm/apm.lock.yaml`.
///
/// Returns `None` only when the lockfile is absent, and `Some(set)` -- possibly
/// empty -- when it was parsed. Read errors and malformed lockfiles fail
/// explicitly. Only `deployed_files` entries under the
/// [`COPILOT_APP_WORKFLOW_URI_PREFIX`] count; agents, skills, and other
/// primitives are ignored.
pub(super) fn read_deployed_workflow_ids(ctx: &Context) -> Result<Option<BTreeSet<String>>> {
    let lock = ctx.home().join(".apm").join("apm.lock.yaml");
    let content = match std::fs::read_to_string(&lock) {
        Ok(content) => content,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(e)
                .with_context(|| format!("reading workflow ownership {}", lock.display()));
        }
    };
    parse_deployed_workflow_ids(&content)
        .map(Some)
        .with_context(|| format!("reading workflow ownership {}", lock.display()))
}

/// Extract the dotfiles-deployed workflow ids from APM lockfile text.
///
/// Walks `dependencies[*].deployed_files[*]` and collects every entry that
/// starts with [`COPILOT_APP_WORKFLOW_URI_PREFIX`], stripped to the bare
/// workflow id. Reject malformed ownership rather than inferring no deployments.
pub(super) fn parse_deployed_workflow_ids(lockfile: &str) -> Result<BTreeSet<String>> {
    use crate::infra::yaml::Value;

    let mut ids = BTreeSet::new();
    let value: Value =
        crate::infra::yaml::from_str(lockfile).context("parsing workflow ownership")?;
    let root = value
        .as_object()
        .context("workflow ownership must be a mapping")?;
    let Some(deps) = root.get("dependencies") else {
        return Ok(ids);
    };
    for dep in deps
        .as_array()
        .context("workflow dependencies must be a sequence")?
    {
        let dep = dep
            .as_object()
            .context("workflow dependency must be a mapping")?;
        let Some(files) = dep.get("deployed_files") else {
            continue;
        };
        for file in files
            .as_array()
            .context("deployed_files must be a sequence")?
        {
            let file = file
                .as_str()
                .context("deployed_files entry must be a string")?;
            if let Some(id) = file.strip_prefix(COPILOT_APP_WORKFLOW_URI_PREFIX) {
                anyhow::ensure!(!id.is_empty(), "empty deployed workflow id");
                ids.insert(id.to_owned());
            }
        }
    }
    Ok(ids)
}
