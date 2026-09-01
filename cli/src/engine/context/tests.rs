use super::*;
use crate::infra::logging::Logger;
use crate::infra::logging::{MsgKind, Output, TaskEntry, TaskRecorder};
use crate::test_helpers::{empty_config, make_linux_context};
use std::path::PathBuf;

#[derive(Debug)]
struct SilentLog;

impl Output for SilentLog {
    fn emit(&self, _kind: MsgKind, _msg: std::borrow::Cow<'_, str>) {}
    fn debug_enabled(&self) -> bool {
        false
    }
}

impl TaskRecorder for SilentLog {
    fn record_task(&self, _task: TaskEntry) {}
}

#[test]
fn root_returns_config_root() {
    let config = empty_config(PathBuf::from("/dotfiles"));
    let ctx = make_linux_context(config);
    assert_eq!(ctx.root(), Path::new("/dotfiles"));
}

#[test]
fn path_view_returns_derived_paths() {
    let config = empty_config(PathBuf::from("/dotfiles"));
    let ctx = make_linux_context(config);
    let paths = ctx.paths();
    assert_eq!(paths.root(), Path::new("/dotfiles"));
    assert_eq!(paths.symlinks_dir(), Path::new("/dotfiles/symlinks"));
    assert_eq!(paths.hooks_dir(), Path::new("/dotfiles/hooks"));
}

#[test]
fn repo_paths_returns_all_derived_paths_from_one_snapshot() {
    let config = empty_config(PathBuf::from("/dotfiles"));
    let ctx = make_linux_context(config);
    let paths = ctx.repo_paths();
    assert_eq!(paths.root, PathBuf::from("/dotfiles"));
    assert_eq!(paths.symlinks_dir, PathBuf::from("/dotfiles/symlinks"));
    assert_eq!(paths.hooks_dir, PathBuf::from("/dotfiles/hooks"));
}

#[test]
fn with_log_preserves_other_fields() {
    let config = empty_config(PathBuf::from("/dotfiles"));
    let ctx = make_linux_context(config);
    let new_log: Arc<dyn Log> = Arc::new(Logger::new("new"));
    let ctx2 = ctx.with_log(new_log);
    assert_eq!(ctx2.root(), ctx.root());
    assert_eq!(ctx2.dry_run(), ctx.dry_run());
    assert_eq!(ctx2.home(), ctx.home());
    assert_eq!(ctx2.parallel(), ctx.parallel());
}

#[test]
fn root_reflects_construction_value() {
    let config = empty_config(PathBuf::from("/my/root"));
    let ctx = make_linux_context(config);
    assert_eq!(ctx.root(), Path::new("/my/root"));
}

#[test]
fn debug_format_includes_key_fields() {
    let config = empty_config(PathBuf::from("/dotfiles"));
    let ctx = make_linux_context(config);
    let debug = format!("{ctx:?}");
    assert!(debug.contains("Context"));
    assert!(debug.contains("dry_run"));
    assert!(debug.contains("home"));
}

#[test]
fn clone_shares_arc_fields() {
    let config = empty_config(PathBuf::from("/dotfiles"));
    let ctx = make_linux_context(config);
    let ctx2 = ctx.clone();
    assert_eq!(ctx2.root(), ctx.root());
    assert_eq!(ctx2.dry_run(), ctx.dry_run());
    assert_eq!(ctx2.home(), ctx.home());
    assert_eq!(ctx2.parallel(), ctx.parallel());
    assert_eq!(ctx.platform(), ctx2.platform());
}

#[test]
fn debug_fmt_skips_closure_when_debug_logging_is_disabled() {
    let config = empty_config(PathBuf::from("/dotfiles"));
    let ctx = make_linux_context(config).with_log(Arc::new(SilentLog));
    let called = std::sync::atomic::AtomicBool::new(false);
    ctx.debug_fmt(|| {
        called.store(true, std::sync::atomic::Ordering::SeqCst);
        "debug message".to_string()
    });
    assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn new_reads_home_and_ci_from_the_injected_environment() {
    use crate::infra::env::MapEnv;
    use crate::infra::platform::{Os, Platform};

    let platform = Platform {
        os: Os::Linux,
        is_arch: false,
        is_wsl: false,
    };
    let env = MapEnv::new()
        .with("HOME", "/home/injected")
        .with("CI", "1")
        .into_handle();

    let ctx = Context::new(
        PathBuf::from("/dotfiles"),
        None,
        platform,
        Arc::new(Logger::new("test")),
        Arc::new(crate::infra::exec::ProcessExecutor::system()),
        env,
        ContextOpts::default(),
    )
    .expect("context builds from the injected environment");

    assert_eq!(ctx.home(), Path::new("/home/injected"));
    assert!(
        ctx.system().is_ci(),
        "CI is set in the injected environment"
    );
}

#[test]
fn new_fails_when_the_injected_environment_has_no_home() {
    use crate::infra::env::MapEnv;
    use crate::infra::platform::{Os, Platform};

    let platform = Platform {
        os: Os::Linux,
        is_arch: false,
        is_wsl: false,
    };

    let result = Context::new(
        PathBuf::from("/dotfiles"),
        None,
        platform,
        Arc::new(Logger::new("test")),
        Arc::new(crate::infra::exec::ProcessExecutor::system()),
        MapEnv::new().into_handle(),
        ContextOpts::default(),
    );

    assert!(result.is_err(), "HOME is unset in the injected environment");
}

#[test]
fn with_env_swaps_the_environment_without_touching_other_fields() {
    use crate::infra::env::MapEnv;

    let config = empty_config(PathBuf::from("/dotfiles"));
    let ctx = make_linux_context(config);
    let swapped = ctx.with_env(MapEnv::new().with("SHELL", "/bin/fish").into_handle());

    assert_eq!(swapped.env().var("SHELL"), Some("/bin/fish".to_string()));
    assert_eq!(swapped.root(), ctx.root());
    assert_eq!(swapped.home(), ctx.home());
}
