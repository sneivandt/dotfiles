//! Paru package provider.

use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::{Context as _, Result};

use super::super::PARU_EXECUTABLE;
use super::package::PackageProvider;
use super::pacman::PacmanProvider;
use crate::engine::ResourceChange;
use crate::infra::exec::{CommandSpec, Executor};

/// Paru provider for AUR packages.
#[derive(Debug, Clone, Copy)]
pub(super) struct ParuProvider;

/// Build the argument vector for a `paru -S` invocation over `names`.
fn paru_args<'a>(names: &[&'a str], config_path: Option<&'a str>) -> Vec<&'a str> {
    let mut args = Vec::new();
    if let Some(path) = config_path {
        args.extend(["--config", path]);
    }
    args.extend(["-S", "--needed", "--noconfirm"]);
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

    fn install(
        &self,
        name: &str,
        executor: &dyn Executor,
        config_path: Option<&str>,
    ) -> Result<ResourceChange> {
        executor
            .execute(CommandSpec::new(PARU_EXECUTABLE).args(&paru_args(&[name], config_path)))?;
        Ok(ResourceChange::Applied)
    }

    fn batch_invocation<'a>(
        &self,
        names: &[&'a str],
        _executor: &dyn Executor,
        config_path: Option<&'a str>,
    ) -> Result<Option<(PathBuf, Vec<&'a str>)>> {
        Ok(Some((
            PathBuf::from(PARU_EXECUTABLE),
            paru_args(names, config_path),
        )))
    }
}
