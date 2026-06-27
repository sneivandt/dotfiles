//! Paru package provider.

use std::collections::HashSet;

use anyhow::Result;

use super::PackageProvider;
use super::pacman::PacmanProvider;
use crate::exec::Executor;
use crate::resources::ResourceChange;

/// Paru provider for AUR packages.
#[derive(Debug, Clone, Copy)]
pub(super) struct ParuProvider;

impl PackageProvider for ParuProvider {
    fn name(&self) -> &'static str {
        "paru"
    }

    fn query_installed(&self, executor: &dyn Executor) -> Result<HashSet<String>> {
        PacmanProvider.query_installed(executor)
    }

    fn install(&self, name: &str, executor: &dyn Executor) -> Result<ResourceChange> {
        executor.run("paru", &["-S", "--needed", "--noconfirm", name])?;
        Ok(ResourceChange::Applied)
    }

    fn supports_batch(&self) -> bool {
        true
    }

    fn batch_install(&self, names: &[&str], executor: &dyn Executor) -> Result<()> {
        let mut args = vec!["-S", "--needed", "--noconfirm"];
        args.extend(names);
        executor.run("paru", &args)?;
        Ok(())
    }
}
