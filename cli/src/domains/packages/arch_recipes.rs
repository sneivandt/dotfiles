//! Dotfiles-managed Arch package recipes and release pins, independent of tool use.

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::engine::Context;
#[cfg(test)]
use crate::engine::{Operation, OperationState, Task, TaskResult};
use crate::infra::logging::OutputExt as _;

mod paru_config;
mod release_updates;
mod snapshots;

pub(crate) use paru_config::{ConfigureParu, ParuBase};
pub(crate) use release_updates::UpdateCustomPackageVersions;

#[cfg(test)]
use paru_config::{ParuConfig, config_path};
use paru_config::{build_root, paru_config_path, repository_section};
#[cfg(test)]
use release_updates::{ReleaseUpdates, release_pins};
use snapshots::Snapshot;

fn preparation_cancelled() -> crate::infra::exec::ExecError {
    crate::infra::exec::ExecError::Cancelled {
        command: "custom package preparation".into(),
        result: crate::infra::exec::ExecResult {
            stdout: String::new(),
            stderr: String::new(),
            success: false,
            code: None,
        },
    }
}

fn check_publication(ctx: &Context) -> Result<()> {
    if ctx.is_cancelled() {
        return Err(preparation_cancelled().into());
    }
    Ok(())
}

/// Evaluate independent recipes in bounded workers; publication stays ordered.
fn map_recipes<T: Sync, R: Send>(
    ctx: &Context,
    items: &[T],
    work: impl Fn(&T) -> Result<R> + Sync + Send,
) -> Vec<Result<R>> {
    use rayon::prelude::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    let cancelled = AtomicBool::new(false);
    let evaluate = |item: &T| {
        if ctx.is_cancelled() || cancelled.load(Ordering::Acquire) {
            return Err(preparation_cancelled().into());
        }
        let result = work(item);
        if result
            .as_ref()
            .err()
            .and_then(|error| error.downcast_ref::<crate::infra::exec::ExecError>())
            .is_some_and(crate::infra::exec::ExecError::is_cancelled)
        {
            cancelled.store(true, Ordering::Release);
        }
        result
    };
    if !ctx.parallel() || items.len() < 2 {
        return items.iter().map(evaluate).collect();
    }
    let task_name = crate::infra::logging::log_thread_name();
    items
        .par_chunks(items.len().div_ceil(4))
        .map(|chunk| {
            crate::infra::logging::set_log_thread_name(&task_name);
            chunk.iter().map(evaluate).collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
        .into_iter()
        .flatten()
        .collect()
}

fn recipe_directories(ctx: &Context) -> Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(ctx.root().join("packages")) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context("reading Arch package recipe directory"),
    };
    let mut directories = Vec::new();
    for entry in entries {
        let entry = entry.context("reading Arch package recipe entry")?;
        if entry.file_type()?.is_dir() {
            directories.push(entry.path());
        }
    }
    directories.sort();
    Ok(directories)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReleaseSource {
    package_base: String,
    #[serde(default)]
    packages: Option<Vec<String>>,
    repository: String,
    #[serde(default)]
    tag_prefix: Option<String>,
    assets: BTreeMap<String, String>,
}

impl ReleaseSource {
    fn validate_srcinfo(&self, metadata: &str) -> Result<()> {
        use std::collections::BTreeSet;

        let values = |key: &str| -> Vec<&str> {
            metadata
                .lines()
                .filter_map(|line| line.trim().split_once(" = "))
                .filter_map(|(name, value)| (name == key).then_some(value))
                .collect()
        };
        ensure!(
            values("pkgbase") == [self.package_base.as_str()],
            "PKGBUILD package base does not match release.json ({})",
            self.package_base
        );
        let packages = values("pkgname");
        let actual: BTreeSet<_> = packages.iter().copied().collect();
        let expected: BTreeSet<_> = self.package_names().iter().map(String::as_str).collect();
        ensure!(
            packages.len() == actual.len() && actual == expected,
            "PKGBUILD package names {actual:?} do not match release.json {expected:?}"
        );
        let actual_arches: BTreeSet<_> = values("arch").into_iter().collect();
        let expected_arches: BTreeSet<_> = self.assets.keys().map(String::as_str).collect();
        ensure!(
            actual_arches == expected_arches,
            "PKGBUILD architectures {actual_arches:?} do not match release.json {expected_arches:?}"
        );
        Ok(())
    }

    fn package_names(&self) -> &[String] {
        self.packages
            .as_deref()
            .unwrap_or_else(|| std::slice::from_ref(&self.package_base))
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            safe_component(&self.package_base),
            "invalid custom package base"
        );
        ensure!(
            !self.package_names().is_empty(),
            "Arch package recipe has no package names"
        );
        for name in self.package_names() {
            ensure!(safe_component(name), "invalid custom package name");
        }
        let components: Vec<_> = self.repository.split('/').collect();
        ensure!(
            components.len() == 2 && components.iter().all(|part| safe_component(part)),
            "invalid GitHub repository"
        );
        if let Some(prefix) = &self.tag_prefix {
            ensure!(safe_component(prefix), "invalid release tag prefix");
        }
        ensure!(!self.assets.is_empty(), "release descriptor has no assets");
        for (arch, asset) in &self.assets {
            ensure!(
                !arch.is_empty() && arch.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
                "invalid package architecture"
            );
            ensure!(!asset.is_empty(), "release asset name is empty");
        }
        Ok(())
    }
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.+".contains(&b))
}

