//! Process environment access seam.
//!
//! Reading `std::env` directly from task and resource code makes behaviour
//! depend on process-global state that tests cannot change safely — Rust 2024
//! makes `std::env::set_var` `unsafe` precisely because it races with other
//! threads.  Code that needs environment values therefore takes an [`Env`]
//! handle instead of calling `std::env` inline.
//!
//! Production wiring uses [`SystemEnv`]; tests use [`MapEnv`] to supply a
//! deterministic set of variables without mutating the real process
//! environment.
//!
//! Startup-only reads that happen before a
//! [`Context`](crate::engine::Context) exists (argument parsing, re-exec
//! guards, log-directory discovery) may still touch `std::env` directly; the
//! seam covers everything reachable from a context.
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::sync::Arc;

/// Read-only access to the process environment.
///
/// Implementors must be cheap to clone behind an [`Arc`] and safe to share
/// across the worker threads used by parallel resource processing.
pub trait Env: std::fmt::Debug + Send + Sync {
    /// Return the raw value of `key`, or `None` when it is unset.
    fn var_os(&self, key: &str) -> Option<OsString>;

    /// Return the value of `key` as UTF-8, or `None` when it is unset or not
    /// valid Unicode.
    fn var(&self, key: &str) -> Option<String> {
        self.var_os(key)?.into_string().ok()
    }

    /// Return whether `key` is set to a non-empty value.
    ///
    /// Callers that only need presence should prefer this over
    /// `var(..).is_some()` so that an explicitly blank value is treated as
    /// unset consistently across the codebase.
    fn is_set(&self, key: &str) -> bool {
        self.var_os(key).is_some_and(|value| !value.is_empty())
    }
}

/// The real process environment.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemEnv;

impl Env for SystemEnv {
    fn var_os(&self, key: &str) -> Option<OsString> {
        std::env::var_os(key)
    }
}

/// Return a shared handle to the real process environment.
#[must_use]
pub fn system() -> Arc<dyn Env> {
    Arc::new(SystemEnv)
}

/// An in-memory environment for tests.
///
/// Variables absent from the map read as unset, so a `MapEnv` isolates the
/// code under test from whatever the host happens to export.
#[derive(Debug, Clone, Default)]
pub struct MapEnv {
    /// Variables visible to this environment, keyed by name.
    vars: BTreeMap<String, OsString>,
}

impl MapEnv {
    /// Create an environment with no variables set.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            vars: BTreeMap::new(),
        }
    }

    /// Set `key` to `value`, returning `self` for chaining.
    #[must_use]
    pub fn with(mut self, key: &str, value: impl AsRef<OsStr>) -> Self {
        self.vars
            .insert(key.to_owned(), value.as_ref().to_os_string());
        self
    }

    /// Wrap this environment in an [`Arc`] for injection.
    #[must_use]
    pub fn into_handle(self) -> Arc<dyn Env> {
        Arc::new(self)
    }
}

impl Env for MapEnv {
    fn var_os(&self, key: &str) -> Option<OsString> {
        self.vars.get(key).cloned()
    }
}

impl<E: Env + ?Sized> Env for Arc<E> {
    fn var_os(&self, key: &str) -> Option<OsString> {
        (**self).var_os(key)
    }
}

#[cfg(test)]
mod tests {
    use super::{Env as _, MapEnv, SystemEnv};

    #[test]
    fn map_env_reads_back_configured_values() {
        let env = MapEnv::new().with("SHELL", "/bin/zsh");
        assert_eq!(env.var("SHELL"), Some("/bin/zsh".to_string()));
        assert_eq!(env.var("UNSET"), None);
    }

    #[test]
    fn map_env_treats_blank_values_as_unset_for_presence() {
        let env = MapEnv::new().with("FLAG", "");
        assert_eq!(env.var("FLAG"), Some(String::new()));
        assert!(!env.is_set("FLAG"));
        assert!(MapEnv::new().with("FLAG", "1").is_set("FLAG"));
    }

    #[test]
    fn system_env_reads_the_real_process_environment() {
        // PATH is set on every platform this CLI supports.
        assert!(SystemEnv.var_os("PATH").is_some());
        assert!(
            SystemEnv
                .var("dotfiles_env_seam_should_never_be_set")
                .is_none()
        );
    }

    #[test]
    fn arc_forwards_to_the_inner_environment() {
        let env = MapEnv::new().with("A", "1").into_handle();
        assert_eq!(env.var("A"), Some("1".to_string()));
    }
}
