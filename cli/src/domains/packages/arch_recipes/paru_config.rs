//! Paru local-repository configuration and snapshot publication.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, ensure};

use super::release_updates::UpdateCustomPackageVersions;
use super::snapshots::Snapshot;
use super::{
    ReleaseSource, check_publication, map_recipes, read_optional, recipes, report_failures,
};
use crate::engine::{
    Context, Operation, OperationState, Task, TaskResult, TaskStats, process_operation,
    task_metadata,
};
use crate::infra::logging::OutputExt as _;

crate::engine::config_task! {
    /// Generate Paru configuration from the selected managed base.
    pub(crate) struct ConfigureParu(Option<ParuBase>);
}

/// Selected base file and its owning managed source tree.
#[derive(Debug, Clone)]
pub(crate) struct ParuBase {
    /// Source file selected by the symlink declaration.
    pub path: PathBuf,
    /// Owning repository's symlinks directory.
    pub root: PathBuf,
}

impl Task for ConfigureParu {
    task_metadata! { name: "Custom package repository", selector: "custom-package-repository", }

    fn ordering_dependencies(&self) -> &[crate::engine::TaskId] {
        const DEPS: &[crate::engine::TaskId] =
            &[crate::engine::TaskId::Type(std::any::TypeId::of::<
                UpdateCustomPackageVersions,
            >())];
        DEPS
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform().supports_aur()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let base = self
            .config
            .get()
            .as_ref()
            .context("no managed source selected for .config/paru/paru.conf.base")?;
        let root =
            crate::infra::fs::canonicalize(&base.root).context("resolving Paru source tree")?;
        let path =
            crate::infra::fs::canonicalize(&base.path).context("resolving paru.conf.base")?;
        ensure!(
            path.starts_with(root),
            "Paru base source resolves outside its managed source tree"
        );
        process_operation(ctx, &ParuConfig { base: &path })
    }
}

pub(super) struct ParuConfig<'a> {
    pub(super) base: &'a Path,
}

pub(super) struct ParuPlan {
    config: String,
    previous_config: ConfigBaseline,
    snapshots: Vec<SnapshotPlan>,
    config_path: PathBuf,
    failures: Vec<String>,
}

#[derive(PartialEq, Eq)]
struct ConfigBaseline {
    contents: Option<String>,
    link: Option<PathBuf>,
}

impl ConfigBaseline {
    fn read(path: &Path) -> Result<Self> {
        let link = crate::infra::fs::symlink_metadata_optional(path, "inspect Paru configuration")?
            .filter(std::fs::Metadata::is_symlink)
            .map(|_| std::fs::read_link(path))
            .transpose()?;
        Ok(Self {
            contents: read_optional(path)?,
            link,
        })
    }

    fn verify(&self, path: &Path) -> Result<()> {
        ensure!(
            Self::read(path)? == *self,
            "Paru configuration {} changed during preparation; preserve your preferences in paru.conf.base before retrying",
            path.display()
        );
        Ok(())
    }
}

struct SnapshotPlan {
    recipe: String,
    snapshot: Snapshot,
    source: ReleaseSource,
    changed: bool,
}

