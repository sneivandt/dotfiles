use super::*;
use std::sync::Arc;

use crate::infra::exec::{ExecResult, MockExecutor};
use crate::infra::platform::{Os, Platform};
use crate::test_helpers::{ContextBuilder, make_context, task_batch};

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

#[test]
fn release_validation_rejects_incomplete_or_unsafe_metadata() {
    let valid: serde_json::Value = serde_json::from_str(&release()).unwrap();
    for (label, pointer, value) in [
        ("missing asset", "/assets", serde_json::json!([])),
        (
            "missing digest",
            "/assets/0/digest",
            serde_json::Value::Null,
        ),
        (
            "bad digest",
            "/assets/0/digest",
            serde_json::json!("sha256:bad"),
        ),
        ("draft", "/draft", serde_json::json!(true)),
        ("prerelease", "/prerelease", serde_json::json!(true)),
        (
            "shell injection",
            "/tag_name",
            serde_json::json!("v1';touch injected;#"),
        ),
    ] {
        let mut data = valid.clone();
        *data.pointer_mut(pointer).unwrap() = value;
        assert!(
            release_pins(&source(), &data.to_string()).is_err(),
            "{label}"
        );
    }
    let pins = release_pins(&source(), &release()).unwrap();
    assert!(pins.contains("pkgver='1.2.3'\n"));
    assert!(pins.contains(&format!("sha256sums_x86_64=('{}')", "a".repeat(64))));
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

fn task() -> UpdateLocalPackageVersions {
    UpdateLocalPackageVersions
}

fn release_context(root: &Path, fail: bool) -> Context {
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .once()
        .withf(|spec| {
            spec.program() == "curl"
                && spec.is_checked()
                && spec.arguments().last().unwrap()
                    == "https://api.github.com/repos/example/tool/releases/latest"
        })
        .returning(move |_| {
            if fail {
                return Err(crate::infra::exec::ExecError::spawn(
                    "curl",
                    std::io::Error::other("fixture network failure"),
                ));
            }
            Ok(ExecResult::success(release()))
        });
    make_context(
        root.into(),
        None,
        Platform::new(Os::Linux, true),
        Arc::new(mock),
    )
}

#[test]
fn release_update_preview_failure_apply_and_repeat() {
    let dir = fixture();
    let target = dir.path().join("packages/tool-bin/version.sh");
    let preview = task()
        .run(&release_context(dir.path(), false).with_dry_run(true))
        .unwrap();
    assert_eq!(task_batch(&preview).changed_count(), 1);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "original pins\n");
    assert!(task().run(&release_context(dir.path(), true)).is_err());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "original pins\n");
    let result = task().run(&release_context(dir.path(), false)).unwrap();
    assert_eq!(task_batch(&result).changed_count(), 1);
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        release_pins(&source(), &release()).unwrap()
    );
    let repeated = task().run(&release_context(dir.path(), false)).unwrap();
    assert!(matches!(repeated, TaskResult::Ok));
}

#[test]
fn fresh_checkout_generates_pins_only_when_applied() {
    let dir = fixture();
    let target = dir.path().join("packages/tool-bin/version.sh");
    std::fs::remove_file(&target).unwrap();
    let preview = task()
        .run(&release_context(dir.path(), false).with_dry_run(true))
        .unwrap();
    assert_eq!(task_batch(&preview).changed_count(), 1);
    assert!(!target.exists());
    let applied = task().run(&release_context(dir.path(), false)).unwrap();
    assert_eq!(task_batch(&applied).changed_count(), 1);
    assert_eq!(
        std::fs::read_to_string(target).unwrap(),
        release_pins(&source(), &release()).unwrap()
    );
}

#[test]
fn recipe_discovery_and_platform_control_release_updates() {
    let dir = fixture();
    let ctx = ContextBuilder::new(dir.path().into()).arch(true).build();
    let empty = tempfile::tempdir().unwrap();
    let empty_ctx = ContextBuilder::new(empty.path().into()).arch(true).build();
    assert!(matches!(task().run(&empty_ctx).unwrap(), TaskResult::Ok));
    assert!(task().should_run(&ctx));
    assert!(!task().should_run(&ContextBuilder::new(dir.path().into()).build()));
    assert!(
        !ConfigureParu.should_run(
            &ContextBuilder::new(dir.path().into())
                .os(Os::Windows)
                .build()
        )
    );
    assert!(task().update_only());
    std::fs::write(
        dir.path().join("packages/tool-bin/release.json"),
        "invalid json",
    )
    .unwrap();
    assert!(task().run(&ctx).is_err());
}

