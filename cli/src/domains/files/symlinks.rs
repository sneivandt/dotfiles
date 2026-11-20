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

pub(crate) fn validate_resolved_targets(resources: &[SymlinkResource]) -> Result<()> {
    validate_target_paths(resources.iter().filter_map(|resource| {
        // Resource discovery reports individual paths that cannot be resolved.
        resource
            .checked_target()
            .ok()
            .map(|path| (path, resource.description()))
    }))
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
mod tests {
    use super::*;
    use crate::domains::files::config::symlinks::Symlink;
    use crate::engine::{RemovableResource as _, ResourceChange};
    use crate::infra::ConfigHandle;
    use crate::test_helpers::make_linux_context;
    use crate::test_helpers::symlink;
    use std::path::PathBuf;

    fn handle(symlinks: Vec<Symlink>) -> ConfigHandle<Vec<Symlink>> {
        ConfigHandle::new(symlinks)
    }

    #[test]
    fn custom_config_home_rejects_install_and_preview_before_writing() {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().join("home");
        let ctx = make_linux_context(fixture.path().into(), None)
            .with_home(home.clone())
            .with_env(
                crate::infra::env::MapEnv::new()
                    .with("XDG_CONFIG_HOME", fixture.path().join("custom-config"))
                    .into_handle(),
            );
        for target in [None, Some("./.config/git/config")] {
            let task = InstallSymlinks::new(handle(vec![symlink("config/git/config", target)]));
            for dry_run in [true, false] {
                let error = task.run(&ctx.with_dry_run(dry_run)).unwrap_err();
                assert!(
                    error
                        .to_string()
                        .contains("symlink.unsupported-config-home")
                );
                assert!(!home.exists());
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn file_capability_is_checked_at_execution_without_delegating_home_links() {
        use crate::engine::SkipKind;
        use crate::test_helpers::make_windows_context;

        for (name, capability, dry_run, file_created) in [
            ("Developer Mode enabled by predecessor", true, false, true),
            ("Developer Mode unavailable", false, false, false),
            ("preview without Developer Mode", false, true, false),
        ] {
            let fixture = tempfile::tempdir_in(".").unwrap();
            let root = crate::infra::fs::canonicalize(fixture.path()).unwrap();
            let home = root.join("original-user");
            let admin_home = root.join("administrator");
            std::fs::create_dir_all(root.join("symlinks/directory")).unwrap();
            std::fs::create_dir(&home).unwrap();
            std::fs::create_dir(&admin_home).unwrap();
            std::fs::write(root.join("symlinks/file"), "file").unwrap();
            let config = handle(
                ["file", "directory"]
                    .map(|source| Symlink {
                        source: source.into(),
                        target: None,
                        allow_external_target: false,
                        origin: None,
                    })
                    .to_vec(),
            );
            let task = InstallSymlinks::new(config);
            let ctx = make_windows_context(root, None)
                .with_home(home.clone())
                .with_dry_run(dry_run);
            assert!(!task.needs_elevation(&ctx), "{name}");
            assert!(
                !task.assess(&ctx).requires_elevation(),
                "{name}: phase assessment must not cache missing file capability"
            );
            let result =
                install_resources(&ctx, build_resources(&ctx, task.config.get()), capability)
                    .unwrap();
            assert_eq!(home.join(".file").exists(), file_created, "{name}");
            assert_eq!(home.join(".directory").exists(), !dry_run, "{name}");
            assert_eq!(
                std::fs::read_dir(admin_home).unwrap().count(),
                0,
                "{name}: a different administrator's home must remain untouched"
            );
            if !capability && !dry_run {
                assert!(
                    matches!(
                        result,
                        TaskResult::Skipped {
                            kind: SkipKind::UnmetWork,
                            ..
                        }
                    ),
                    "{name}: pending file work must block dependents"
                );
            } else {
                assert!(matches!(result, TaskResult::Batch(_)), "{name}");
            }
            if file_created {
                let repeated =
                    install_resources(&ctx, build_resources(&ctx, task.config.get()), false)
                        .unwrap();
                let TaskResult::Batch(stats) = repeated else {
                    panic!("converged links must remain runnable")
                };
                assert_eq!(stats.already_ok_count(), 2);
                assert_eq!(stats.changed_count(), 0);
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn unavailable_file_links_do_not_hide_other_resource_failures() {
        use crate::test_helpers::make_windows_context;

        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = crate::infra::fs::canonicalize(fixture.path()).unwrap();
        let home = root.join("home");
        std::fs::create_dir_all(root.join("symlinks")).unwrap();
        std::fs::create_dir(&home).unwrap();
        std::fs::write(root.join("symlinks/file"), "managed file").unwrap();
        let ctx = make_windows_context(root, None).with_home(home.clone());
        let config = [symlink("file", None), symlink("absent", None)];
        let result = install_resources(&ctx, build_resources(&ctx, &config), false).unwrap();
        let TaskResult::Batch(stats) = result else {
            panic!("an invalid source must remain a failed batch, not an elevation skip");
        };
        assert_eq!(stats.failed_count(), 1);
        assert!(!home.join(".file").exists());
    }

    // ------------------------------------------------------------------
    // compute_target
    // ------------------------------------------------------------------

    #[test]
    fn target_for_bashrc() {
        let home = PathBuf::from("/home/user");
        let target = compute_target(&home, "bashrc");
        assert_eq!(target, PathBuf::from("/home/user/.bashrc"));
    }

    #[test]
    fn target_for_config_subpath() {
        let home = PathBuf::from("/home/user");
        let target = compute_target(&home, "config/git/config");
        assert_eq!(target, PathBuf::from("/home/user/.config/git/config"));
    }

    #[test]
    fn default_target_normalizes_current_directory_components() {
        let home = PathBuf::from("/home/user");
        for (source, expected) in [
            ("./bashrc", ".bashrc"),
            ("././bashrc", ".bashrc"),
            ("./config/./git//config", ".config/git/config"),
            ("config//git/config", ".config/git/config"),
            (".hidden", "..hidden"),
        ] {
            let target = compute_target(&home, source);
            assert_eq!(target, home.join(expected), "{source}");
            assert!(
                !target
                    .components()
                    .any(|component| component == std::path::Component::ParentDir),
                "{source}: {target:?}"
            );
        }
    }

    #[test]
    fn default_target_uses_native_separator_semantics() {
        let home = PathBuf::from("/home/user");
        let expected = if cfg!(windows) {
            ".config/git/config"
        } else {
            r"..\config\.\git\config"
        };
        assert_eq!(
            compute_target(&home, r".\config\.\git\config"),
            home.join(expected)
        );
    }

    #[test]
    fn resource_with_dot_prefixed_source_stays_inside_home() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("repo");
        let home = fixture.path().join("home");
        std::fs::create_dir_all(root.join("symlinks")).unwrap();
        std::fs::create_dir(&home).unwrap();
        std::fs::write(root.join("symlinks/bashrc"), "source").unwrap();
        let outside = fixture.path().join("bashrc");
        std::fs::write(&outside, "must survive").unwrap();
        let ctx = make_linux_context(root.clone(), None).with_home(home.clone());
        let resource = build_resource(
            &symlink("./bashrc", None),
            &root,
            &home,
            &ctx.executor_arc(),
            Some(&mut GitSymlinkPlaceholders::default()),
        );

        assert_eq!(resource.target, home.join(".bashrc"));
        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
        assert_eq!(
            resource.with_restore_missing(true).remove().unwrap(),
            ResourceChange::Applied
        );
        assert_eq!(
            std::fs::read_to_string(home.join(".bashrc")).unwrap(),
            "source"
        );
        assert_eq!(std::fs::read_to_string(outside).unwrap(), "must survive");
    }

    #[test]
    fn overlay_resource_description_uses_overlay_relative_source() {
        let root = PathBuf::from("/repo");
        let overlay = PathBuf::from("/private/overlay");
        let home = PathBuf::from("/home/user");
        let symlink = Symlink {
            source: "config/example".to_string(),
            target: None,
            allow_external_target: false,
            origin: Some(overlay),
        };
        let ctx = make_linux_context(root.clone(), None).with_home(home);
        let resource = build_resource(
            &symlink,
            &root,
            ctx.home(),
            &ctx.executor_arc(),
            Some(&mut GitSymlinkPlaceholders::default()),
        );

        assert_eq!(
            resource.description(),
            "~/.config/example \u{2192} symlinks/config/example"
        );
    }

    #[test]
    fn git_symlink_placeholders_are_rejected_in_main_and_overlay() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = fixture.path().join("repo");
        let overlay = fixture.path().join("overlay");
        let home = fixture.path().join("home");
        std::fs::create_dir(&home).unwrap();
        for source_root in [&root, &overlay] {
            let repository = git2::Repository::init(source_root).unwrap();
            let sources = source_root.join("symlinks");
            std::fs::create_dir(&sources).unwrap();
            std::fs::write(sources.join("placeholder"), "real-config").unwrap();
            std::fs::write(sources.join("real-config"), "actual configuration").unwrap();
            let mut index = repository.index().unwrap();
            let path = Path::new("symlinks").join("placeholder");
            index.add_path(&path).unwrap();
            let mut entry = index.get_path(&path, 0).unwrap();
            entry.mode = 0o120_000;
            index.add(&entry).unwrap();
            index.write().unwrap();
        }
        let ctx = make_linux_context(root.clone(), None).with_home(home.clone());
        for origin in [None, Some(overlay)] {
            let symlink = Symlink {
                source: "placeholder".into(),
                target: Some(".configuration".into()),
                allow_external_target: false,
                origin,
            };
            let resource = build_resource(
                &symlink,
                &root,
                &home,
                &ctx.executor_arc(),
                Some(&mut GitSymlinkPlaceholders::default()),
            );
            assert!(
                matches!(
                    resource.current_state().unwrap(),
                    ResourceState::Invalid { reason } if reason.contains("Git records it as a symlink")
                ),
                "{symlink:?}"
            );
            assert!(!home.join(".configuration").exists());

            let ordinary = Symlink {
                source: "real-config".into(),
                ..symlink
            };
            assert_eq!(
                build_resource(
                    &ordinary,
                    &root,
                    &home,
                    &ctx.executor_arc(),
                    Some(&mut GitSymlinkPlaceholders::default())
                )
                .current_state()
                .unwrap(),
                ResourceState::Missing,
                "ordinary source files must remain installable"
            );
        }
    }

    #[test]
    fn placeholder_indexes_are_scoped_to_each_repository_and_build_pass() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("repo");
        let overlay = fixture.path().join("overlay");
        let initialize = |path: &Path, mode| {
            let repo = git2::Repository::init(path).unwrap();
            std::fs::create_dir(path.join("symlinks")).unwrap();
            std::fs::write(path.join("symlinks/source"), "target").unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("symlinks/source")).unwrap();
            let mut entry = index.get_path(Path::new("symlinks/source"), 0).unwrap();
            entry.mode = mode;
            index.add(&entry).unwrap();
            index.write().unwrap();
        };
        initialize(&root, 0o120_000);
        initialize(&overlay, 0o100_644);
        let source = root.join("symlinks/source");
        let ctx = make_linux_context(root.clone(), None).with_home(fixture.path().join("home"));
        let entries = [symlink("source", None)];
        assert_eq!(
            build_resources(&ctx, &entries)
                .pop()
                .unwrap()
                .current_state()
                .unwrap(),
            ResourceState::Missing
        );
        let windows_ctx = crate::test_helpers::make_windows_context(root.clone(), None)
            .with_home(ctx.home().to_path_buf());
        assert!(
            matches!(build_resources(&windows_ctx, &entries).pop().unwrap().current_state().unwrap(), ResourceState::Invalid { reason } if reason.contains("Git records it as a symlink"))
        );
        let mut cache = GitSymlinkPlaceholders::default();
        assert!(
            cache
                .reason(&source, &root)
                .unwrap()
                .contains("Git records it as a symlink")
        );
        assert!(
            cache
                .reason(&overlay.join("symlinks/source"), &overlay)
                .is_none()
        );

        // A pass uses its original index, but a new pass must see index changes.
        let repo = git2::Repository::open(&root).unwrap();
        let mut index = repo.index().unwrap();
        let mut entry = index.get_path(Path::new("symlinks/source"), 0).unwrap();
        entry.mode = 0o100_644;
        index.add(&entry).unwrap();
        index.write().unwrap();
        assert!(cache.reason(&source, &root).is_some());
        assert!(
            GitSymlinkPlaceholders::default()
                .reason(&source, &root)
                .is_none()
        );

        std::fs::remove_file(&source).unwrap();
        std::fs::create_dir(&source).unwrap();
        assert!(
            cache.reason(&source, &root).is_none(),
            "a directory is not a placeholder file"
        );
        std::fs::remove_dir(&source).unwrap();
        assert!(
            cache.reason(&source, &root).is_none(),
            "an absent source is not a placeholder file"
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("missing", &source).unwrap();
            assert!(
                cache.reason(&source, &root).is_none(),
                "a real symlink is not a placeholder file"
            );
        }
    }

    #[test]
    fn unavailable_placeholder_index_is_best_effort_and_cached_for_the_pass() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        let source = root.join("source");
        std::fs::write(&source, "target").unwrap();
        let mut cache = GitSymlinkPlaceholders::default();
        assert!(cache.reason(&source, root).is_none());
        let repo = git2::Repository::init(root).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("source")).unwrap();
        let mut entry = index.get_path(Path::new("source"), 0).unwrap();
        entry.mode = 0o120_000;
        index.add(&entry).unwrap();
        index.write().unwrap();
        assert!(cache.reason(&source, root).is_none());
        assert!(
            GitSymlinkPlaceholders::default()
                .reason(&source, root)
                .is_some()
        );
    }

    #[test]
    fn target_for_ssh() {
        let home = PathBuf::from("/home/user");
        let target = compute_target(&home, "ssh/config");
        assert_eq!(target, PathBuf::from("/home/user/.ssh/config"));
    }

    // ------------------------------------------------------------------
    // InstallSymlinks::should_run
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // UninstallSymlinks::should_run
    // ------------------------------------------------------------------

    #[test]
    fn uninstall_should_run_false_when_no_symlinks_configured() {
        let ctx = make_linux_context(PathBuf::from("/tmp"), None);
        assert!(!UninstallSymlinks::new(handle(vec![])).should_run(&ctx));
    }

    #[test]
    fn uninstall_should_run_true_when_symlinks_configured() {
        let ctx = make_linux_context(PathBuf::from("/tmp"), None);
        let task = UninstallSymlinks::new(handle(vec![symlink("bashrc", None)]));
        assert!(task.should_run(&ctx));
    }

    // ------------------------------------------------------------------
    // InstallSymlinks::run / UninstallSymlinks::run — real filesystem
    // ------------------------------------------------------------------

    #[cfg(unix)]
    #[test]
    fn redirected_home_target_requires_opt_in_for_preview_apply_and_remove() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("repo");
        let home = fixture.path().join("home");
        let outside = fixture.path().join("outside");
        std::fs::create_dir_all(root.join("symlinks")).unwrap();
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir(&outside).unwrap();
        let source = root.join("symlinks/bashrc");
        std::fs::write(&source, "managed content").unwrap();
        std::os::unix::fs::symlink(&outside, home.join(".config")).unwrap();
        let target = outside.join("nested/config");
        let ctx = make_linux_context(root.clone(), None).with_home(home.clone());
        let mut entry = symlink("bashrc", Some(".config/nested/config"));
        let blocked = build_resource(&entry, &root, &home, &ctx.executor_arc(), None);

        assert!(matches!(
            blocked.current_state().unwrap(),
            ResourceState::Invalid { reason }
                if reason.contains("allow_external_target") && reason.contains(&*target.to_string_lossy())
        ));
        assert!(crate::engine::Resource::apply(&blocked).is_err());
        assert!(crate::engine::RemovableResource::remove(&blocked).is_err());
        assert!(!outside.join("nested").exists());

        entry.allow_external_target = true;
        let allowed = build_resource(&entry, &root, &home, &ctx.executor_arc(), None);
        assert_eq!(allowed.current_state().unwrap(), ResourceState::Missing);
        assert!(
            crate::engine::Resource::description(&allowed).contains(&*target.to_string_lossy())
        );
        crate::engine::Resource::apply(&allowed).unwrap();
        assert_eq!(std::fs::read_link(&target).unwrap(), source);
        assert!(matches!(
            blocked.current_state().unwrap(),
            ResourceState::Invalid { .. }
        ));
        assert!(crate::engine::RemovableResource::remove(&blocked).is_err());
        assert!(target.is_symlink());
        crate::engine::RemovableResource::remove(&allowed).unwrap();
        assert!(!target.is_symlink());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "managed content");
    }

    #[cfg(unix)]
    #[test]
    fn install_run_creates_symlink_for_configured_source() {
        let repo_dir = tempfile::tempdir().unwrap();
        let home_dir = tempfile::tempdir().unwrap();

        // Create symlinks/ dir and a source file inside it
        let symlinks_dir = repo_dir.path().join("symlinks");
        std::fs::create_dir(&symlinks_dir).unwrap();
        std::fs::write(symlinks_dir.join("bashrc"), "# bash config").unwrap();

        let ctx = make_linux_context(repo_dir.path().to_path_buf(), None)
            .with_home(home_dir.path().to_path_buf());

        let task = InstallSymlinks::new(handle(vec![symlink("bashrc", None)]));
        let result = task.run(&ctx).unwrap();
        assert!(matches!(
            result,
            TaskResult::Batch(stats) if stats.changed_count() == 1
        ));

        // Symlink must exist at $HOME/.bashrc and point to symlinks/bashrc
        let link = home_dir.path().join(".bashrc");
        assert!(
            link.symlink_metadata().is_ok(),
            "symlink should exist at ~/.bashrc"
        );
        let target = std::fs::read_link(&link).unwrap();
        assert_eq!(target, symlinks_dir.join("bashrc"));
    }

    #[cfg(unix)]
    #[test]
    fn install_run_skips_source_path_traversal() {
        let repo_dir = tempfile::tempdir().unwrap();
        let home_dir = tempfile::tempdir().unwrap();
        let outside = repo_dir.path().join("outside");
        std::fs::write(&outside, "outside").unwrap();

        let ctx = make_linux_context(repo_dir.path().to_path_buf(), None)
            .with_home(home_dir.path().to_path_buf());

        let task =
            InstallSymlinks::new(handle(vec![symlink("../outside", Some("escaped-source"))]));
        let result = task.run(&ctx).unwrap();
        assert!(matches!(
            result,
            TaskResult::Batch(stats) if stats.failed_count() == 1
        ));
        assert!(!home_dir.path().join("escaped-source").exists());
    }

    #[cfg(unix)]
    #[test]
    fn install_run_skips_source_symlink_outside_root() {
        let repo_dir = tempfile::tempdir().unwrap();
        let home_dir = tempfile::tempdir().unwrap();
        let outside_dir = tempfile::tempdir().unwrap();
        let symlinks_dir = repo_dir.path().join("symlinks");
        std::fs::create_dir(&symlinks_dir).unwrap();
        let outside = outside_dir.path().join("outside");
        std::fs::write(&outside, "outside").unwrap();
        std::os::unix::fs::symlink(&outside, symlinks_dir.join("escaped")).unwrap();

        let ctx = make_linux_context(repo_dir.path().to_path_buf(), None)
            .with_home(home_dir.path().to_path_buf());
        let task = InstallSymlinks::new(handle(vec![symlink("escaped", None)]));

        let result = task.run(&ctx).unwrap();

        assert!(matches!(
            result,
            TaskResult::Batch(stats) if stats.failed_count() == 1
        ));
        assert!(!home_dir.path().join(".escaped").exists());
    }

    #[cfg(unix)]
    #[test]
    fn historical_editor_migration_rechecks_a_retargeted_source_and_can_retry() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let home = root.join("home");
        let sources = root.join("symlinks");
        let editor = sources.join("vim");
        let managed = sources.join("managed-editor");
        let outside = root.join("user-editor");
        let historical = sources.join("config/nvim");
        let installed = home.join(".config/nvim");
        std::fs::create_dir_all(historical.parent().unwrap()).unwrap();
        std::fs::create_dir_all(installed.parent().unwrap()).unwrap();
        for (dir, text) in [(&managed, "managed"), (&outside, "custom")] {
            std::fs::create_dir(dir).unwrap();
            std::fs::write(dir.join("init.vim"), text).unwrap();
        }
        std::os::unix::fs::symlink(&managed, &editor).unwrap();
        std::os::unix::fs::symlink(&historical, &installed).unwrap();
        let ctx = make_linux_context(root, None).with_home(home);
        let resource = build_resources(&ctx, &[symlink("vim", Some(".config/nvim"))])
            .pop()
            .unwrap();
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Incorrect { .. }
        ));
        assert_eq!(resource.removal_state().unwrap(), ResourceState::Correct);

        // A checkout edit between planning and mutation is not authorization
        // to link to or import the user's separate editor configuration.
        std::fs::remove_file(&editor).unwrap();
        std::os::unix::fs::symlink(&outside, &editor).unwrap();
        for result in [resource.apply(), resource.remove()] {
            assert!(
                format!("{:#}", result.unwrap_err()).contains("outside its symlinks directory")
            );
            assert_eq!(std::fs::read_link(&installed).unwrap(), historical);
        }
        assert_eq!(
            std::fs::read_to_string(outside.join("init.vim")).unwrap(),
            "custom"
        );
        assert_eq!(
            std::fs::read_dir(installed.parent().unwrap())
                .unwrap()
                .count(),
            1
        );

        std::fs::remove_file(&editor).unwrap();
        std::os::unix::fs::symlink(&managed, &editor).unwrap();
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        assert_eq!(std::fs::read_link(&installed).unwrap(), editor);
        assert_eq!(resource.apply().unwrap(), ResourceChange::AlreadyCorrect);
        assert_eq!(resource.remove().unwrap(), ResourceChange::Applied);
        assert!(!installed.is_symlink());
        assert_eq!(
            std::fs::read_to_string(installed.join("init.vim")).unwrap(),
            "managed"
        );
    }

    #[cfg(unix)]
    #[test]
    fn install_run_skips_target_path_traversal() {
        let repo_dir = tempfile::tempdir().unwrap();
        let home_dir = tempfile::tempdir().unwrap();
        let symlinks_dir = repo_dir.path().join("symlinks");
        std::fs::create_dir(&symlinks_dir).unwrap();
        std::fs::write(symlinks_dir.join("bashrc"), "# bash config").unwrap();
        let outside_target = home_dir.path().join("..").join("outside-target");
        let ctx = make_linux_context(repo_dir.path().to_path_buf(), None)
            .with_home(home_dir.path().to_path_buf());

        let task = InstallSymlinks::new(handle(vec![symlink("bashrc", Some("../outside-target"))]));
        let result = task.run(&ctx).unwrap();
        assert!(matches!(
            result,
            TaskResult::Batch(stats) if stats.failed_count() == 1
        ));
        assert!(!outside_target.exists());
    }

    #[cfg(unix)]
    #[test]
    fn uninstall_run_materializes_symlink_content() {
        let repo_dir = tempfile::tempdir().unwrap();
        let home_dir = tempfile::tempdir().unwrap();

        // Create source file
        let symlinks_dir = repo_dir.path().join("symlinks");
        std::fs::create_dir(&symlinks_dir).unwrap();
        std::fs::write(symlinks_dir.join("bashrc"), "# bash config").unwrap();

        // Pre-create the symlink at $HOME/.bashrc
        let link = home_dir.path().join(".bashrc");
        std::os::unix::fs::symlink(symlinks_dir.join("bashrc"), &link).unwrap();

        let ctx = make_linux_context(repo_dir.path().to_path_buf(), None)
            .with_home(home_dir.path().to_path_buf());

        let task = UninstallSymlinks::new(handle(vec![symlink("bashrc", None)]));
        let result = task.run(&ctx).unwrap();
        assert!(matches!(
            result,
            TaskResult::Batch(stats) if stats.changed_count() == 1
        ));

        // Must be a regular file (materialized), not a symlink
        let meta = std::fs::symlink_metadata(&link).unwrap();
        assert!(
            !meta.is_symlink(),
            "target should be materialized to a regular file"
        );
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "# bash config");
    }

    #[test]
    fn uninstall_run_materializes_content_when_target_is_missing() {
        let repo_dir = tempfile::tempdir().unwrap();
        let home_dir = tempfile::tempdir().unwrap();
        let symlinks_dir = repo_dir.path().join("symlinks/config/git");
        std::fs::create_dir_all(&symlinks_dir).unwrap();
        std::fs::write(symlinks_dir.join("config"), "# git config").unwrap();

        let target = home_dir.path().join(".config/git/config");
        let ctx = make_linux_context(repo_dir.path().to_path_buf(), None)
            .with_home(home_dir.path().to_path_buf());

        let config = handle(vec![symlink("config/git/config", None)]);
        let default = UninstallSymlinks::new(config.clone()).run(&ctx).unwrap();
        assert!(matches!(default, TaskResult::Batch(stats) if stats.changed_count() == 0));
        assert!(!target.exists());
        let result = UninstallSymlinks::with_restore_missing(config, true)
            .run(&ctx)
            .unwrap();

        assert!(matches!(
            result,
            TaskResult::Batch(stats) if stats.changed_count() == 1
        ));
        assert_eq!(std::fs::read_to_string(target).unwrap(), "# git config");
    }

    #[cfg(unix)]
    #[test]
    fn uninstall_run_parallel_materializes_similar_file_names() {
        let repo_dir = tempfile::tempdir().unwrap();
        let home_dir = tempfile::tempdir().unwrap();

        let symlinks_dir = repo_dir.path().join("symlinks/config/systemd/user");
        std::fs::create_dir_all(&symlinks_dir).unwrap();
        std::fs::write(
            symlinks_dir.join("clean-home-tmp.service"),
            "[Service]\nExecStart=/bin/true\n",
        )
        .unwrap();
        std::fs::write(
            symlinks_dir.join("clean-home-tmp.timer"),
            "[Timer]\nOnCalendar=daily\n",
        )
        .unwrap();

        let target_dir = home_dir.path().join(".config/systemd/user");
        std::fs::create_dir_all(&target_dir).unwrap();
        let service_link = target_dir.join("clean-home-tmp.service");
        let timer_link = target_dir.join("clean-home-tmp.timer");
        std::os::unix::fs::symlink(symlinks_dir.join("clean-home-tmp.service"), &service_link)
            .unwrap();
        std::os::unix::fs::symlink(symlinks_dir.join("clean-home-tmp.timer"), &timer_link).unwrap();

        let ctx = make_linux_context(repo_dir.path().to_path_buf(), None)
            .with_home(home_dir.path().to_path_buf())
            .with_parallel(true);

        let task = UninstallSymlinks::new(handle(vec![
            symlink("config/systemd/user/clean-home-tmp.service", None),
            symlink("config/systemd/user/clean-home-tmp.timer", None),
        ]));
        let result = task.run(&ctx).unwrap();
        assert!(matches!(
            result,
            TaskResult::Batch(stats) if stats.changed_count() == 2
        ));

        let service_meta = std::fs::symlink_metadata(&service_link).unwrap();
        let timer_meta = std::fs::symlink_metadata(&timer_link).unwrap();
        assert!(!service_meta.is_symlink());
        assert!(!timer_meta.is_symlink());
        assert_eq!(
            std::fs::read_to_string(&service_link).unwrap(),
            "[Service]\nExecStart=/bin/true\n"
        );
        assert_eq!(
            std::fs::read_to_string(&timer_link).unwrap(),
            "[Timer]\nOnCalendar=daily\n"
        );
    }
}
