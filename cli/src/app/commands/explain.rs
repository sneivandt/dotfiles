//! Read-only explanation of a configured home symlink.
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};

use crate::app::cli::ExplainOpts;
use crate::domains::files::{config::symlinks as config, symlinks};
use crate::engine::plan::{ApplyOperation, RemoveOperation};
use crate::engine::{IntrinsicState as _, ProcessOpts, RemovableResource as _};
use crate::infra::exec::{Executor, ProcessExecutor};
use crate::infra::fs::resolve_target_entry;
use crate::infra::platform::Platform;

pub(crate) fn run(opts: &ExplainOpts) -> Result<()> {
    let env = crate::infra::env::system();
    let platform = Platform::detect();
    let config = super::list::load_discovery_config(&opts.repository, env.as_ref(), platform)?;
    let home = crate::engine::context::home_directory(env.as_ref(), platform)?;
    let requested = if opts.path.is_absolute() {
        opts.path.clone()
    } else {
        home.join(opts.path.strip_prefix(Path::new("~")).unwrap_or(&opts.path))
    };
    let executor: Arc<dyn Executor> = Arc::new(ProcessExecutor::system());
    let resources = symlinks::resources_for(
        &config.root,
        config.overlay.as_deref(),
        &home,
        platform,
        &executor,
        &config.symlinks,
    );
    symlinks::validate_resolved_targets(&resources)?;
    let identity = config::path_identity(&requested);
    let resolved_identity = resolve_target_entry(&requested)
        .map(|path| config::path_identity(&path))
        .ok();
    let (entry, resource) = config
        .symlinks
        .iter()
        .zip(resources)
        .find(|(_, resource)| {
            config::path_identity(&resource.target) == identity
                || resource.checked_target().is_ok_and(|path| {
                    resolved_identity.as_ref().is_some_and(|requested_path| {
                        config::path_identity(&path) == *requested_path
                    })
                })
        })
        .with_context(|| format!("no active symlink manages {}", requested.display()))?;

    let origin = entry.origin.as_deref().unwrap_or(&config.root);
    let section =
        config::declaring_section(entry, &config.root, &config.profile.active_categories)?
            .context("could not identify the declaring symlink section")?;
    let mut output = std::io::stdout().lock();
    writeln!(output, "Profile: {}", config.profile.name)?;
    if let Some(overlay) = config.overlay.as_deref() {
        writeln!(output, "Active overlay: {}", overlay.display())?;
    }
    let declaration: PathBuf = origin.join("conf").join(config::SYMLINKS_TOML);
    writeln!(output, "Declaration: {} [{section}]", declaration.display())?;
    writeln!(output, "Source: {}", resource.source.display())?;
    writeln!(output, "Target: {}", resource.target.display())?;
    match resource.checked_target() {
        Ok(path) => writeln!(output, "Resolved target: {}", path.display())?,
        Err(error) => writeln!(output, "Target blocked: {error:#}")?,
    }
    let update_state = resource.current_state()?;
    let update_action =
        match ApplyOperation::from_state(&update_state, &ProcessOpts::strict("link")) {
            ApplyOperation::Apply { .. } => "create managed link".into(),
            ApplyOperation::Noop => "already linked".into(),
            ApplyOperation::Skip { reason, .. } => format!("blocked ({reason})"),
        };
    writeln!(output, "Update: {update_action}")?;
    let removal_action = match resource.removal_state() {
        Ok(state) => match RemoveOperation::from_state(&state, "materialize") {
            RemoveOperation::Remove { .. } => "materialize managed link".into(),
            RemoveOperation::Noop => "leave target as it is".into(),
            RemoveOperation::Skip { reason } => format!("blocked ({reason})"),
        },
        Err(error) => format!("blocked ({error:#})"),
    };
    writeln!(output, "Remove: {removal_action}")?;
    Ok(())
}