#[test]
fn config_preview_apply_and_repeat() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ContextBuilder::new(dir.path().join("repo with spaces"))
        .arch(true)
        .build()
        .with_home(dir.path().join("home"));
    let base = ctx.symlinks_dir().join("config/paru/paru.conf.base");
    std::fs::create_dir_all(base.parent().unwrap()).unwrap();
    std::fs::write(&base, "[options]\nDevel\n").unwrap();
    let target = ctx.home().join(".config/paru/paru.conf");
    let preview = ConfigureParu.run(&ctx.with_dry_run(true)).unwrap();
    assert_eq!(task_batch(&preview).changed_count(), 1);
    assert!(!target.exists());
    assert_eq!(
        task_batch(&ConfigureParu.run(&ctx).unwrap()).changed_count(),
        1
    );
    let config = std::fs::read_to_string(&target).unwrap();
    assert!(config.contains(&format!(
        "CloneDir = {}\n",
        build_root(&ctx).unwrap().display()
    )));
    assert!(config.contains("[options]\nDevel\n"));
    assert!(matches!(ConfigureParu.run(&ctx).unwrap(), TaskResult::Ok));
    std::fs::write(base, "[options]\nBottomUp\n").unwrap();
    assert_eq!(
        task_batch(&ConfigureParu.run(&ctx).unwrap()).changed_count(),
        1
    );
}

#[cfg(unix)]
#[test]
fn config_migrates_old_link_without_overwriting_tracked_source() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ContextBuilder::new(dir.path().into())
        .arch(true)
        .build()
        .with_home(dir.path().join("home"));
    let base = ctx.symlinks_dir().join("config/paru/paru.conf.base");
    std::fs::create_dir_all(base.parent().unwrap()).unwrap();
    std::fs::write(&base, "[options]\nDevel\n").unwrap();
    let target = ctx.home().join(".config/paru/paru.conf");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&base, &target).unwrap();
    assert_eq!(
        task_batch(&ConfigureParu.run(&ctx).unwrap()).changed_count(),
        1
    );
    assert!(!std::fs::symlink_metadata(target).unwrap().is_symlink());
    assert_eq!(std::fs::read_to_string(base).unwrap(), "[options]\nDevel\n");
}

fn snapshot_context(root: &Path) -> Context {
    let ctx = ContextBuilder::new(root.into())
        .arch(true)
        .build()
        .with_home(root.join("home"))
        .with_env(crate::infra::env::MapEnv::new().into_handle());
    let base = ctx.symlinks_dir().join("config/paru/paru.conf.base");
    std::fs::create_dir_all(base.parent().unwrap()).unwrap();
    std::fs::write(base, "[options]\nCleanAfter\n").unwrap();
    ctx
}

