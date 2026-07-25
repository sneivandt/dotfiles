use super::*;
use crate::infra::logging::Logger;
use crate::infra::logging::{MsgKind, Output, TaskRecorder, TaskStatus};
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
    fn record_task(&self, _name: &str, _status: TaskStatus, _message: Option<&str>) {}
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
