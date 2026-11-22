use super::*;
use std::sync::Arc;

use crate::engine::scheduler::run_tasks;
use crate::infra::exec::{ExecResult, MockExecutor};
use crate::infra::github::tests::MockHttpClient;
use crate::infra::platform::{Os, Platform};
use crate::test_helpers::{ContextBuilder, make_context, task_batch};
use std::time::Duration;

fn paru_task(ctx: &Context) -> ConfigureParu {
    ConfigureParu::new(crate::infra::ConfigHandle::new(Some(ParuBase {
        path: ctx.symlinks_dir().join("config/paru/paru.conf.base"),
        root: ctx.symlinks_dir().to_path_buf(),
    })))
}

fn configure_paru(ctx: &Context) -> Result<TaskResult> {
    paru_task(ctx).run(ctx)
}

fn source() -> ReleaseSource {
    serde_json::from_str(r#"{"package_base":"tool-bin","repository":"example/tool","assets":{"x86_64":"tool.tar.gz"}}"#)
        .unwrap()
}

fn release() -> String {
    serde_json::json!({
        "tag_name": "v1.2.3", "draft": false, "prerelease": false,
        "assets": [{"name":"tool.tar.gz", "digest": format!("sha256:{}", "a".repeat(64))}]
    })
    .to_string()
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("packages/tool-bin");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join("release.json"),
        r#"{"package_base":"tool-bin","repository":"example/tool","assets":{"x86_64":"tool.tar.gz"}}"#,
    )
    .unwrap();
    std::fs::write(
        package.join("PKGBUILD"),
        "pkgname=tool-bin\npkgver=1\npkgrel=1\narch=('any')\n",
    )
    .unwrap();
    std::fs::write(package.join("version.sh"), "original pins\n").unwrap();
    dir
}

fn task() -> UpdateCustomPackageVersions {
    UpdateCustomPackageVersions
}

fn expect_metadata(mock: &mut MockExecutor) {
    mock.expect_execute()
        .withf(|spec| {
            spec.program() == "makepkg"
                && spec.arguments() == ["--printsrcinfo"]
                && spec.is_checked()
        })
        .returning(|spec| {
            let dir = spec.working_dir().unwrap();
            let recipe = std::fs::read_to_string(dir.join("PKGBUILD")).unwrap();
            assert!(dir.join("version.sh").is_file());
            let name = recipe
                .lines()
                .find_map(|line| line.strip_prefix("pkgname="))
                .unwrap_or("tool-bin");
            Ok(ExecResult::success(format!(
                "pkgbase = {name}\n\tarch = x86_64\npkgname = {name}\n"
            )))
        });
}

fn metadata_executor() -> MockExecutor {
    let mut mock = MockExecutor::new();
    expect_metadata(&mut mock);
    mock
}

fn arch_context(root: &Path, mock: MockExecutor) -> Context {
    make_context(
        root.into(),
        None,
        Platform::new(Os::Linux, true),
        Arc::new(mock),
    )
    .with_home(root.join("home"))
    .with_env(crate::infra::env::MapEnv::new().into_handle())
}

fn snapshot_context(root: &Path, mock: MockExecutor) -> Context {
    let ctx = arch_context(root, mock);
    let base = ctx.symlinks_dir().join("config/paru/paru.conf.base");
    std::fs::create_dir_all(base.parent().unwrap()).unwrap();
    std::fs::write(base, "[options]\nCleanAfter\n").unwrap();
    ctx
}

struct SnapshotFixture {
    dir: tempfile::TempDir,
    source: PathBuf,
    cache: PathBuf,
    ctx: Context,
}

impl SnapshotFixture {
    fn new() -> Self {
        Self::with_executor(metadata_executor())
    }

    fn with_executor(mock: MockExecutor) -> Self {
        let dir = fixture();
        let source = dir.path().join("packages/tool-bin");
        let ctx = snapshot_context(dir.path(), mock);
        let cache = build_root(&ctx).unwrap().join("tool-bin");
        Self {
            dir,
            source,
            cache,
            ctx,
        }
    }

    fn plan(&self, preview: bool) -> Snapshot {
        Snapshot::plan(
            self.cache.parent().unwrap(),
            "tool-bin",
            &self.source,
            preview,
        )
        .unwrap()
    }

    fn configure(&self) -> TaskResult {
        configure_paru(&self.ctx).unwrap()
    }

    fn preview(&self) -> TaskResult {
        configure_paru(&self.ctx.with_dry_run(true)).unwrap()
    }

    fn repo(&self) -> git2::Repository {
        git2::Repository::open(&self.cache).unwrap()
    }

    fn commit_index(&self, message: &str, edit: impl FnOnce(&mut git2::Index)) {
        let repo = self.repo();
        let mut index = repo.index().unwrap();
        edit(&mut index);
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        commit_tree(&repo, message, &tree);
    }
}

fn commit_tree(repo: &git2::Repository, message: &str, tree: &git2::Tree<'_>) {
    let parent = repo.head().unwrap().peel_to_commit().unwrap();
    let signature = git2::Signature::now("fixture", "ci@test.local").unwrap();
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        message,
        tree,
        &[&parent],
    )
    .unwrap();
}

fn add_recipe(root: &Path, base: &str) -> PathBuf {
    let directory = root.join("packages").join(base);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("release.json"),
        serde_json::json!({
            "package_base": base,  "repository": format!("example/{base}"),
            "assets": {"x86_64": "tool.tar.gz"}
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        directory.join("PKGBUILD"),
        format!("pkgname={base}\nsource ./version.sh\n"),
    )
    .unwrap();
    directory
}

mod configuration;
mod readiness;
mod releases;
mod snapshots;
mod validation;