pub(super) fn build_root(ctx: &Context) -> Result<PathBuf> {
    let root = ctx
        .env()
        .var_os("AURDEST")
        .filter(|v| !v.is_empty())
        .map_or_else(
            || {
                ctx.env()
                    .var_os("XDG_CACHE_HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .unwrap_or_else(|| ctx.home().join(".cache"))
                    .join("paru/clone")
            },
            PathBuf::from,
        );
    ensure!(root.is_absolute(), "Paru build directory must be absolute");
    Ok(root)
}

// Use the same override precedence as Paru. Reject relative or empty overrides
// rather than publish a config whose meaning depends on the caller's cwd.
pub(super) fn paru_config_path(ctx: &Context) -> Result<PathBuf> {
    let path = if let Some(path) = ctx.env().var_os("PARU_CONF") {
        PathBuf::from(path)
    } else if let Some(path) = ctx.env().var_os("XDG_CONFIG_HOME") {
        let path = PathBuf::from(path);
        ensure!(path.is_absolute(), "XDG_CONFIG_HOME must be absolute");
        path.join("paru/paru.conf")
    } else {
        ctx.home().join(".config/paru/paru.conf")
    };
    ensure!(path.is_absolute(), "Paru config path must be absolute");
    Ok(path)
}

pub(super) fn config_path(path: &Path) -> Result<&str> {
    let text = path.to_str().context("Paru path is not UTF-8")?;
    ensure!(
        text.trim() == text && !text.contains(['\n', '\r', '#']),
        "unsupported character in Paru path"
    );
    Ok(text)
}

pub(super) fn repository_section(base: &str, path: &Path) -> Result<String> {
    Ok(format!(
        "\n[dotfiles-{base}]\nPath = {}\nGenerateSrcinfo\n",
        config_path(path)?
    ))
}

impl Operation for ParuConfig<'_> {
    type Plan = ParuPlan;

    fn current_state(&self, ctx: &Context) -> Result<OperationState<Self::Plan>> {
        let base = std::fs::read_to_string(self.base).context("reading paru.conf.base")?;
        let build_root = build_root(ctx)?;
        let mut content = format!(
            "# Generated by dotfiles from paru.conf.base.\n{base}\n[options]\nCloneDir = {}\n",
            config_path(&build_root)?
        );
        let mut snapshots = Vec::new();
        let mut failures = Vec::new();
        for (directory, source) in recipes(ctx)? {
            let prepared =
                Snapshot::plan(&build_root, &source.package_base, &directory, ctx.dry_run())
                    .and_then(|snapshot| {
                        Ok((snapshot.validated_metadata(&source)?.is_none(), snapshot))
                    });
            match prepared {
                Ok((changed, snapshot)) => {
                    content.push_str(&repository_section(&source.package_base, &snapshot.path)?);
                    snapshots.push(SnapshotPlan {
                        recipe: directory
                            .file_name()
                            .context("missing recipe name")?
                            .to_string_lossy()
                            .into_owned(),
                        snapshot,
                        source,
                        changed,
                    });
                }
                Err(error) => {
                    failures.push(format!("{} unavailable: {error:#}", source.package_base));
                }
            }
        }
        let config_path = paru_config_path(ctx)?;
        let previous_config = ConfigBaseline::read(&config_path)?;
        let snapshots_current = snapshots.iter().all(|prepared| !prepared.changed);
        if previous_config.contents.as_deref() == Some(&content)
            && snapshots_current
            && failures.is_empty()
        {
            return Ok(OperationState::Complete);
        }
        Ok(OperationState::needs_run(ParuPlan {
            config: content,
            config_path,
            previous_config,
            snapshots,
            failures,
        }))
    }

    fn preview(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        for prepared in &plan.snapshots {
            if !prepared.changed {
                continue;
            }
            ctx.log()
                .debug(format!("snapshot {}", prepared.snapshot.path.display()));
            ctx.log().action(
                "refresh",
                &prepared.recipe,
                true,
                &format!("refresh {}", prepared.recipe),
            );
        }
        let config_changed = plan.previous_config.contents.as_deref() != Some(&plan.config);
        if config_changed {
            ctx.log()
                .debug(format!("write {}", plan.config_path.display()));
            ctx.log().action(
                "update",
                "paru configuration",
                true,
                "update paru configuration",
            );
        }
        report_failures(ctx, &plan.failures);
        Ok(TaskStats::from_counts(
            u32::try_from(
                usize::from(config_changed)
                    + plan
                        .snapshots
                        .iter()
                        .filter(|prepared| prepared.changed)
                        .count(),
            )?,
            0,
            0,
            u32::try_from(plan.failures.len())?,
        )
        .finish())
    }

    fn apply(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        let mut content = plan.config.clone();
        let mut changed = 0;
        let mut failures = plan.failures.clone();
        let validations = map_recipes(ctx, &plan.snapshots, |prepared| {
            prepared.snapshot.validate_metadata(ctx, &prepared.source)
        });
        // Every validation has finished. An interrupted worker stops publication,
        // including earlier recipes whose validations happened to succeed.
        let validations: Vec<_> = validations
            .into_iter()
            .map(|result| match result {
                Err(error)
                    if error
                        .downcast_ref::<crate::infra::exec::ExecError>()
                        .is_some_and(crate::infra::exec::ExecError::is_cancelled) =>
                {
                    Err(error)
                }
                other => Ok(other),
            })
            .collect::<Result<_>>()?;
        check_publication(ctx)?;
        plan.previous_config.verify(&plan.config_path)?;
        for (prepared, validation) in plan.snapshots.iter().zip(validations) {
            check_publication(ctx)?;
            let snapshot = &prepared.snapshot;
            let result = validation.and_then(|srcinfo| {
                let write_inputs = snapshot.metadata_changed(&srcinfo) || snapshot.needs_write()?;
                if write_inputs {
                    snapshot.apply(&srcinfo)?;
                }
                snapshot.record_validation(&prepared.source, &srcinfo)?;
                Ok(write_inputs || prepared.changed)
            });
            match result {
                Ok(snapshot_changed) => {
                    if snapshot_changed {
                        changed += 1;
                        ctx.log()
                            .debug(format!("snapshot {}", snapshot.path.display()));
                        ctx.log().action(
                            "refresh",
                            &prepared.recipe,
                            false,
                            &format!("refresh {}", prepared.recipe),
                        );
                    }
                }
                Err(error) => {
                    let base = snapshot
                        .path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .context("invalid snapshot base")?;
                    content = content.replace(&repository_section(base, &snapshot.path)?, "");
                    failures.push(format!("{base} unavailable: {error:#}"));
                }
            }
        }
        check_publication(ctx)?;
        if plan.previous_config.contents.as_deref() == Some(&content) {
            plan.previous_config.verify(&plan.config_path)?;
        } else {
            let path = &plan.config_path;
            crate::infra::fs::write_atomic_checked(path, &content, || {
                plan.previous_config.verify(path)
            })?;
            changed += 1;
            ctx.log().debug(format!("write {}", path.display()));
            ctx.log().action(
                "update",
                "paru configuration",
                false,
                "update paru configuration",
            );
        }
        report_failures(ctx, &failures);
        if changed == 0 && failures.is_empty() {
            return Ok(TaskResult::Ok);
        }
        Ok(TaskStats::from_counts(changed, 0, 0, u32::try_from(failures.len())?).finish())
    }
}
