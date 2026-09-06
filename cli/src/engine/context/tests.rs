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
fn derived_paths_use_the_configured_root() {
    let config = empty_config(PathBuf::from("/dotfiles"));
    let ctx = make_linux_context(config);
    assert_eq!(ctx.root(), Path::new("/dotfiles"));
    assert_eq!(ctx.symlinks_dir(), Path::new("/dotfiles/symlinks"));
    assert_eq!(ctx.hooks_dir(), Path::new("/dotfiles/hooks"));
}

#[test]
fn task_log_context_shares_paths_environment_and_cancellation() {
    let config = empty_config(PathBuf::from("/dotfiles"));
    let ctx = make_linux_context(config);
    let new_log: Arc<dyn Log> = Arc::new(SilentLog);
    let ctx2 = ctx.with_log(Arc::clone(&new_log));
    assert!(Arc::ptr_eq(&ctx.paths, &ctx2.paths));
    assert!(Arc::ptr_eq(&ctx.home, &ctx2.home));
    assert!(Arc::ptr_eq(&ctx.env, &ctx2.env));
    assert!(Arc::ptr_eq(&ctx.executor, &ctx2.executor));
    assert!(Arc::ptr_eq(&ctx2.log, &new_log));
    assert!(!Arc::ptr_eq(&ctx.log, &new_log));
    assert_eq!(ctx2.dry_run(), ctx.dry_run());
    assert_eq!(ctx2.parallel(), ctx.parallel());
    ctx2.cancellation_token().cancel();
    assert!(ctx.is_cancelled());
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
fn new_preserves_explicit_ci_overrides_and_environment_defaults() {
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

    for (override_ci, expected) in [(None, true), (Some(false), false), (Some(true), true)] {
        let ctx = Context::new(
            PathBuf::from("/dotfiles"),
            None,
            platform,
            Arc::new(Logger::new("test")),
            Arc::new(crate::infra::exec::ProcessExecutor::system()),
            Arc::clone(&env),
            ContextOpts {
                is_ci: override_ci,
                ..ContextOpts::default()
            },
        )
        .expect("context builds from the injected environment");

        assert_eq!(ctx.home(), Path::new("/home/injected"));
        assert_eq!(ctx.is_ci(), expected);
        assert_eq!(ctx.require_complete(), expected);
        assert_eq!(ctx.non_interactive(), expected);
        assert_eq!(
            ctx.with_ci(!expected).require_complete(),
            expected,
            "the legacy CI builder does not reset other policy choices"
        );
    }
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
fn windows_home_prefers_userprofile_and_falls_back_to_home() {
    use crate::infra::env::MapEnv;
    use crate::infra::platform::{Os, Platform};

    for (env, expected) in [
        (
            MapEnv::new()
                .with("USERPROFILE", "C:/Users/fixture")
                .with("HOME", "/fallback"),
            Some("C:/Users/fixture"),
        ),
        (MapEnv::new().with("HOME", "/fallback"), Some("/fallback")),
        (MapEnv::new(), None),
    ] {
        let ctx = Context::new(
            "/fixture-repo".into(),
            None,
            Platform::new(Os::Windows, false),
            Arc::new(Logger::new("test")),
            Arc::new(crate::infra::exec::ProcessExecutor::system()),
            env.into_handle(),
            ContextOpts::default(),
        );
        match expected {
            Some(home) => assert_eq!(ctx.unwrap().home(), Path::new(home)),
            None => assert!(
                ctx.unwrap_err()
                    .to_string()
                    .contains("neither USERPROFILE nor HOME")
            ),
        }
    }
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
