//! Tasks: install and uninstall symlinks.
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

use crate::domains::files::config::symlinks::{Symlink, default_target, validate_target_paths};
use crate::domains::files::resources::symlink::SymlinkResource;
use crate::engine::{
    Context, IntrinsicState as _, ProcessOpts, Resource as _, ResourceState, Task, TaskMeta,
    TaskResult, process_resources_remove, run_resource_task, task_metadata,
};
use crate::infra::logging::OutputExt as _;

/// Build a single [`SymlinkResource`] from a config entry.
fn build_resource(
    s: &Symlink,
    repo_root: &Path,
    home: &Path,
    executor: &Arc<dyn crate::infra::exec::Executor>,
    placeholders: Option<&mut GitSymlinkPlaceholders>,
) -> SymlinkResource {
    let symlinks_dir = crate::domains::files::config::symlinks::resolve_symlinks_dir(s, repo_root);
    let source = symlinks_dir.join(&s.source);
    let target = s.target.as_ref().map_or_else(
        || compute_target(home, &s.source),
        |explicit| home.join(explicit),
    );
    let source_root = s.origin.as_deref().unwrap_or(repo_root);
    let validation_error = crate::domains::files::config::symlinks::validate_paths(s)
        .err()
        .map(|e| e.to_string())
        .or_else(|| placeholders.and_then(|cache| cache.reason(&source, source_root)));
    let legacy_source = legacy_editor_source(s, repo_root).map(|path| symlinks_dir.join(path));
    SymlinkResource::new(source, target, Arc::clone(executor))
        .with_source_directory(symlinks_dir)
        .with_legacy_source(legacy_source)
        .with_display_roots(home, source_root)
        .with_external_target_allowed(s.allow_external_target)
        .with_validation_error(validation_error)
}

/// Forwarding entries retired by 3c39f434. Match both ends of the
/// public declaration so an overlay or another target gains no ownership.
fn legacy_editor_source<'a>(s: &'a Symlink, root: &Path) -> Option<&'a str> {
    if s.origin.as_deref().is_some_and(|origin| origin != root) {
        return None;
    }
    match (s.source.as_str(), s.target.as_deref()?) {
        ("vim", ".config/nvim") => Some("config/nvim"),
        ("config/Code/User/keybindings.json", ".config/Code - Insiders/User/keybindings.json") => {
            Some("config/Code - Insiders/User/keybindings.json")
        }
        ("config/Code/User/settings.json", ".config/Code - Insiders/User/settings.json") => {
            Some("config/Code - Insiders/User/settings.json")
        }
        (
            "config/Code/User/keybindings.json",
            target @ "AppData/Roaming/Code - Insiders/User/keybindings.json",
        )
        | (
            "config/Code/User/settings.json",
            target @ "AppData/Roaming/Code - Insiders/User/settings.json",
        ) => Some(target),
        _ => None,
    }
}

/// Build [`SymlinkResource`] instances from a symlink configuration slice.
pub(crate) fn build_resources(ctx: &Context, symlinks: &[Symlink]) -> Vec<SymlinkResource> {
    resources_for(
        ctx.root(),
        ctx.overlay(),
        ctx.home(),
        ctx.platform(),
        &ctx.executor_arc(),
        symlinks,
    )
}

pub(crate) fn resources_for(
    root: &Path,
    overlay: Option<&Path>,
    home: &Path,
    platform: crate::infra::platform::Platform,
    executor: &Arc<dyn crate::infra::exec::Executor>,
    symlinks: &[Symlink],
) -> Vec<SymlinkResource> {
    let mut placeholders = platform.is_windows().then(GitSymlinkPlaceholders::default);
    symlinks
        .iter()
        .map(|entry| {
            let other_root = if entry.origin.as_deref().is_some_and(|origin| origin != root) {
                Some(root)
            } else {
                overlay
            };
            build_resource(entry, root, home, executor, placeholders.as_mut())
                .with_other_checkout(other_root.map(Path::to_path_buf))
        })
        .collect()
}

fn validate_resolved_targets(resources: &[SymlinkResource]) -> Result<()> {
    validate_observed_targets(resources.iter().map(|r| (r, r.checked_target().ok())))
}

pub(crate) fn validate_observed_targets<'a>(
    targets: impl Iterator<Item = (&'a SymlinkResource, Option<std::path::PathBuf>)>,
) -> Result<()> {
    // Resource discovery reports individual paths that cannot be resolved.
    validate_target_paths(
        targets.filter_map(|(resource, path)| path.map(|path| (path, resource.description()))),
    )
}

pub(crate) fn check_install_config_home(
    symlinks: &[Symlink],
    home: &Path,
    env: &dyn crate::infra::env::Env,
) -> Result<()> {
    if let Some(diagnostic) =
        crate::domains::files::config::symlinks::config_home_diagnostic(symlinks, home, env)
    {
        anyhow::bail!("{}: {}", diagnostic.code, diagnostic.message);
    }
    Ok(())
}

