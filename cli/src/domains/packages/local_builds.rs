//! Local PKGBUILD repositories and release pin updates, independent of tool use.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::engine::{
    Context, Operation, OperationState, Task, TaskResult, TaskStats, process_operation,
    task_metadata,
};
use crate::infra::fs::write_atomic;
use crate::infra::github::{CacheWrites, Releases, github_token};
use crate::infra::logging::OutputExt as _;

/// Generate the machine-specific path to the checkout's PKGBUILD repository.
#[derive(Debug)]
pub struct ConfigureParu;

impl Task for ConfigureParu {
    task_metadata! { name: "Paru configuration", selector: "paru-config", }

    fn ordering_dependencies(&self) -> &[crate::engine::TaskId] {
        const DEPS: &[crate::engine::TaskId] =
            &[crate::engine::TaskId::Type(std::any::TypeId::of::<
                UpdateLocalPackageVersions,
            >())];
        DEPS
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform().supports_aur()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        process_operation(ctx, &ParuConfig)
    }
}

/// Evaluate independent recipes in bounded workers; publication stays ordered.
fn map_recipes<T: Sync, R: Send>(
    ctx: &Context,
    items: &[T],
    work: impl Fn(&T) -> Result<R> + Sync + Send,
) -> Result<Vec<Result<R>>> {
    use rayon::prelude::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    let cancelled = AtomicBool::new(false);
    let evaluate = |item: &T| {
        if ctx.is_cancelled() || cancelled.load(Ordering::Acquire) {
            return Err(crate::infra::exec::ExecError::Cancelled {
                command: "local package preparation".into(),
                result: crate::infra::exec::ExecResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: false,
                    code: None,
                },
            }
            .into());
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
        return Ok(items.iter().map(evaluate).collect());
    }
    let task_name = crate::infra::logging::log_thread_name();
    let pool = rayon::ThreadPoolBuilder::new().num_threads(4).build()?;
    Ok(pool.install(|| {
        items
            .par_iter()
            .map(|item| {
                crate::infra::logging::set_log_thread_name(&task_name);
                evaluate(item)
            })
            .collect()
    }))
}

mod snapshots;
use snapshots::Snapshot;

struct ParuConfig;

struct ParuPlan {
    config: String,
    previous_config: Option<String>,
    snapshots: Vec<SnapshotPlan>,
    config_path: PathBuf,
    failures: Vec<String>,
}

struct SnapshotPlan {
    snapshot: Snapshot,
    source: ReleaseSource,
    changed: bool,
}

fn build_root(ctx: &Context) -> Result<PathBuf> {
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
fn paru_config_path(ctx: &Context) -> Result<PathBuf> {
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

fn recipe_directories(ctx: &Context) -> Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(ctx.root().join("packages")) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context("reading local package recipe directory"),
    };
    let mut directories = Vec::new();
    for entry in entries {
        let entry = entry.context("reading local package recipe entry")?;
        if entry.file_type()?.is_dir() {
            directories.push(entry.path());
        }
    }
    directories.sort();
    Ok(directories)
}

fn config_path(path: &Path) -> Result<&str> {
    let text = path.to_str().context("Paru path is not UTF-8")?;
    ensure!(
        text.trim() == text && !text.contains(['\n', '\r', '#']),
        "unsupported character in Paru path"
    );
    Ok(text)
}

impl Operation for ParuConfig {
    type Plan = ParuPlan;

