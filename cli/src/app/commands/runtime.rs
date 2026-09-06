//! Startup policy resolved once from arguments and injectable process capabilities.

use std::io::IsTerminal as _;
use std::sync::Arc;

use crate::app::cli::GlobalOpts;
use crate::engine::context::ExecutionPolicy;
use crate::infra::env::Env;

use super::reexec::{REEXEC_GUARD_VAR, REPOSITORY_REEXEC_GUARD_VAR, SELF_UPDATE_REEXEC_GUARD_VAR};

/// Immutable startup decisions and the environment shared with task execution.
#[derive(Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent process guards and output switches"
)]
pub struct RuntimePolicy<'a> {
    pub(crate) global: &'a GlobalOpts,
    pub(crate) env: Arc<dyn Env>,
    pub(crate) execution: ExecutionPolicy,
    pub(crate) verbose: bool,
    pub(crate) reexec_guarded: bool,
    pub(crate) repository_child: bool,
    pub(crate) self_update_child: bool,
}

impl<'a> RuntimePolicy<'a> {
    /// Capture process capabilities once at the command entry point.
    #[must_use]
    pub fn detect(global: &'a GlobalOpts, verbose: bool) -> Self {
        Self::new(
            global,
            verbose,
            crate::infra::env::system(),
            std::io::stdin().is_terminal(),
            std::io::stdout().is_terminal(),
        )
    }

    /// Resolve policy using injected environment and terminal capabilities.
    #[must_use]
    pub fn new(
        global: &'a GlobalOpts,
        verbose: bool,
        env: Arc<dyn Env>,
        stdin_terminal: bool,
        stdout_terminal: bool,
    ) -> Self {
        // CI and restart guards use presence, including empty/non-Unicode values.
        // The elevated-child environment marker historically requires a value.
        let is_ci = env.var_os("CI").is_some();
        Self {
            global,
            execution: ExecutionPolicy {
                dry_run: global.dry_run,
                parallel: global.parallel,
                is_ci,
                require_complete: global.require_complete || is_ci,
                non_interactive: global.non_interactive || is_ci || !stdin_terminal,
                stdout_terminal,
                elevated_child: global.elevated_child
                    || env.is_set(crate::infra::elevation::ELEVATED_CHILD_VAR),
            },
            verbose,
            reexec_guarded: env.var_os(REEXEC_GUARD_VAR).is_some(),
            repository_child: env.var_os(REPOSITORY_REEXEC_GUARD_VAR).is_some(),
            self_update_child: env.var_os(SELF_UPDATE_REEXEC_GUARD_VAR).is_some(),
            env,
        }
    }

    /// Restart only changed checkouts owned by the unelevated parent command.
    pub(super) const fn restart_after_repository_update(&self, updated: bool) -> bool {
        updated && !self.execution.elevated_child
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::env::MapEnv;

    #[test]
    fn ci_and_terminal_policy_is_shared_with_the_context() {
        use crate::engine::Context;
        use crate::infra::platform::{Os, Platform};

        for (ci_value, is_ci) in [(None, false), (Some(""), true), (Some("false"), true)] {
            for (flags, strict, non_interactive) in [
                (vec![], false, false),
                (vec!["--fail-on-skip"], true, false),
                (vec!["--non-interactive"], false, true),
                (vec!["--fail-on-skip", "--non-interactive"], true, true),
            ] {
                let global = super::super::tests::global(&flags);
                for (stdin, stdout) in [(true, true), (true, false), (false, true), (false, false)]
                {
                    let env = MapEnv::new().with("HOME", "/fixture-home");
                    let env = if let Some(value) = ci_value {
                        env.with("CI", value)
                    } else {
                        env
                    };
                    let runtime =
                        RuntimePolicy::new(&global, false, env.into_handle(), stdin, stdout);
                    let ctx = Context::new_with_policy(
                        "/fixture-repo".into(),
                        None,
                        Platform::new(Os::Linux, false),
                        Arc::new(crate::infra::logging::Logger::new("test")),
                        Arc::new(crate::infra::exec::ProcessExecutor::system()),
                        Arc::clone(&runtime.env),
                        runtime.execution,
                    )
                    .unwrap();
                    assert!(ctx.parallel(), "parallel execution remains the default");
                    assert!(!ctx.with_parallel(false).parallel(), "sequential override");
                    assert_eq!(ctx.is_ci(), is_ci, "{ci_value:?} {flags:?}");
                    assert_eq!(
                        ctx.require_complete(),
                        strict || is_ci,
                        "{ci_value:?} {flags:?}"
                    );
                    assert_eq!(
                        ctx.non_interactive(),
                        non_interactive || is_ci || !stdin,
                        "{ci_value:?} {flags:?}"
                    );
                    assert_eq!(
                        ctx.execution_policy().can_prompt_for_elevation(),
                        !non_interactive && !is_ci && stdin && stdout,
                        "{ci_value:?} {flags:?} {stdin}/{stdout}",
                    );
                }
            }
        }
    }

    #[test]
    fn elevated_marker_requires_a_value_but_cli_marker_always_wins() {
        for flag in [false, true] {
            let global =
                super::super::tests::global(if flag { &["--elevated-child"] } else { &[] });
            for (value, marked) in [(None, false), (Some(""), false), (Some("0"), true)] {
                let env = value.map_or_else(MapEnv::new, |value| {
                    MapEnv::new().with(crate::infra::elevation::ELEVATED_CHILD_VAR, value)
                });
                let runtime = RuntimePolicy::new(&global, false, env.into_handle(), true, true);
                assert_eq!(runtime.execution.elevated_child, flag || marked);
                for updated in [false, true] {
                    assert_eq!(
                        runtime.restart_after_repository_update(updated),
                        updated && !flag && !marked,
                        "updated={updated}, flag={flag}, marker={value:?}"
                    );
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn non_unicode_ci_value_counts_as_present() {
        use std::os::unix::ffi::OsStringExt as _;
        let global = super::super::tests::global(&[]);
        let env = MapEnv::new().with("CI", std::ffi::OsString::from_vec(vec![0xff]));
        let runtime = RuntimePolicy::new(&global, false, env.into_handle(), true, true);
        assert!(runtime.execution.is_ci);
        assert!(runtime.execution.require_complete);
        assert!(runtime.execution.non_interactive);
    }
}
