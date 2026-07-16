//! Tasks: install and uninstall symlinks.
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

use crate::domains::files::config::symlinks::Symlink;
use crate::domains::files::resources::symlink::SymlinkResource;
use crate::engine::{
    Context, Domain, ProcessOpts, Task, TaskPhase, TaskResult, config_resource_task,
    process_resources_remove,
};
use crate::infra::ConfigHandle;

/// Build a single [`SymlinkResource`] from a config entry.
fn build_resource(
    s: &Symlink,
    repo_root: &Path,
    home: &Path,
    executor: &Arc<dyn crate::infra::exec::Executor>,
) -> SymlinkResource {
    let symlinks_dir = crate::domains::files::config::symlinks::resolve_symlinks_dir(s, repo_root);
    let source = symlinks_dir.join(&s.source);
    let target = s.target.as_ref().map_or_else(
        || compute_target(home, &s.source),
        |explicit| home.join(explicit),
    );
    let validation_error = crate::domains::files::config::symlinks::validate_paths(s)
        .err()
        .map(|e| e.to_string())
        .or_else(|| git_symlink_placeholder_reason(&source, repo_root));
    SymlinkResource::new(source, target, Arc::clone(executor))
        .with_validation_error(validation_error)
}

/// Build [`SymlinkResource`] instances from a symlink configuration slice.
fn build_resources(ctx: &Context, symlinks: &[Symlink]) -> Vec<SymlinkResource> {
    let paths = ctx.paths();
    let executor = ctx.system().executor_arc();
    symlinks
        .iter()
        .map(|s| build_resource(s, paths.root(), paths.home(), &executor))
        .collect()
}

config_resource_task! {
    /// Create symlinks from symlinks/ to $HOME.
    pub InstallSymlinks {
        name: "Install symlinks",
        phase: TaskPhase::Provision,
        domain: Domain::Files,
        config: Vec<Symlink>,
        items: |cfg| cfg.clone(),
        build: |s, ctx| {
            let paths = ctx.paths();
            let executor = ctx.system().executor_arc();
            build_resource(&s, paths.root(), paths.home(), &executor)
        },
        opts: ProcessOpts::strict("link"),
    }
}

/// Materialize installed symlinks into real files/directories.
#[derive(Debug)]
pub struct UninstallSymlinks {
    config: ConfigHandle<Vec<Symlink>>,
}

impl UninstallSymlinks {
    /// Create the task with a handle to the symlink configuration.
    #[must_use]
    pub const fn new(config: ConfigHandle<Vec<Symlink>>) -> Self {
        Self { config }
    }
}

impl Task for UninstallSymlinks {
    fn name(&self) -> &'static str {
        "Materialize symlinks"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Provision
    }

    fn domain(&self) -> Domain {
        Domain::Files
    }

    fn should_run(&self, _ctx: &Context) -> bool {
        !self.config.read().is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let symlinks = self.config.read();
        process_resources_remove(ctx, build_resources(ctx, &symlinks), "materialize")
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
    home.join(format!(".{source}"))
}

#[cfg(windows)]
fn git_symlink_placeholder_reason(source: &Path, repo_root: &Path) -> Option<String> {
    let metadata = std::fs::symlink_metadata(source).ok()?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return None;
    }

    let relative_source = source.strip_prefix(repo_root).ok()?;
    let repository = git2::Repository::open(repo_root).ok()?;
    let index = repository.index().ok()?;
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