    fn current_state(&self, ctx: &Context) -> Result<OperationState<Self::Plan>> {
        let base = std::fs::read_to_string(ctx.symlinks_dir().join("config/paru/paru.conf.base"))
            .context("reading paru.conf.base")?;
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
                    .and_then(|snapshot| Ok((snapshot.needs_write()?, snapshot)));
            match prepared {
                Ok((changed, snapshot)) => {
                    content.push_str(&repository_section(&source.package_base, &snapshot.path)?);
                    snapshots.push(SnapshotPlan {
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
        let previous_config = read_optional(&config_path)?;
        if previous_config.as_deref() == Some(&content)
            && snapshots.is_empty()
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
            let snapshot = &prepared.snapshot;
            if !prepared.changed {
                continue;
            }
            ctx.log().dry_run(format!(
                "refresh local package snapshot {}",
                snapshot.path.display()
            ));
        }
        let config_changed = plan.previous_config.as_deref() != Some(&plan.config);
        if config_changed {
            ctx.log()
                .dry_run(format!("write {}", plan.config_path.display()));
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
        })?;
        for (prepared, validation) in plan.snapshots.iter().zip(validations) {
            let snapshot = &prepared.snapshot;
            let result = validation.and_then(|srcinfo| {
                let snapshot_changed = prepared.changed || snapshot.metadata_changed(&srcinfo);
                if snapshot_changed {
                    snapshot.apply(&srcinfo)?;
                }
                snapshot.record_validation(&prepared.source, &srcinfo)?;
                Ok(snapshot_changed)
            });
            match result {
                Ok(snapshot_changed) => {
                    if snapshot_changed {
                        changed += 1;
                        ctx.log().info(format!(
                            "refresh local package snapshot {}",
                            snapshot.path.display()
                        ));
                    }
                }
                Err(error) => {
                    if error
                        .downcast_ref::<crate::infra::exec::ExecError>()
                        .is_some_and(crate::infra::exec::ExecError::is_cancelled)
                    {
                        return Err(error);
                    }
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
        if plan.previous_config.as_deref() != Some(&content) {
            let path = &plan.config_path;
            write_atomic(path, &content)?;
            changed += 1;
            ctx.log().info(format!("write {}", path.display()));
        }
        report_failures(ctx, &failures);
        if changed == 0 && failures.is_empty() {
            return Ok(TaskResult::Ok);
        }
        Ok(TaskStats::from_counts(changed, 0, 0, u32::try_from(failures.len())?).finish())
    }
}

/// Refresh release pins for the tracked local recipe directories.
#[derive(Debug)]
pub struct UpdateLocalPackageVersions;

impl Task for UpdateLocalPackageVersions {
    task_metadata! {
        name: "Local package versions",
        selector: "local-package-versions",
        update_only: true,
    }

    fn ordering_dependencies(&self) -> &[crate::engine::TaskId] {
        const DEPS: &[crate::engine::TaskId] =
            &[crate::engine::TaskId::Type(std::any::TypeId::of::<
                super::install::InstallPackages,
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

struct ReleaseUpdates;

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
            "invalid local package base"
        );
        ensure!(
            !self.package_names().is_empty(),
            "local recipe has no package names"
        );
        for name in self.package_names() {
            ensure!(safe_component(name), "invalid local package name");
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

struct ReleasePlan {
    files: Vec<(PathBuf, String)>,
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
                        Ok(_) => files.push((path, pins)),
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
        for (path, _) in &plan.files {
            ctx.log().dry_run(format!("write {}", path.display()));
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
        plan.cache.persist();
        let mut failures = plan.failures.clone();
        let mut changed = 0;
        for (path, content) in &plan.files {
            match write_atomic(path, content) {
                Ok(()) => {
                    changed += 1;
                    ctx.log().info(format!("write {}", path.display()));
                }
                Err(error) => failures.push(format!("writing {}: {error:#}", path.display())),
            }
        }
        report_failures(ctx, &failures);
        Ok(TaskStats::from_counts(changed, 0, 0, u32::try_from(failures.len())?).finish())
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

fn release_pins(source: &ReleaseSource, raw: &str) -> Result<String> {
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
        "# Generated by dotfiles local-package-versions; sourced by PKGBUILD.\n# shellcheck shell=bash\n# shellcheck disable=SC2034\npkgver='{version}'\n"
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

fn repository_section(base: &str, path: &Path) -> Result<String> {
    Ok(format!(
        "\n[dotfiles-{base}]\nPath = {}\nGenerateSrcinfo\n",
        config_path(path)?
    ))
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
                "local recipe is missing release.json: {}",
                directory.display()
            );
            continue;
        };
        let source: ReleaseSource = serde_json::from_str(&raw)
            .with_context(|| format!("parsing {}", directory.join("release.json").display()))?;
        source.validate()?;
        ensure!(
            bases.insert(source.package_base.clone()),
            "duplicate local package base"
        );
        for name in source.package_names() {
            ensure!(
                names.insert(name.clone()),
                "duplicate local package name: {name}"
            );
        }
        recipes.push((directory, source));
    }
    Ok(recipes)
}

/// Unavailable locally owned package names; never send these to an AUR lookup.
/// Preview accepts inputs that prerequisite tasks would prepare without writing.
pub(super) fn unavailable_packages(ctx: &Context) -> Result<BTreeMap<String, String>> {
    let recipes = recipes(ctx)?;
    if recipes.is_empty() {
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
            for name in source.package_names() {
                unavailable.insert(name.clone(), format!("{error:#}"));
            }
        }
    }
    Ok(unavailable)
}

#[cfg(test)]
mod tests;
