//! GitHub release pin refreshes for tracked Arch package recipes.

use std::fmt::Write as _;
use std::path::PathBuf;

use anyhow::{Context as _, Result, ensure};
use serde::Deserialize;

use super::{
    ReleaseSource, check_publication, read_optional, recipes, report_failures, safe_component,
};
use crate::engine::{
    Context, Operation, OperationState, Task, TaskResult, TaskStats, process_operation,
    task_metadata,
};
use crate::infra::fs::write_atomic;
use crate::infra::github::{CacheWrites, Releases, github_token};
use crate::infra::logging::OutputExt as _;

/// Refresh release pins for the tracked Arch package recipe directories.
#[derive(Debug)]
pub(crate) struct UpdateCustomPackageVersions;

impl Task for UpdateCustomPackageVersions {
    task_metadata! {
        name: "Custom package versions",
        selector: "custom-package-versions",
        update_only: true,
    }

    fn ordering_dependencies(&self) -> &[crate::engine::TaskId] {
        const DEPS: &[crate::engine::TaskId] =
            &[crate::engine::TaskId::Type(std::any::TypeId::of::<
                crate::domains::packages::install::InstallPackages,
            >())];
        DEPS
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform().supports_aur()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        process_operation(ctx, &ReleaseUpdates)
    }
}

pub(super) struct ReleaseUpdates;

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    digest: Option<String>,
}

struct ReleasePins {
    recipe: String,
    path: PathBuf,
    content: String,
}

pub(super) struct ReleasePlan {
    files: Vec<ReleasePins>,
    failures: Vec<String>,
    cache: CacheWrites,
}

impl Operation for ReleaseUpdates {
    type Plan = ReleasePlan;

    fn current_state(&self, ctx: &Context) -> Result<OperationState<Self::Plan>> {
        let mut files = Vec::new();
        let mut failures = Vec::new();
        let recipes = recipes(ctx)?;
        if recipes.is_empty() {
            return Ok(OperationState::Complete);
        }
        let token = github_token(&**ctx.env(), ctx.executor());
        let mut releases = Releases::new(ctx.root(), ctx.http(), token)
            .with_cancellation(ctx.cancellation_token());
        for (directory, source) in recipes {
            let discovered =
                releases.latest(&source.repository, |body| release_pins(&source, body));
            match discovered {
                Ok(pins) => {
                    let path = directory.join("version.sh");
                    match read_optional(&path) {
                        Ok(existing) if existing.as_deref() == Some(&pins) => {}
                        Ok(_) => files.push(ReleasePins {
                            recipe: directory
                                .file_name()
                                .context("missing recipe name")?
                                .to_string_lossy()
                                .into_owned(),
                            path,
                            content: pins,
                        }),
                        Err(error) => failures.push(format!("{}: {error:#}", source.package_base)),
                    }
                }
                Err(error) => {
                    if error
                        .downcast_ref::<crate::infra::exec::ExecError>()
                        .is_some_and(crate::infra::exec::ExecError::is_cancelled)
                    {
                        return Err(error);
                    }
                    failures.push(format!(
                        "{} release refresh failed; existing pins retained: {error:#}",
                        source.package_base
                    ));
                }
            }
        }
        let cache = releases.into_cache_writes();
        if files.is_empty() && failures.is_empty() && cache.is_empty() {
            Ok(OperationState::Complete)
        } else {
            Ok(OperationState::needs_run(ReleasePlan {
                files,
                failures,
                cache,
            }))
        }
    }

    fn preview(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        for pins in &plan.files {
            ctx.log().debug(format!("write {}", pins.path.display()));
            ctx.log().action(
                "update",
                &pins.recipe,
                true,
                &format!("update {}", pins.recipe),
            );
        }
        report_failures(ctx, &plan.failures);
        Ok(TaskStats::from_counts(
            u32::try_from(plan.files.len())?,
            0,
            0,
            u32::try_from(plan.failures.len())?,
        )
        .finish())
    }

    fn apply(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        check_publication(ctx)?;
        plan.cache.persist();
        let mut failures = plan.failures.clone();
        let mut changed = 0;
        for pins in &plan.files {
            check_publication(ctx)?;
            let path = &pins.path;
            match write_atomic(path, &pins.content) {
                Ok(()) => {
                    changed += 1;
                    ctx.log().debug(format!("write {}", path.display()));
                    ctx.log().action(
                        "update",
                        &pins.recipe,
                        false,
                        &format!("update {}", pins.recipe),
                    );
                }
                Err(error) => failures.push(format!("writing {}: {error:#}", path.display())),
            }
        }
        report_failures(ctx, &failures);
        Ok(TaskStats::from_counts(changed, 0, 0, u32::try_from(failures.len())?).finish())
    }
}

pub(super) fn release_pins(source: &ReleaseSource, raw: &str) -> Result<String> {
    source.validate()?;
    let release: Release = serde_json::from_str(raw).context("parsing GitHub release")?;
    ensure!(
        !release.draft && !release.prerelease,
        "expected a stable published release"
    );
    let version = if let Some(prefix) = &source.tag_prefix {
        release
            .tag_name
            .strip_prefix(prefix)
            .context("release tag does not match configured prefix")?
    } else {
        release
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&release.tag_name)
    };
    ensure!(
        safe_component(version)
            && version.bytes().next().is_some_and(|b| b.is_ascii_digit())
            && !version.contains('-'),
        "unsupported release version"
    );
    let mut content = format!(
        "# Generated by dotfiles custom-package-versions; sourced by PKGBUILD.\n# shellcheck shell=bash\n# shellcheck disable=SC2034\npkgver='{version}'\n"
    );
    for (arch, name) in &source.assets {
        #[expect(
            clippy::literal_string_with_formatting_args,
            reason = "release descriptors use a literal version placeholder"
        )]
        let name = name.replace("{version}", version);
        let asset = release
            .assets
            .iter()
            .find(|a| a.name == name)
            .with_context(|| format!("release asset missing: {name}"))?;
        let digest = asset
            .digest
            .as_deref()
            .and_then(|s| s.strip_prefix("sha256:"))
            .with_context(|| format!("SHA-256 digest missing for {name}"))?;
        ensure!(
            digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid SHA-256 digest for {name}"
        );
        writeln!(content, "sha256sums_{arch}=('{digest}')")?;
    }
    Ok(content)
}