crate::engine::config_task! {
    /// Create symlinks from symlinks/ to $HOME.
    pub struct InstallSymlinks(Vec<Symlink>);
}

const INSTALL_NAME: &str = "Home symlinks";

impl Task for InstallSymlinks {
    task_metadata! {
        name: INSTALL_NAME,
        selector: "symlinks",
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let symlinks = self.config.get();
        check_install_config_home(symlinks, ctx.home(), ctx.env().as_ref())?;
        let resources = build_resources(ctx, symlinks);
        validate_resolved_targets(&resources)?;
        // UAC can switch accounts. Home mutations stay in the parent, and the
        // capability is re-probed after the ordering-only Developer Mode task.
        install_resources(ctx, resources, ctx.can_create_symlinks())
    }
}

fn install_resources(
    ctx: &Context,
    resources: Vec<SymlinkResource>,
    can_create_symlinks: bool,
) -> Result<TaskResult> {
    if ctx.dry_run() || can_create_symlinks {
        return run_resource_task(ctx, resources, &ProcessOpts::strict("link"));
    }

    let (unavailable, runnable): (Vec<_>, Vec<_>) = resources.into_iter().partition(|resource| {
        resource.source.is_file()
            && matches!(
                resource.current_state(),
                Ok(ResourceState::Missing | ResourceState::Incorrect { .. })
            )
    });
    let result = run_resource_task(ctx, runnable, &ProcessOpts::strict("link"))?;
    if unavailable.is_empty() {
        return Ok(result);
    }
    let reason = format!(
        "{} file link(s) require Developer Mode in the original user's session; home symlinks are never delegated through UAC",
        unavailable.len()
    );
    ctx.log().warn(&reason);
    if matches!(&result, TaskResult::Batch(stats) if stats.failed_count() > 0) {
        return Ok(result);
    }
    let detail = if let TaskResult::Batch(stats) = result {
        format!("{}; {reason}", stats.summary(false))
    } else {
        reason
    };
    Ok(TaskResult::unmet(detail))
}

/// Materialize installed symlinks into real files and directories.
#[derive(Debug)]
pub struct UninstallSymlinks {
    config: crate::infra::ConfigHandle<Vec<Symlink>>,
    restore_missing: bool,
}

impl UninstallSymlinks {
    /// Materialize only existing managed links.
    pub const fn new(config: crate::infra::ConfigHandle<Vec<Symlink>>) -> Self {
        Self::with_restore_missing(config, false)
    }

    /// Select whether missing targets also receive copies.
    pub const fn with_restore_missing(
        config: crate::infra::ConfigHandle<Vec<Symlink>>,
        restore_missing: bool,
    ) -> Self {
        Self {
            config,
            restore_missing,
        }
    }
}

impl Task for UninstallSymlinks {
    fn meta(&self) -> TaskMeta<'_> {
        TaskMeta::new("Home symlinks").with_selector("symlinks")
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        !self.config.get().is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let symlinks = self.config.get();
        let resources: Vec<_> = build_resources(ctx, symlinks)
            .into_iter()
            .map(|resource| resource.with_restore_missing(self.restore_missing))
            .collect();
        validate_resolved_targets(&resources)?;
        process_resources_remove(ctx, resources, "copy")
    }
}

/// Compute the default target path in $HOME for a symlink source.
///
/// Symlink sources like "bashrc" map to "$HOME/.bashrc".
/// Sources like "config/git/config" map to "$HOME/.config/git/config".
///
/// When a non-standard target path is required (e.g. Windows paths under
/// `AppData/` or `Documents/`), use an explicit `target` field in
/// `conf/symlinks.toml` rather than relying on naming conventions.
fn compute_target(home: &Path, source: &str) -> std::path::PathBuf {
    home.join(default_target(source))
}

/// One index snapshot per owning repository and resource-building pass.
/// Failed opens are cached too, preserving the best-effort placeholder check.
#[derive(Default)]
struct GitSymlinkPlaceholders {
    indexes: std::collections::HashMap<std::path::PathBuf, Option<git2::Index>>,
}

impl GitSymlinkPlaceholders {
    fn reason(&mut self, source: &Path, repo_root: &Path) -> Option<String> {
        let metadata = std::fs::symlink_metadata(source).ok()?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return None;
        }

        let relative_source = source.strip_prefix(repo_root).ok()?;
        let index = self
            .indexes
            .entry(repo_root.to_path_buf())
            .or_insert_with(|| git2::Repository::open(repo_root).ok()?.index().ok())
            .as_ref()?;
        let entry = index.get_path(relative_source, 0)?;
        let link_mode = u32::try_from(i32::from(git2::FileMode::Link)).ok()?;

        (entry.mode == link_mode).then(|| {
            format!(
                "source is checked out as a plain file but Git records it as a symlink: {} \
                 (enable symlink support and restore the checkout)",
                source.display()
            )
        })
    }
}

#[cfg(test)]
#[path = "tests/symlinks.rs"]
mod tests;