#[cfg(not(windows))]
const fn git_symlink_placeholder_reason(_source: &Path, _repo_root: &Path) -> Option<String> {
    None
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::domains::files::config::symlinks::Symlink;
    use crate::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    fn sym(source: &str, target: Option<&str>) -> Symlink {
        Symlink {
            source: source.to_string(),
            target: target.map(str::to_string),
            origin: None,
        }
    }

    fn handle(symlinks: Vec<Symlink>) -> ConfigHandle<Vec<Symlink>> {
        ConfigHandle::new(symlinks)
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
    fn target_for_ssh() {
        let home = PathBuf::from("/home/user");
        let target = compute_target(&home, "ssh/config");
        assert_eq!(target, PathBuf::from("/home/user/.ssh/config"));
    }

    // ------------------------------------------------------------------
    // InstallSymlinks::should_run
    // ------------------------------------------------------------------

    #[test]
    fn install_should_run_is_true_without_explicit_guard() {
        let ctx = make_linux_context(empty_config(PathBuf::from("/tmp")));
        assert!(InstallSymlinks::new(handle(vec![])).should_run(&ctx));
    }

    #[test]
    fn install_should_run_true_when_symlinks_configured() {
        let ctx = make_linux_context(empty_config(PathBuf::from("/tmp")));
        let task = InstallSymlinks::new(handle(vec![sym("bashrc", None)]));
        assert!(task.should_run(&ctx));
    }

    // ------------------------------------------------------------------
    // UninstallSymlinks::should_run
    // ------------------------------------------------------------------

    #[test]
    fn uninstall_should_run_false_when_no_symlinks_configured() {
        let ctx = make_linux_context(empty_config(PathBuf::from("/tmp")));
        assert!(!UninstallSymlinks::new(handle(vec![])).should_run(&ctx));
    }

    #[test]
    fn uninstall_should_run_true_when_symlinks_configured() {
        let ctx = make_linux_context(empty_config(PathBuf::from("/tmp")));
        let task = UninstallSymlinks::new(handle(vec![sym("bashrc", None)]));
        assert!(task.should_run(&ctx));
    }

    // ------------------------------------------------------------------
    // InstallSymlinks::run / UninstallSymlinks::run — real filesystem
    // ------------------------------------------------------------------

    #[cfg(unix)]
    #[test]
    fn install_run_creates_symlink_for_configured_source() {
        let repo_dir = tempfile::tempdir().unwrap();
        let home_dir = tempfile::tempdir().unwrap();

        // Create symlinks/ dir and a source file inside it
        let symlinks_dir = repo_dir.path().join("symlinks");
        std::fs::create_dir(&symlinks_dir).unwrap();
        std::fs::write(symlinks_dir.join("bashrc"), "# bash config").unwrap();

        let ctx = make_linux_context(empty_config(repo_dir.path().to_path_buf()))
            .with_home(home_dir.path().to_path_buf());

        let task = InstallSymlinks::new(handle(vec![sym("bashrc", None)]));
        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::OkWithMessage(_)));

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

        let ctx = make_linux_context(empty_config(repo_dir.path().to_path_buf()))
            .with_home(home_dir.path().to_path_buf());

        let task = InstallSymlinks::new(handle(vec![sym("../outside", Some("escaped-source"))]));
        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Failed(_)));
        assert!(!home_dir.path().join("escaped-source").exists());
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

        let ctx = make_linux_context(empty_config(repo_dir.path().to_path_buf()))
            .with_home(home_dir.path().to_path_buf());

        let task = InstallSymlinks::new(handle(vec![sym("bashrc", Some("../outside-target"))]));
        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Failed(_)));
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

        let ctx = make_linux_context(empty_config(repo_dir.path().to_path_buf()))
            .with_home(home_dir.path().to_path_buf());

        let task = UninstallSymlinks::new(handle(vec![sym("bashrc", None)]));
        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::OkWithMessage(_)));

        // Must be a regular file (materialized), not a symlink
        let meta = std::fs::symlink_metadata(&link).unwrap();
        assert!(
            !meta.is_symlink(),
            "target should be materialized to a regular file"
        );
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "# bash config");
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

        let ctx = make_linux_context(empty_config(repo_dir.path().to_path_buf()))
            .with_home(home_dir.path().to_path_buf())
            .with_parallel(true);

        let task = UninstallSymlinks::new(handle(vec![
            sym("config/systemd/user/clean-home-tmp.service", None),
            sym("config/systemd/user/clean-home-tmp.timer", None),
        ]));
        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::OkWithMessage(_)));

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
