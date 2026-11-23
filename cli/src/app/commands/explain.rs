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
    let identity = config::path_identity(&requested);
    let resolved_identity = resolve_target_entry(&requested)
        .map(|path| config::path_identity(&path))
        .ok();
    let mut matched = None;
    // Compare the same observation used to check collisions. A later collision
    // must still fail the command, even after its requested target was found.
    symlinks::validate_observed_targets(config.symlinks.iter().zip(&resources).map(
        |(entry, resource)| {
            let path = resource.checked_target().ok();
            if matched.is_none()
                && (config::path_identity(&resource.target) == identity
                    || path.as_ref().zip(resolved_identity.as_ref()).is_some_and(
                        |(path, requested_path)| config::path_identity(path) == *requested_path,
                    ))
            {
                matched = Some((entry, resource));
            }
            (resource, path)
        },
    ))?;
    let (entry, resource) =
        matched.with_context(|| format!("no active symlink manages {}", requested.display()))?;

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
    let update_action = if let Err(error) =
        symlinks::check_install_config_home(&config.symlinks, &home, env.as_ref())
    {
        format!("blocked ({error})")
    } else {
        format_update_action(resource.current_state())
    };
    writeln!(output, "Update: {update_action}")?;
    let removal_action = match resource.removal_state() {
        Ok(state) => match RemoveOperation::from_state(&state, "copy") {
            RemoveOperation::Remove { .. } => "replace link with an independent copy".into(),
            RemoveOperation::Noop => "leave target as it is".into(),
            RemoveOperation::Skip { reason } => format!("blocked ({reason})"),
        },
        Err(error) => format!("blocked ({error:#})"),
    };
    writeln!(output, "Remove: {removal_action}")?;
    Ok(())
}

fn format_update_action(
    state: crate::engine::ResourceResult<crate::engine::ResourceState>,
) -> String {
    match state {
        Ok(state) => match ApplyOperation::from_state(&state, &ProcessOpts::strict("link")) {
            ApplyOperation::Apply { .. } => "create managed link".into(),
            ApplyOperation::Noop => "already linked".into(),
            ApplyOperation::Skip { reason, .. } => format!("blocked ({reason})"),
        },
        Err(error) => format!("blocked ({error:#})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::ResourceState;

    #[test]
    fn update_action_reports_state_errors_as_blocked() {
        let error = anyhow::anyhow!("cannot inspect target").into();
        assert_eq!(
            format_update_action(Err(error)),
            "blocked (cannot inspect target)"
        );
        assert_eq!(
            format_update_action(Ok(ResourceState::Correct)),
            "already linked"
        );
        assert_eq!(
            format_update_action(Ok(ResourceState::Missing)),
            "create managed link"
        );
    }
}