#[test]
fn snapshot_is_disposable_and_commits_inputs_for_git_cleanup() {
    let dir = fixture();
    let ctx = snapshot_context(dir.path());
    let cache = build_root(&ctx).unwrap().join("tool-bin");
    let preview = ConfigureParu.run(&ctx.with_dry_run(true)).unwrap();
    assert_eq!(task_batch(&preview).changed_count(), 2);
    assert!(!cache.exists());
    let applied = ConfigureParu.run(&ctx).unwrap();
    assert_eq!(task_batch(&applied).changed_count(), 2);
    let repo = git2::Repository::open(&cache).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    let tree = head.tree().unwrap();
    let blob = repo
        .find_blob(tree.get_name("version.sh").unwrap().id())
        .unwrap();
    assert_eq!(blob.content(), b"original pins\n");
    let config = std::fs::read_to_string(ctx.home().join(".config/paru/paru.conf")).unwrap();
    assert!(config.contains("\nCleanAfter\n"));
    assert!(config.contains(&format!(
        "[dotfiles-tool-bin]\nPath = {}\nGenerateSrcinfo",
        cache.display()
    )));
    assert!(matches!(ConfigureParu.run(&ctx).unwrap(), TaskResult::Ok));

    let source = dir.path().join("packages/tool-bin/PKGBUILD");
    std::fs::write(&source, "modified tracked source\n").unwrap();
    // Resetting the standalone cache cannot reset the source checkout.
    std::fs::write(cache.join("PKGBUILD"), "modified cache\n").unwrap();
    repo.reset(head.as_object(), git2::ResetType::Hard, None)
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&source).unwrap(),
        "modified tracked source\n"
    );
    std::fs::write(cache.join("PKGBUILD"), "modified tracked source\n").unwrap();
    let refreshed = ConfigureParu.run(&ctx).unwrap();
    assert_eq!(task_batch(&refreshed).changed_count(), 1);
    assert_ne!(repo.head().unwrap().target().unwrap(), head.id());
    assert!(matches!(ConfigureParu.run(&ctx).unwrap(), TaskResult::Ok));
}

#[test]
fn snapshot_preview_handles_missing_pins_and_refuses_unmanaged_cache() {
    let dir = fixture();
    let ctx = snapshot_context(dir.path());
    let pins = dir.path().join("packages/tool-bin/version.sh");
    std::fs::remove_file(&pins).unwrap();
    let preview = ConfigureParu.run(&ctx.with_dry_run(true)).unwrap();
    assert_eq!(task_batch(&preview).changed_count(), 2);
    assert!(!build_root(&ctx).unwrap().exists());
    assert!(ConfigureParu.run(&ctx).is_err());
    std::fs::write(pins, "pins\n").unwrap();
    let cache = build_root(&ctx).unwrap().join("tool-bin");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("sentinel"), "unmanaged\n").unwrap();
    assert!(ConfigureParu.run(&ctx).is_err());
    assert_eq!(
        std::fs::read_to_string(cache.join("sentinel")).unwrap(),
        "unmanaged\n"
    );
    assert!(!cache.join(".git").exists());
}

#[test]
fn build_cache_respects_injected_xdg_and_aurdest() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = snapshot_context(dir.path());
    let cache = dir.path().join("custom-cache");
    let env = crate::infra::env::MapEnv::new().with("XDG_CACHE_HOME", &cache);
    assert_eq!(
        build_root(&ctx.with_env(env.clone().into_handle())).unwrap(),
        cache.join("paru/clone")
    );
    let dest = dir.path().join("aurdest");
    assert_eq!(
        build_root(&ctx.with_env(env.with("AURDEST", &dest).into_handle())).unwrap(),
        dest
    );
}

#[test]
fn tracked_recipe_descriptors_validate_without_network_or_generated_pins() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let ctx = ContextBuilder::new(root.into()).build();
    let directories = recipe_directories(&ctx).unwrap();
    assert!(!directories.is_empty(), "expected tracked local recipes");
    let mut validated = 0;
    for directory in directories {
        let Some(raw) = read_optional(&directory.join("release.json")).unwrap() else {
            assert!(
                !directory.join("PKGBUILD").exists(),
                "missing release descriptor: {}",
                directory.display()
            );
            continue;
        };
        validated += 1;
        let descriptor: ReleaseSource = serde_json::from_str(&raw).unwrap();
        descriptor.validate().unwrap();
        assert!(
            directory.join("PKGBUILD").is_file(),
            "missing recipe: {}",
            directory.display()
        );
    }
    assert!(validated > 0, "expected at least one release descriptor");
}