fn read_optional(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

fn report_failures(ctx: &Context, failures: &[String]) {
    for failure in failures {
        ctx.log().warn(failure);
    }
}

// Validate ownership before any network requests or writes. If ownership cannot
// be established, callers must fail closed rather than fall back to the AUR.
fn recipes(ctx: &Context) -> Result<Vec<(PathBuf, ReleaseSource)>> {
    let mut recipes = Vec::new();
    let mut bases = std::collections::HashSet::new();
    let mut names = std::collections::HashSet::new();
    for directory in recipe_directories(ctx)? {
        let Some(raw) = read_optional(&directory.join("release.json"))? else {
            ensure!(
                !directory.join("PKGBUILD").try_exists()?,
                "Arch package recipe is missing release.json: {}",
                directory.display()
            );
            continue;
        };
        let source: ReleaseSource = serde_json::from_str(&raw)
            .with_context(|| format!("parsing {}", directory.join("release.json").display()))?;
        source.validate()?;
        ensure!(
            bases.insert(source.package_base.clone()),
            "duplicate custom package base"
        );
        for name in source.package_names() {
            ensure!(
                names.insert(name.clone()),
                "duplicate custom package name: {name}"
            );
        }
        recipes.push((directory, source));
    }
    Ok(recipes)
}

/// Unavailable dotfiles-managed names among packages still needed for install.
/// Preview accepts inputs that prerequisite tasks would prepare without writing.
pub(super) fn unavailable_packages(
    ctx: &Context,
    needs_install: impl Fn(&str) -> bool,
) -> Result<BTreeMap<String, String>> {
    // Check descriptor ownership and conflicts even for packages that are installed.
    let mut recipes = recipes(ctx)?
        .into_iter()
        .filter(|(_, source)| {
            source
                .package_names()
                .iter()
                .any(|name| needs_install(name))
        })
        .peekable();
    if recipes.peek().is_none() {
        return Ok(BTreeMap::new());
    }
    let root = build_root(ctx)?;
    let config = read_optional(&paru_config_path(ctx)?);
    let mut unavailable = BTreeMap::new();
    for (directory, source) in recipes {
        let readiness = (|| -> Result<()> {
            let snapshot = Snapshot::plan(&root, &source.package_base, &directory, ctx.dry_run())?;
            if ctx.dry_run() {
                snapshot.needs_write()?;
            } else {
                ensure!(
                    snapshot.validated_metadata(&source)?.is_some(),
                    "snapshot is not validated for the current inputs and release descriptor"
                );
                let section = repository_section(&source.package_base, &snapshot.path)?;
                let config = config
                    .as_ref()
                    .map_err(|error| anyhow::anyhow!("reading Paru config: {error:#}"))?;
                ensure!(
                    config.as_ref().is_some_and(|text| text.contains(&section)),
                    "local repository is absent from Paru configuration"
                );
            }
            Ok(())
        })();
        if let Err(error) = readiness {
            for name in source
                .package_names()
                .iter()
                .filter(|name| needs_install(name))
            {
                unavailable.insert(name.clone(), format!("{error:#}"));
            }
        }
    }
    Ok(unavailable)
}

#[cfg(test)]
mod tests;
