//! Paru package provider.

use std::collections::HashSet;

use anyhow::{Context as _, Result};

use super::package::PackageProvider;
use super::pacman::PacmanProvider;
use crate::engine::ResourceChange;
use crate::infra::exec::{CommandSpec, Executor};

/// Paru provider for AUR packages.
#[derive(Debug, Clone, Copy)]
pub(super) struct ParuProvider;

/// Build the argument vector for a `paru -S` invocation over `names`.
fn paru_args<'a>(names: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec!["-S", "--needed", "--noconfirm"];
    args.extend_from_slice(names);
    args
}

impl PackageProvider for ParuProvider {
    fn name(&self) -> &'static str {
        "paru"
    }

    fn query_installed(&self, executor: &dyn Executor) -> Result<HashSet<String>> {
        PacmanProvider
            .query_installed(executor)
            .context("querying installed paru packages")
    }

    fn install(&self, name: &str, executor: &dyn Executor) -> Result<ResourceChange> {
        executor.execute(CommandSpec::new("paru").args(&paru_args(&[name])))?;
        Ok(ResourceChange::Applied)
    }

    fn batch_invocation<'a>(
        &self,
        names: &[&'a str],
        _executor: &dyn Executor,
    ) -> Result<Option<(&'static str, Vec<&'a str>)>> {
        Ok(Some(("paru", paru_args(names))))
    }
}