#[test]
fn invalid_descriptors_do_not_query_or_publish_snapshots() {
    for (label, field, value) in [
        (
            "repository path",
            "repository",
            serde_json::json!("example/tool/extra"),
        ),
        ("empty assets", "assets", serde_json::json!({})),
        (
            "architecture shell syntax",
            "assets",
            serde_json::json!({"x86_64;echo":"tool.tar.gz"}),
        ),
        (
            "empty asset name",
            "assets",
            serde_json::json!({"x86_64":""}),
        ),
    ] {
        let dir = fixture();
        let ctx = snapshot_context(dir.path());
        let path = dir.path().join("packages/tool-bin/release.json");
        let mut descriptor: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        descriptor[field] = value;
        std::fs::write(path, descriptor.to_string()).unwrap();
        // The executor has no process expectations: discovery must reject
        // invalid metadata before making a network request.
        assert!(
            task().run(&ctx).is_err(),
            "{label}: release discovery accepted invalid input"
        );
        assert!(
            ConfigureParu.run(&ctx).is_err(),
            "{label}: snapshot preparation accepted invalid input"
        );
        assert!(
            !build_root(&ctx).unwrap().exists(),
            "{label}: cache was mutated"
        );
        assert!(
            !ctx.home().join(".config/paru/paru.conf").exists(),
            "{label}: config was published"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("packages/tool-bin/version.sh")).unwrap(),
            "original pins\n",
            "{label}: pins were changed"
        );
    }
}

#[test]
fn unreadable_descriptor_is_an_error_not_an_absent_recipe() {
    let dir = fixture();
    let ctx = snapshot_context(dir.path());
    let descriptor = dir.path().join("packages/tool-bin/release.json");
    std::fs::remove_file(&descriptor).unwrap();
    std::fs::create_dir(&descriptor).unwrap();
    assert!(task().run(&ctx).is_err());
    assert!(ConfigureParu.run(&ctx).is_err());
    assert!(!build_root(&ctx).unwrap().exists());
    assert!(!ctx.home().join(".config/paru/paru.conf").exists());
}

#[test]
fn recipe_directory_query_failure_does_not_publish_empty_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = snapshot_context(dir.path());
    std::fs::write(dir.path().join("packages"), "not a directory\n").unwrap();
    assert!(task().run(&ctx).is_err());
    assert!(ConfigureParu.run(&ctx).is_err());
    assert!(!ctx.home().join(".config/paru/paru.conf").exists());
}

#[test]
fn interrupted_snapshot_initialization_recovers_an_unborn_head() {
    let dir = fixture();
    let ctx = snapshot_context(dir.path());
    assert_eq!(
        task_batch(&ConfigureParu.run(&ctx).unwrap()).changed_count(),
        2
    );
    let cache = build_root(&ctx).unwrap().join("tool-bin");
    let repo = git2::Repository::open(&cache).unwrap();
    let branch = repo.head().unwrap().name().unwrap().to_owned();
    std::fs::remove_file(repo.path().join(branch)).unwrap();
    assert!(repo.head().is_err());
    let recovered = ConfigureParu.run(&ctx).unwrap();
    assert_eq!(task_batch(&recovered).changed_count(), 1);
    assert!(repo.head().unwrap().peel_to_commit().is_ok());
    assert!(matches!(ConfigureParu.run(&ctx).unwrap(), TaskResult::Ok));
}

#[test]
fn corrupt_snapshot_head_fails_before_preview_or_cached_input_changes() {
    let dir = fixture();
    let ctx = snapshot_context(dir.path());
    assert_eq!(
        task_batch(&ConfigureParu.run(&ctx).unwrap()).changed_count(),
        2
    );
    let source = dir.path().join("packages/tool-bin");
    let cache = build_root(&ctx).unwrap().join("tool-bin");
    let original = std::fs::read_to_string(cache.join("PKGBUILD")).unwrap();
    std::fs::write(source.join("PKGBUILD"), "new recipe inputs\n").unwrap();
    let plan = Snapshot::plan(&build_root(&ctx).unwrap(), "tool-bin", &source, false).unwrap();
    let repo = git2::Repository::open(&cache).unwrap();
    let branch = repo.head().unwrap().name().unwrap().to_owned();
    std::fs::write(repo.path().join(branch), "corrupted reference\n").unwrap();
    assert!(repo.head().is_err());
    assert!(ConfigureParu.run(&ctx.with_dry_run(true)).is_err());
    assert!(plan.apply().is_err());
    assert_eq!(
        std::fs::read_to_string(cache.join("PKGBUILD")).unwrap(),
        original
    );
    assert_eq!(
        std::fs::read_to_string(cache.join("version.sh")).unwrap(),
        "original pins\n"
    );
}
