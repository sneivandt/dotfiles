//! Paru package provider.

use std::collections::HashSet;

use anyhow::Result;

use super::package::{PackageProvider, PackageResource};
use super::pacman::PacmanProvider;
use super::report::PackageInstallReport;
use crate::engine::ResourceChange;
use crate::infra::exec::Executor;

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

    fn install_missing(
        &self,
        resources: &[&PackageResource],
        executor: &dyn Executor,
    ) -> Result<PackageInstallReport> {
        let mut args = vec!["-S", "--needed", "--noconfirm"];
        let names: Vec<&str> = resources.iter().map(|r| r.name.as_str()).collect();
        args.extend(names);
        executor.run("paru", &args)?;
        Ok(PackageInstallReport::applied(
            resources
                .iter()
                .map(|resource| resource.name.clone())
                .collect(),
        ))
    }
}
