//! Read-only explanation of a configured home symlink.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};

use crate::app::cli::ExplainOpts;
use crate::domains::files::{config::symlinks as config, symlinks};
use crate::engine::plan::{ApplyOperation, RemoveOperation};
use crate::engine::{IntrinsicState as _, ProcessOpts, RemovableResource as _};
use crate::infra::exec::{Executor, ProcessExecutor};
use crate::infra::fs::resolve_target_entry;
use crate::infra::logging::{ColorMode, StyleChoice, TextStyle, stdout_style_for};
use crate::infra::platform::Platform;

pub(crate) fn run(opts: &ExplainOpts, color: ColorMode) -> Result<()> {
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
    let declaration: PathBuf = origin.join("conf").join(config::SYMLINKS_TOML);
    let target_status = match resource.checked_target() {
        Ok(path) => ("Resolved target:", path.display().to_string(), None),
        Err(error) => (
            "Target blocked:",
            format!("{error:#}"),
            Some(TextStyle::Yellow),
        ),
    };
    let update_action = if let Err(error) =
        symlinks::check_install_config_home(&config.symlinks, &home, env.as_ref())
    {
        (format!("blocked ({error})"), TextStyle::Yellow)
    } else {
        format_update_action(resource.current_state())
    };
    let removal_action = match resource.removal_state() {
        Ok(state) => match RemoveOperation::from_state(&state, "copy") {
            RemoveOperation::Remove { .. } => (
                "replace link with an independent copy".into(),
                TextStyle::Cyan,
            ),
            RemoveOperation::Noop => ("leave target as it is".into(), TextStyle::Dim),
            RemoveOperation::Skip { reason } => (format!("blocked ({reason})"), TextStyle::Yellow),
        },
        Err(error) => (format!("blocked ({error:#})"), TextStyle::Yellow),
    };
    let style = stdout_style_for(color);
    let mut rows: Vec<(&str, String)> = Vec::with_capacity(8);
    rows.push(("Profile:", style.bold(&config.profile.name)));
    if let Some(overlay) = config.overlay.as_deref() {
        rows.push(("Active overlay:", overlay.display().to_string()));
    }
    rows.push((
        "Declaration:",
        format!(
            "{} {}",
            declaration.display(),
            style.paint(TextStyle::Cyan, &format!("[{section}]"))
        ),
    ));
    rows.push(("Source:", resource.source.display().to_string()));
    rows.push(("Target:", resource.target.display().to_string()));
    rows.push((
        target_status.0,
        target_status.2.map_or_else(
            || target_status.1.clone(),
            |tone| style.paint(tone, &target_status.1),
        ),
    ));
    rows.push(("Update:", style.paint(update_action.1, &update_action.0)));
    rows.push(("Remove:", style.paint(removal_action.1, &removal_action.0)));

    let mut output = std::io::stdout().lock();
    write_explanation_rows(&rows, style, &mut output)
}

fn write_explanation_rows(
    rows: &[(&str, String)],
    style: StyleChoice,
    out: &mut dyn std::io::Write,
) -> Result<()> {
    let label_width = rows.iter().map(|(label, _)| label.len()).max().unwrap_or(0);
    for (label, value) in rows {
        let pad = label_width - label.len();
        writeln!(
            out,
            "{}{:pad$}  {value}",
            style.paint(TextStyle::Dim, label),
            ""
        )?;
    }
    Ok(())
}

fn format_update_action(
    state: crate::engine::ResourceResult<crate::engine::ResourceState>,
) -> (String, TextStyle) {
    match state {
        Ok(state) => match ApplyOperation::from_state(&state, &ProcessOpts::strict("link")) {
            ApplyOperation::Apply { .. } => ("create managed link".into(), TextStyle::Cyan),
            ApplyOperation::Noop => ("already linked".into(), TextStyle::Green),
            ApplyOperation::Skip { reason, .. } => {
                (format!("blocked ({reason})"), TextStyle::Yellow)
            }
        },
        Err(error) => (format!("blocked ({error:#})"), TextStyle::Yellow),
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
            ("blocked (cannot inspect target)".into(), TextStyle::Yellow)
        );
        assert_eq!(
            format_update_action(Ok(ResourceState::Correct)),
            ("already linked".into(), TextStyle::Green)
        );
        assert_eq!(
            format_update_action(Ok(ResourceState::Missing)),
            ("create managed link".into(), TextStyle::Cyan)
        );
    }

    #[test]
    fn explanation_rows_align_labels_in_plain_and_colored_modes() {
        let rows = [
            ("Profile:", "desktop".to_string()),
            ("Resolved target:", "/home/user/.bashrc".to_string()),
        ];
        let mut plain = Vec::new();
        write_explanation_rows(&rows, StyleChoice::plain(), &mut plain).unwrap();
        assert_eq!(
            String::from_utf8(plain).unwrap(),
            "Profile:          desktop\nResolved target:  /home/user/.bashrc\n"
        );
        let mut colored = Vec::new();
        write_explanation_rows(&rows, StyleChoice::colored(), &mut colored).unwrap();
        assert_eq!(
            String::from_utf8(colored).unwrap(),
            "\x1b[2mProfile:\x1b[0m          desktop\n\x1b[2mResolved target:\x1b[0m  /home/user/.bashrc\n"
        );
    }
}
