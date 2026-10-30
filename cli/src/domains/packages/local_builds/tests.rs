use super::*;
use std::sync::Arc;

use crate::infra::exec::{ExecResult, MockExecutor};
use crate::infra::github::tests::MockHttpClient;
use crate::infra::platform::{Os, Platform};
use crate::test_helpers::{ContextBuilder, make_context, task_batch};
use std::time::Duration;

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

#[test]
fn configured_tag_prefix_is_required_and_does_not_bypass_version_validation() {
    let mut source = source();
    source.tag_prefix = Some("rust-v".into());
    let mut data: serde_json::Value = serde_json::from_str(&release()).unwrap();
    for tag in [
        "rust-v1.2.3",
        "v1.2.3",
        "1.2.3",
        "rust-v1.2.3-alpha.1",
        "rust-v1';echo injected;#",
    ] {
        data["tag_name"] = serde_json::json!(tag);
        let result = release_pins(&source, &data.to_string());
        if tag == "rust-v1.2.3" {
            assert!(result.unwrap().contains("pkgver='1.2.3'\n"));
        } else {
            assert!(result.is_err(), "unexpected accepted tag: {tag}");
        }
    }
    source.tag_prefix = Some("../".into());
    assert!(source.validate().is_err());
    source.tag_prefix = Some(String::new());
    assert!(source.validate().is_err());
}

#[test]
fn versioned_assets_use_the_validated_release_version_and_exact_digest() {
    let mut source = source();
    source.assets.insert(
        "x86_64".into(),
        "powershell-{version}-linux-x64.tar.gz".into(),
    );
    for tag in ["v1.2.3", "1.2.3"] {
        let data = serde_json::json!({
            "tag_name": tag, "draft": false, "prerelease": false,
            "assets": [
                {"name": "powershell-1.2.2-linux-x64.tar.gz", "digest": format!("sha256:{}", "b".repeat(64))},
                {"name": "powershell-1.2.3-linux-x64.tar.gz", "digest": format!("sha256:{}", "a".repeat(64))}
            ]
        });
        let pins = release_pins(&source, &data.to_string()).unwrap();
        assert!(pins.contains(&format!("sha256sums_x86_64=('{}')", "a".repeat(64))));

        let mut missing = data;
        missing["assets"].as_array_mut().unwrap().pop();
        assert!(release_pins(&source, &missing.to_string()).is_err());
        missing["tag_name"] = serde_json::json!("v1';touch injected;#");
        assert!(release_pins(&source, &missing.to_string()).is_err());
    }
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
    mock.expect_which()
        .withf(|program| program == "gh")
        .return_const(false);
    let response = if fail {
        Err(anyhow::anyhow!("fixture network failure"))
    } else {
        Ok(release().into_bytes())
    };
    make_context(
        root.into(),
        None,
        Platform::new(Os::Linux, true),
        Arc::new(mock),
    )
    .with_env(crate::infra::env::MapEnv::new().into_handle())
    .with_http(Arc::new(MockHttpClient::new(vec![response])))
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
    let warnings = Arc::new(crate::test_helpers::CapturingOutput::default());
    let failure_ctx = release_context(dir.path(), true).with_log(Arc::<
        crate::test_helpers::CapturingOutput,
    >::clone(&warnings));
    assert_eq!(
        task_batch(&task().run(&failure_ctx).unwrap()).failed_count(),
        1
    );
    assert!(
        warnings
            .messages_of(crate::infra::logging::MsgKind::Warn)
            .iter()
            .any(|message| {
                message.contains("tool-bin release refresh failed; existing pins retained")
                    && message.contains("fixture network failure")
            }),
        "a failed refresh must retain its recipe and cause in diagnostics"
    );
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
fn unchanged_pins_still_persist_fetched_metadata_without_reporting_changes() {
    let dir = fixture();
    let target = dir.path().join("packages/tool-bin/version.sh");
    let pins = release_pins(&source(), &release()).unwrap();
    std::fs::write(&target, &pins).unwrap();
    let http = Arc::new(MockHttpClient::new(vec![Ok(release().into_bytes())]));
    let ctx = release_context(dir.path(), false).with_http(Arc::<MockHttpClient>::clone(&http));
    let cache = dir.path().join("bin/.github-releases-v1");
    assert!(!cache.exists());
    let result = task().run(&ctx).unwrap();
    assert_eq!(task_batch(&result).changed_count(), 0);
    assert_eq!(task_batch(&result).failed_count(), 0);
    assert!(
        cache.exists(),
        "unchanged pins must not discard pending metadata"
    );
    assert!(matches!(task().run(&ctx).unwrap(), TaskResult::Ok));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), pins);
    assert_eq!(
        http.request_urls().len(),
        1,
        "the second invocation uses the cache"
    );
}

#[test]
fn package_discovery_authenticates_deduplicates_and_preserves_preview_cache() {
    let dir = fixture();
    let other = add_recipe(dir.path(), "other-bin");
    let mut descriptor: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(other.join("release.json")).unwrap())
            .unwrap();
    descriptor["repository"] = serde_json::json!("example/tool");
    std::fs::write(other.join("release.json"), descriptor.to_string()).unwrap();
    let http = Arc::new(MockHttpClient::new(vec![
        Ok(release().into_bytes()),
        Ok(release().into_bytes()),
    ]));
    let ctx = make_context(
        dir.path().into(),
        None,
        Platform::new(Os::Linux, true),
        Arc::new(MockExecutor::new()),
    )
    .with_http(Arc::<MockHttpClient>::clone(&http))
    .with_env(
        crate::infra::env::MapEnv::new()
            .with("GH_TOKEN", "fixture-token")
            .into_handle(),
    );
    let cache = dir.path().join("bin/.github-releases-v1");
    let preview = task().run(&ctx.with_dry_run(true)).unwrap();
    assert_eq!(task_batch(&preview).changed_count(), 2);
    assert!(!cache.exists());
    assert!(!other.join("version.sh").exists());
    assert_eq!(task_batch(&task().run(&ctx).unwrap()).changed_count(), 2);
    assert!(cache.exists());
    assert!(matches!(task().run(&ctx).unwrap(), TaskResult::Ok));
    assert_eq!(
        http.request_urls().len(),
        2,
        "one request per invocation, none on warm repeat"
    );
    assert!(
        http.request_headers()
            .iter()
            .all(|headers| headers
                .contains(&("Authorization".into(), "Bearer fixture-token".into())))
    );
}

#[test]
fn package_rate_limit_stops_following_requests_and_retains_all_pins() {
    let dir = fixture();
    let other = add_recipe(dir.path(), "z-bin");
    std::fs::write(other.join("version.sh"), "older pins\n").unwrap();
    let http = Arc::new(MockHttpClient::responses(
        (0..2)
            .map(|_| {
                Ok(crate::infra::github::tests::response(
                    429,
                    &[("retry-after", "120")],
                    "",
                ))
            })
            .collect(),
    ));
    let ctx = release_context(dir.path(), false).with_http(Arc::<MockHttpClient>::clone(&http));
    assert_eq!(
        task_batch(&task().run(&ctx.with_dry_run(true)).unwrap()).failed_count(),
        2
    );
    assert!(
        !dir.path().join("bin/.github-releases-v1").exists(),
        "preview must not persist cooldowns"
    );
    for _ in 0..2 {
        assert_eq!(task_batch(&task().run(&ctx).unwrap()).failed_count(), 2);
    }
    assert_eq!(http.request_urls().len(), 2);
    assert_eq!(
        std::fs::read_to_string(other.join("version.sh")).unwrap(),
        "older pins\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("packages/tool-bin/version.sh")).unwrap(),
        "original pins\n"
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
        .with_home(dir.path().join("home"))
        .with_env(crate::infra::env::MapEnv::new().into_handle());
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

#[test]
fn config_paths_must_survive_paru_whitespace_and_comment_parsing() {
    for path in [
        "cache with spaces",
        "cache/internal space/clone",
        "cache\twith\ttabs",
    ] {
        assert_eq!(config_path(Path::new(path)).unwrap(), path);
    }
    for path in [
        " cache",
        "cache ",
        "\tcache",
        "cache\t",
        "cache\nother",
        "cache\rother",
        "cache#comment",
    ] {
        assert!(
            config_path(Path::new(path)).is_err(),
            "Paru would interpret {path:?} as a different path"
        );
    }
}

#[cfg(unix)]
#[test]
fn config_migrates_old_link_without_overwriting_tracked_source() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ContextBuilder::new(dir.path().into())
        .arch(true)
        .build()
        .with_home(dir.path().join("home"))
        .with_env(crate::infra::env::MapEnv::new().into_handle());
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

fn snapshot_context(root: &Path) -> Context {
    let mut mock = MockExecutor::new();
    expect_metadata(&mut mock);
    let ctx = make_context(
        root.into(),
        None,
        Platform::new(Os::Linux, true),
        Arc::new(mock),
    )
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
    let preview_log = Arc::new(crate::test_helpers::CapturingOutput::default());
    let preview_ctx =
        ctx.with_dry_run(true)
            .with_log(Arc::<crate::test_helpers::CapturingOutput>::clone(
                &preview_log,
            ));
    let preview = ConfigureParu.run(&preview_ctx).unwrap();
    assert_eq!(task_batch(&preview).changed_count(), 2);
    assert!(
        preview_log
            .messages_of(crate::infra::logging::MsgKind::DryRun)
            .iter()
            .any(|message| message.contains("refresh local package snapshot")),
        "preview must describe a snapshot that needs preparation"
    );
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

    let unchanged_log = Arc::new(crate::test_helpers::CapturingOutput::default());
    let unchanged_ctx =
        ctx.with_dry_run(true)
            .with_log(Arc::<crate::test_helpers::CapturingOutput>::clone(
                &unchanged_log,
            ));
    let unchanged = ConfigureParu.run(&unchanged_ctx).unwrap();
    assert_eq!(task_batch(&unchanged).changed_count(), 0);
    assert!(
        unchanged_log
            .messages_of(crate::infra::logging::MsgKind::DryRun)
            .is_empty(),
        "preview must not announce work for a prepared snapshot"
    );

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
fn snapshot_publishes_current_metadata_and_repairs_legacy_or_damaged_srcinfo() {
    let dir = fixture();
    let setup = snapshot_context(dir.path());
    let pins = dir.path().join("packages/tool-bin/version.sh");
    std::fs::write(&pins, "pkgver=1.0.103\n").unwrap();
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .withf(|spec| spec.program() == "makepkg" && spec.arguments() == ["--printsrcinfo"] && spec.is_checked())
        .returning(|spec| {
            let pin_contents = std::fs::read_to_string(spec.working_dir().unwrap().join("version.sh")).unwrap();
            let version = pin_contents.trim().strip_prefix("pkgver=").unwrap();
            Ok(ExecResult::success(format!(
                "pkgbase = tool-bin\n\tpkgver = {version}\n\tpkgrel = 1\n\tarch = x86_64\npkgname = tool-bin\n"
            )))
        });
    let ctx = make_context(
        dir.path().into(),
        None,
        Platform::new(Os::Linux, true),
        Arc::new(mock),
    )
    .with_home(setup.home().to_path_buf())
    .with_env(crate::infra::env::MapEnv::new().into_handle());
    let cache = build_root(&ctx).unwrap().join("tool-bin");
    let _first = ConfigureParu.run(&ctx).unwrap();
    let metadata = cache.join(".SRCINFO");
    let old_metadata = std::fs::read_to_string(&metadata).unwrap();
    assert!(old_metadata.contains("pkgver = 1.0.103\n"));

    // Reproduce the pre-fix snapshot: metadata exists but is not committed,
    // and a successful validation stamp exists for the recipe inputs.
    let repo = git2::Repository::open(&cache).unwrap();
    {
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        let mut index = repo.index().unwrap();
        index.remove_path(Path::new(".SRCINFO")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let signature = git2::Signature::now("fixture", "ci@test.local").unwrap();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "legacy snapshot",
            &tree,
            &[&parent],
        )
        .unwrap();
    }
    assert_eq!(
        task_batch(&ConfigureParu.run(&ctx).unwrap()).changed_count(),
        1
    );
    assert!(matches!(ConfigureParu.run(&ctx).unwrap(), TaskResult::Ok));

    std::fs::write(&pins, "pkgver=1.0.104\n").unwrap();
    let previous_head = repo.head().unwrap().target();
    let preview = ConfigureParu.run(&ctx.with_dry_run(true)).unwrap();
    assert_eq!(task_batch(&preview).changed_count(), 1);
    assert_eq!(std::fs::read_to_string(&metadata).unwrap(), old_metadata);
    assert_eq!(repo.head().unwrap().target(), previous_head);
    assert_eq!(
        task_batch(&ConfigureParu.run(&ctx).unwrap()).changed_count(),
        1
    );
    let current = std::fs::read_to_string(&metadata).unwrap();
    assert!(current.contains("pkgver = 1.0.104\n"));

    for damaged in [Some(old_metadata.as_str()), None] {
        if let Some(contents) = damaged {
            std::fs::write(&metadata, contents).unwrap();
        } else {
            std::fs::remove_file(&metadata).unwrap();
        }
        assert_eq!(
            task_batch(&ConfigureParu.run(&ctx).unwrap()).changed_count(),
            1
        );
        assert_eq!(std::fs::read_to_string(&metadata).unwrap(), current);
        assert!(matches!(ConfigureParu.run(&ctx).unwrap(), TaskResult::Ok));
    }
    // Paru's reset/clean must preserve the current package metadata too.
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    std::fs::write(&metadata, old_metadata).unwrap();
    repo.reset(head.as_object(), git2::ResetType::Hard, None)
        .unwrap();
    assert_eq!(std::fs::read_to_string(metadata).unwrap(), current);
    assert!(unavailable_packages(&ctx).unwrap().is_empty());
}

#[test]
fn changed_committed_metadata_invalidates_validation_cache() {
    let dir = fixture();
    let ctx = snapshot_context(dir.path());
    let _prepared = ConfigureParu.run(&ctx).unwrap();
    let cache = build_root(&ctx).unwrap().join("tool-bin");
    let metadata = cache.join(".SRCINFO");
    let current = std::fs::read_to_string(&metadata).unwrap();
    std::fs::write(&metadata, "stale committed metadata\n").unwrap();
    let repo = git2::Repository::open(&cache).unwrap();
    let parent = repo.head().unwrap().peel_to_commit().unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(".SRCINFO")).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let signature = git2::Signature::now("fixture", "ci@test.local").unwrap();
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        "stale committed metadata",
        &tree,
        &[&parent],
    )
    .unwrap();
    assert_eq!(
        task_batch(&ConfigureParu.run(&ctx).unwrap()).changed_count(),
        1
    );
    assert_eq!(std::fs::read_to_string(&metadata).unwrap(), current);
    assert!(matches!(ConfigureParu.run(&ctx).unwrap(), TaskResult::Ok));
}

#[test]
fn snapshot_commits_install_script_and_refreshes_changes_for_cleanup() {
    let dir = fixture();
    let source = dir.path().join("packages/tool-bin");
    let script = source.join("package.install");
    let build_root = dir.path().join("builds");
    std::fs::write(&script, "post_install() { :; }\n").unwrap();
    for name in ["LICENSE", "NOTICE"] {
        std::fs::write(source.join(name), format!("upstream {name}\n")).unwrap();
    }
    let snapshot = Snapshot::plan(&build_root, "tool-bin", &source, true).unwrap();
    assert!(snapshot.needs_write().unwrap());
    assert!(!snapshot.path.exists(), "planning must not publish inputs");
    snapshot.apply("fixture metadata\n").unwrap();
    assert!(
        !Snapshot::plan(&build_root, "tool-bin", &source, false)
            .unwrap()
            .needs_write()
            .unwrap()
    );
    let repo = git2::Repository::open(&snapshot.path).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    std::fs::write(snapshot.path.join("package.install"), "cache edit\n").unwrap();
    repo.reset(head.as_object(), git2::ResetType::Hard, None)
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(snapshot.path.join("package.install")).unwrap(),
        "post_install() { :; }\n"
    );
    std::fs::write(&script, "post_install() { echo updated; }\n").unwrap();
    let changed = Snapshot::plan(&build_root, "tool-bin", &source, false).unwrap();
    assert!(changed.needs_write().unwrap());
    changed.apply("fixture metadata\n").unwrap();
    assert!(
        !Snapshot::plan(&build_root, "tool-bin", &source, false)
            .unwrap()
            .needs_write()
            .unwrap()
    );
    let tree = repo.head().unwrap().peel_to_tree().unwrap();
    for name in ["LICENSE", "NOTICE"] {
        let blob = repo.find_blob(tree.get_name(name).unwrap().id()).unwrap();
        assert_eq!(blob.content(), format!("upstream {name}\n").as_bytes());
    }
    let blob = repo
        .find_blob(tree.get_name("package.install").unwrap().id())
        .unwrap();
    assert_eq!(blob.content(), b"post_install() { echo updated; }\n");
}

#[test]
fn snapshot_repairs_attributes_and_preserves_exact_bytes_through_git_reset() {
    let dir = fixture();
    let source = dir.path().join("packages/tool-bin");
    let build_root = dir.path().join("builds");
    let script = "post_install() { :; }\r\n";
    let metadata = "fixture metadata\n";
    std::fs::write(source.join("package.install"), script).unwrap();
    let snapshot = Snapshot::plan(&build_root, "tool-bin", &source, false).unwrap();
    snapshot.apply(metadata).unwrap();
    let repo = git2::Repository::open(&snapshot.path).unwrap();
    let attributes = repo.path().join("info/attributes");
    std::fs::write(snapshot.path.join(".gitattributes"), "* text eol=crlf\n").unwrap();

    for contents in [None, Some("* text eol=crlf\n")] {
        if let Some(contents) = contents {
            std::fs::write(&attributes, contents).unwrap();
        } else {
            std::fs::remove_file(&attributes).unwrap();
        }
        let planned = Snapshot::plan(&build_root, "tool-bin", &source, false).unwrap();
        assert!(planned.needs_write().unwrap());
        assert_eq!(read_optional(&attributes).unwrap().as_deref(), contents);
        planned.apply(metadata).unwrap();

        let head = repo.head().unwrap().peel_to_commit().unwrap();
        for name in ["package.install", ".SRCINFO"] {
            std::fs::write(snapshot.path.join(name), "changed\n").unwrap();
        }
        repo.reset(head.as_object(), git2::ResetType::Hard, None)
            .unwrap();

        for (name, expected) in [("package.install", script), (".SRCINFO", metadata)] {
            assert_eq!(
                std::fs::read(snapshot.path.join(name)).unwrap(),
                expected.as_bytes(),
                "{name}: snapshot-local attributes must preserve both CRLF and LF inputs"
            );
        }
        assert!(
            !Snapshot::plan(&build_root, "tool-bin", &source, false)
                .unwrap()
                .needs_write()
                .unwrap()
        );
    }
}

#[test]
fn snapshot_repairs_committed_symlink_inputs_even_when_blob_contents_match() {
    for name in ["PKGBUILD", "version.sh", ".SRCINFO"] {
        let dir = fixture();
        let source = dir.path().join("packages/tool-bin");
        let build_root = dir.path().join("builds");
        let snapshot = Snapshot::plan(&build_root, "tool-bin", &source, false).unwrap();
        snapshot.apply("fixture metadata\n").unwrap();
        let repo = git2::Repository::open(&snapshot.path).unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        let original = parent.tree().unwrap();
        let mut builder = repo.treebuilder(Some(&original)).unwrap();
        builder
            .insert(name, original.get_name(name).unwrap().id(), 0o120_000)
            .unwrap();
        let tree = repo.find_tree(builder.write().unwrap()).unwrap();
        let signature = git2::Signature::now("fixture", "ci@test.local").unwrap();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "replace input with committed symlink",
            &tree,
            &[&parent],
        )
        .unwrap();

        let damaged = Snapshot::plan(&build_root, "tool-bin", &source, false).unwrap();
        assert!(
            damaged.needs_write().unwrap(),
            "{name}: Git cleanup would restore a symlink rather than a usable input"
        );
        damaged.apply("fixture metadata\n").unwrap();
        assert_eq!(
            repo.head()
                .unwrap()
                .peel_to_tree()
                .unwrap()
                .get_name(name)
                .unwrap()
                .filemode(),
            0o100_644,
            "{name}",
        );
        assert!(
            !Snapshot::plan(&build_root, "tool-bin", &source, false)
                .unwrap()
                .needs_write()
                .unwrap(),
            "{name}: repaired snapshot must converge",
        );
    }
}

#[cfg(unix)]
#[test]
fn snapshot_repairs_working_symlinks_without_modifying_their_targets() {
    for name in ["PKGBUILD", "version.sh", ".SRCINFO"] {
        let dir = fixture();
        let source = dir.path().join("packages/tool-bin");
        let build_root = dir.path().join("builds");
        let snapshot = Snapshot::plan(&build_root, "tool-bin", &source, false).unwrap();
        snapshot.apply("fixture metadata\n").unwrap();
        let input = snapshot.path.join(name);
        let original = std::fs::read_to_string(&input).unwrap();
        let target = dir.path().join("outside-snapshot");
        std::fs::write(&target, &original).unwrap();
        std::fs::remove_file(&input).unwrap();
        std::os::unix::fs::symlink(&target, &input).unwrap();

        let damaged = Snapshot::plan(&build_root, "tool-bin", &source, false).unwrap();
        assert!(
            damaged.needs_write().unwrap(),
            "{name}: matching symlink contents must not bypass snapshot repair"
        );
        damaged.apply("fixture metadata\n").unwrap();

        assert!(std::fs::symlink_metadata(&input).unwrap().is_file());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), original);
        assert_eq!(std::fs::read_to_string(&input).unwrap(), original);
        assert!(
            !Snapshot::plan(&build_root, "tool-bin", &source, false)
                .unwrap()
                .needs_write()
                .unwrap(),
            "{name}: repaired snapshot must converge",
        );
    }
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
    assert_eq!(
        task_batch(&ConfigureParu.run(&ctx).unwrap()).failed_count(),
        1
    );
    std::fs::write(pins, "pins\n").unwrap();
    let cache = build_root(&ctx).unwrap().join("tool-bin");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("sentinel"), "unmanaged\n").unwrap();
    assert_eq!(
        task_batch(&ConfigureParu.run(&ctx).unwrap()).failed_count(),
        1
    );
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
    assert_eq!(
        task_batch(&ConfigureParu.run(&ctx.with_dry_run(true)).unwrap()).failed_count(),
        1
    );
    assert!(plan.apply("fixture metadata\n").is_err());
    assert_eq!(
        std::fs::read_to_string(cache.join("PKGBUILD")).unwrap(),
        original
    );
    assert_eq!(
        std::fs::read_to_string(cache.join("version.sh")).unwrap(),
        "original pins\n"
    );
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

fn release_failure_executor(retained: bool) -> MockExecutor {
    let mut mock = MockExecutor::new();
    mock.expect_which()
        .withf(|program| program == "gh")
        .return_const(false);
    expect_metadata(&mut mock);
    mock.expect_execute()
        .once()
        .withf(|spec| {
            spec.program() == "pacman" && spec.arguments() == ["-Q", "paru"] && !spec.is_checked()
        })
        .returning(|_| Ok(ExecResult::success("paru 2.1.0")));
    mock.expect_execute()
        .once()
        .withf(|spec| {
            spec.program() == "/usr/bin/paru"
                && spec.arguments() == ["--version"]
                && spec.is_checked()
        })
        .returning(|_| Ok(ExecResult::success("paru 2.1.0")));
    mock.expect_execute()
        .once()
        .withf(|spec| spec.program() == "pacman" && spec.arguments() == ["-Q"] && spec.is_checked())
        .returning(|_| Ok(ExecResult::success("")));
    mock.expect_execute()
        .once()
        .withf(move |spec| {
            let expected = if retained {
                vec![
                    "-S",
                    "--needed",
                    "--noconfirm",
                    "broken-bin",
                    "tool-bin",
                    "unrelated-aur",
                ]
            } else {
                vec!["-S", "--needed", "--noconfirm", "tool-bin", "unrelated-aur"]
            };
            spec.program() == "/usr/bin/paru" && spec.arguments() == expected && spec.is_checked()
        })
        .returning(|_| Ok(ExecResult::success("installed")));
    mock
}

#[test]
fn failed_release_does_not_block_other_updates_or_aur_installs() {
    use crate::domains::packages::config::packages::Package;
    use crate::domains::packages::install::InstallAurPackages;
    use crate::engine::TaskAssessment;
    use crate::engine::graph::ResolvedTaskGraph;
    use crate::infra::ConfigHandle;
    use crate::infra::logging::TaskStatus;
    use std::collections::HashMap;

    for parallel in [false, true] {
        for retained in [false, true] {
            let dir = fixture();
            let broken = add_recipe(dir.path(), "broken-bin");
            let old_pins = release_pins(&source(), &release()).unwrap();
            if retained {
                std::fs::write(broken.join("version.sh"), &old_pins).unwrap();
            }
            let setup = snapshot_context(dir.path());
            let mock = release_failure_executor(retained);
            let (logger, _logs, _guard) = crate::infra::logging::isolated_logger();
            let logger = Arc::new(logger);
            let ctx = make_context(
                dir.path().into(),
                None,
                Platform::new(Os::Linux, true),
                Arc::new(mock),
            )
            .with_home(setup.home().to_path_buf())
            .with_env(crate::infra::env::MapEnv::new().into_handle())
            .with_http(Arc::new(release_failure_http(retained)))
            .with_log(Arc::<crate::infra::logging::Logger>::clone(&logger))
            .with_parallel(parallel);
            let versions = UpdateLocalPackageVersions;
            let config = ConfigureParu;
            let install = InstallAurPackages::new(ConfigHandle::new(
                ["broken-bin", "tool-bin", "unrelated-aur"]
                    .into_iter()
                    .map(|name| Package {
                        name: name.into(),
                        is_aur: true,
                    })
                    .collect(),
            ));
            // Reverse the intended order to ensure dependency edges schedule it.
            let tasks: Vec<&dyn Task> = vec![&install, &config, &versions];
            let graph = ResolvedTaskGraph::resolve(&tasks).unwrap();
            let assessments = tasks
                .iter()
                .map(|task| (task.task_id(), TaskAssessment::applicable()))
                .collect::<HashMap<_, _>>();
            let run = if parallel {
                crate::engine::scheduler::run_tasks_parallel
            } else {
                crate::engine::scheduler::run_tasks_sequential
            };
            let summary = run(&tasks, &graph, &assessments, &ctx, &logger);
            assert!(
                summary.failure_count() > 0,
                "refresh failure must remain a failed run"
            );
            let entries = logger.task_entries();
            let version_entry = entries
                .iter()
                .find(|entry| entry.name == "Local package versions")
                .unwrap();
            assert_eq!(version_entry.status, TaskStatus::Failed);
            assert!(
                entries
                    .iter()
                    .all(|entry| entry.status != TaskStatus::Blocked)
            );
            let install_entry = entries
                .iter()
                .find(|entry| entry.name == "AUR packages")
                .unwrap();
            if !retained {
                assert_eq!(install_entry.status, TaskStatus::Failed);
            }
            assert_eq!(
                std::fs::read_to_string(dir.path().join("packages/tool-bin/version.sh")).unwrap(),
                old_pins
            );
            let generated =
                std::fs::read_to_string(ctx.home().join(".config/paru/paru.conf")).unwrap();
            assert!(generated.contains("[dotfiles-tool-bin]"));
            assert_eq!(generated.contains("[dotfiles-broken-bin]"), retained);
            if retained {
                assert_eq!(
                    std::fs::read_to_string(broken.join("version.sh")).unwrap(),
                    old_pins
                );
                assert!(unavailable_packages(&ctx).unwrap().is_empty());
            } else {
                assert!(!broken.join("version.sh").exists());
                assert!(
                    unavailable_packages(&ctx)
                        .unwrap()
                        .contains_key("broken-bin")
                );
            }
        }
    }
}

fn release_failure_http(retained: bool) -> MockHttpClient {
    let broken = if retained {
        let mut metadata: serde_json::Value = serde_json::from_str(&release()).unwrap();
        metadata["assets"] = serde_json::json!([]);
        Ok(metadata.to_string().into_bytes())
    } else {
        Err(anyhow::anyhow!("upstream unavailable"))
    };
    MockHttpClient::new(vec![broken, Ok(release().into_bytes())])
}

#[test]
fn snapshot_conflict_removes_only_its_repository_and_preserves_other_recipes() {
    let dir = fixture();
    let ctx = snapshot_context(dir.path());
    let broken = add_recipe(dir.path(), "broken-bin");
    std::fs::write(broken.join("version.sh"), "old pins\n").unwrap();
    assert_eq!(
        task_batch(&ConfigureParu.run(&ctx).unwrap()).failed_count(),
        0
    );
    let path = build_root(&ctx).unwrap().join("broken-bin");
    std::fs::remove_file(path.join(".git/dotfiles-local-build")).unwrap();
    let result = ConfigureParu.run(&ctx).unwrap();
    assert_eq!(task_batch(&result).failed_count(), 1);
    let config = std::fs::read_to_string(ctx.home().join(".config/paru/paru.conf")).unwrap();
    assert!(config.contains("[dotfiles-tool-bin]"));
    assert!(!config.contains("[dotfiles-broken-bin]"));
    assert!(
        path.join("version.sh").exists(),
        "conflicting snapshot must be preserved"
    );
    assert_eq!(
        unavailable_packages(&ctx)
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        [&"broken-bin".to_string()]
    );
}

#[test]
fn readiness_requires_published_config_and_committed_current_inputs() {
    let dir = fixture();
    let ctx = snapshot_context(dir.path());
    let _result = ConfigureParu.run(&ctx).unwrap();
    assert!(unavailable_packages(&ctx).unwrap().is_empty());
    let config = ctx.home().join(".config/paru/paru.conf");
    let original = std::fs::read_to_string(&config).unwrap();
    std::fs::write(&config, "[options]\n").unwrap();
    assert!(unavailable_packages(&ctx).unwrap().contains_key("tool-bin"));
    std::fs::write(config, original).unwrap();
    std::fs::write(
        dir.path().join("packages/tool-bin/PKGBUILD"),
        "new inputs\n",
    )
    .unwrap();
    assert!(unavailable_packages(&ctx).unwrap().contains_key("tool-bin"));
    let _refreshed = ConfigureParu.run(&ctx).unwrap();
    assert!(unavailable_packages(&ctx).unwrap().is_empty());
}

#[test]
fn split_package_ownership_is_explicit_and_ordinary_names_default_to_base() {
    assert_eq!(source().package_names(), ["tool-bin"]);
    let dir = fixture();
    let ctx = snapshot_context(dir.path());
    let path = dir.path().join("packages/tool-bin/release.json");
    let mut descriptor: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    descriptor["packages"] = serde_json::json!(["tool-cli", "tool-gui"]);
    std::fs::write(&path, descriptor.to_string()).unwrap();
    let missing = unavailable_packages(&ctx).unwrap();
    assert!(missing.contains_key("tool-cli"));
    assert!(missing.contains_key("tool-gui"));
    assert!(!missing.contains_key("tool-bin"));
    for invalid in [
        serde_json::json!([]),
        serde_json::json!(["../unsafe"]),
        serde_json::json!(["same", "same"]),
    ] {
        descriptor["packages"] = invalid;
        std::fs::write(&path, descriptor.to_string()).unwrap();
        assert!(unavailable_packages(&ctx).is_err());
    }
    std::fs::remove_file(path).unwrap();
    assert!(
        unavailable_packages(&ctx).is_err(),
        "missing descriptor must not erase ownership and permit AUR fallback"
    );
}

#[test]
fn readiness_rejects_descriptor_changes_before_configuration_is_refreshed() {
    let dir = fixture();
    let ctx = snapshot_context(dir.path());
    let _prepared = ConfigureParu.run(&ctx).unwrap();
    assert!(unavailable_packages(&ctx).unwrap().is_empty());
    let path = dir.path().join("packages/tool-bin/release.json");
    let mut descriptor: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    descriptor["packages"] = serde_json::json!(["tool-cli", "tool-gui"]);
    std::fs::write(path, descriptor.to_string()).unwrap();

    let missing = unavailable_packages(&ctx).unwrap();

    assert_eq!(
        missing.keys().map(String::as_str).collect::<Vec<_>>(),
        ["tool-cli", "tool-gui"],
        "an old snapshot must not send newly declared local packages to the AUR"
    );
}

#[test]
fn readiness_requires_successful_metadata_validation_for_current_inputs() {
    let dir = fixture();
    let ctx = snapshot_context(dir.path());
    let _prepared = ConfigureParu.run(&ctx).unwrap();
    let stamp = build_root(&ctx)
        .unwrap()
        .join("tool-bin/.git/dotfiles-validated-metadata");
    for damaged in [None, Some("stale validation key")] {
        if let Some(value) = damaged {
            std::fs::write(&stamp, value).unwrap();
        } else {
            std::fs::remove_file(&stamp).unwrap();
        }

        assert!(
            unavailable_packages(&ctx).unwrap().contains_key("tool-bin"),
            "missing or stale validation is not proof of package readiness"
        );
        assert!(
            unavailable_packages(&ctx.with_dry_run(true))
                .unwrap()
                .is_empty(),
            "preview may plan prerequisite validation without executing recipes"
        );
        let _revalidated = ConfigureParu.run(&ctx).unwrap();
        assert!(unavailable_packages(&ctx).unwrap().is_empty());
    }
}

#[test]
fn snapshot_apply_failure_does_not_publish_its_section_or_block_other_snapshots() {
    let dir = fixture();
    let ctx = snapshot_context(dir.path());
    let broken = add_recipe(dir.path(), "broken-bin");
    std::fs::write(broken.join("version.sh"), "old pins\n").unwrap();
    let OperationState::NeedsRun { plan } = ParuConfig.current_state(&ctx).unwrap() else {
        panic!("expected preparation plan")
    };
    let conflict = build_root(&ctx).unwrap().join("broken-bin");
    std::fs::create_dir_all(&conflict).unwrap();
    std::fs::write(conflict.join("sentinel"), "preserve").unwrap();
    let result = ParuConfig.apply(&ctx, &plan).unwrap();
    assert_eq!(task_batch(&result).failed_count(), 1);
    assert_eq!(task_batch(&result).changed_count(), 2);
    assert_eq!(
        std::fs::read_to_string(conflict.join("sentinel")).unwrap(),
        "preserve"
    );
    let config = std::fs::read_to_string(ctx.home().join(".config/paru/paru.conf")).unwrap();
    assert!(!config.contains("[dotfiles-broken-bin]"));
    assert!(config.contains("[dotfiles-tool-bin]"));
}

#[test]
fn cancellation_aborts_discovery_without_publishing_pending_updates() {
    let dir = fixture();
    let other = add_recipe(dir.path(), "z-bin");
    let original =
        std::fs::read_to_string(dir.path().join("packages/tool-bin/version.sh")).unwrap();
    let mut mock = MockExecutor::new();
    mock.expect_which()
        .withf(|program| program == "gh")
        .return_const(false);
    let http = MockHttpClient::new(vec![
        Ok(release().into_bytes()),
        Err(crate::infra::exec::ExecError::Cancelled {
            command: "release discovery".into(),
            result: ExecResult::failure("", "interrupted", None),
        }
        .into()),
    ]);
    let ctx = make_context(
        dir.path().into(),
        None,
        Platform::new(Os::Linux, true),
        Arc::new(mock),
    )
    .with_env(crate::infra::env::MapEnv::new().into_handle())
    .with_http(Arc::new(http));
    let error = UpdateLocalPackageVersions.run(&ctx).unwrap_err();
    assert!(
        error
            .downcast_ref::<crate::infra::exec::ExecError>()
            .unwrap()
            .is_cancelled()
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("packages/tool-bin/version.sh")).unwrap(),
        original
    );
    assert!(!other.join("version.sh").exists());
    assert!(!dir.path().join("bin/.github-releases-v1").exists());
}

#[test]
fn config_overrides_control_publication_and_readiness() {
    for explicit in [false, true] {
        let dir = fixture();
        let ctx = snapshot_context(dir.path());
        let xdg = dir.path().join("xdg config");
        let explicit_path = dir.path().join("custom/paru.conf");
        let mut env = crate::infra::env::MapEnv::new().with("XDG_CONFIG_HOME", &xdg);
        let target = if explicit {
            env = env.with("PARU_CONF", &explicit_path);
            explicit_path
        } else {
            xdg.join("paru/paru.conf")
        };
        let ctx = ctx.with_env(env.into_handle());
        let default = ctx.home().join(".config/paru/paru.conf");
        std::fs::create_dir_all(default.parent().unwrap()).unwrap();
        std::fs::write(&default, "sentinel\n").unwrap();
        let _preview = ConfigureParu.run(&ctx.with_dry_run(true)).unwrap();
        assert!(!target.exists());
        assert!(!build_root(&ctx).unwrap().exists());
        assert!(unavailable_packages(&ctx).unwrap().contains_key("tool-bin"));
        let _applied = ConfigureParu.run(&ctx).unwrap();
        assert!(target.is_file());
        assert!(unavailable_packages(&ctx).unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(&default).unwrap(), "sentinel\n");
        // A valid default config must not mask an invalid active override.
        std::fs::copy(&target, &default).unwrap();
        std::fs::write(&target, "[options]\n").unwrap();
        assert!(unavailable_packages(&ctx).unwrap().contains_key("tool-bin"));
        let _repaired = ConfigureParu.run(&ctx).unwrap();
        assert!(unavailable_packages(&ctx).unwrap().is_empty());
        assert!(matches!(ConfigureParu.run(&ctx).unwrap(), TaskResult::Ok));
    }
    for key in ["PARU_CONF", "XDG_CONFIG_HOME"] {
        for value in ["", "relative/path"] {
            let dir = fixture();
            let ctx = snapshot_context(dir.path()).with_env(
                crate::infra::env::MapEnv::new()
                    .with(key, value)
                    .into_handle(),
            );
            assert!(ConfigureParu.run(&ctx).is_err(), "{key}={value:?}");
            assert!(unavailable_packages(&ctx).is_err());
            assert!(!build_root(&ctx).unwrap().exists());
        }
    }
}

#[test]
fn srcinfo_must_match_descriptor_ownership_and_architectures() {
    let valid = "pkgbase = tool-bin\n\tarch = x86_64\npkgname = tool-bin\n";
    source().validate_srcinfo(valid).unwrap();
    for (label, metadata) in [
        ("empty", String::new()),
        (
            "base",
            valid.replace("pkgbase = tool-bin", "pkgbase = other"),
        ),
        (
            "name",
            valid.replace("pkgname = tool-bin", "pkgname = other"),
        ),
        ("architecture", valid.replace("x86_64", "aarch64")),
        ("extra package", format!("{valid}pkgname = extra\n")),
        ("extra architecture", format!("{valid}\tarch = aarch64\n")),
        ("duplicate package", format!("{valid}pkgname = tool-bin\n")),
    ] {
        assert!(source().validate_srcinfo(&metadata).is_err(), "{label}");
    }
    let mut split = source();
    split.packages = Some(vec!["tool-cli".into(), "tool-gui".into()]);
    let metadata = "pkgbase = tool-bin\n\tarch = x86_64\npkgname = tool-gui\npkgname = tool-cli\n";
    split.validate_srcinfo(metadata).unwrap();
    assert!(
        split
            .validate_srcinfo(&metadata.replace("pkgname = tool-cli\n", ""))
            .is_err()
    );
}

#[test]
fn invalid_metadata_is_not_published_and_other_recipes_continue() {
    let dir = fixture();
    let ctx = snapshot_context(dir.path());
    let _prepared = ConfigureParu.run(&ctx).unwrap();
    let cache = build_root(&ctx).unwrap().join("tool-bin");
    let original = std::fs::read(cache.join("PKGBUILD")).unwrap();
    let original_metadata = std::fs::read(cache.join(".SRCINFO")).unwrap();
    let recipe = dir.path().join("packages/tool-bin/PKGBUILD");
    std::fs::write(&recipe, "pkgname=wrong-name\n").unwrap();
    let other = add_recipe(dir.path(), "other-bin");
    std::fs::write(other.join("version.sh"), "pins\n").unwrap();
    let result = ConfigureParu.run(&ctx).unwrap();
    assert_eq!(task_batch(&result).failed_count(), 1);
    assert_eq!(std::fs::read(cache.join("PKGBUILD")).unwrap(), original);
    assert_eq!(
        std::fs::read(cache.join(".SRCINFO")).unwrap(),
        original_metadata
    );
    let config = std::fs::read_to_string(paru_config_path(&ctx).unwrap()).unwrap();
    assert!(!config.contains("[dotfiles-tool-bin]"));
    assert!(config.contains("[dotfiles-other-bin]"));
    assert!(unavailable_packages(&ctx).unwrap().contains_key("tool-bin"));
    // Descriptor-only changes must also be checked when snapshot inputs match.
    std::fs::write(recipe, original).unwrap();
    let path = dir.path().join("packages/tool-bin/release.json");
    let mut descriptor: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    descriptor["packages"] = serde_json::json!(["different-name"]);
    std::fs::write(path, descriptor.to_string()).unwrap();
    assert_eq!(
        task_batch(&ConfigureParu.run(&ctx).unwrap()).failed_count(),
        1
    );
    assert!(
        !std::fs::read_to_string(paru_config_path(&ctx).unwrap())
            .unwrap()
            .contains("[dotfiles-tool-bin]")
    );
}

#[test]
fn metadata_execution_failure_preserves_snapshot_and_cancellation_type() {
    for cancelled in [false, true] {
        let dir = fixture();
        let setup = snapshot_context(dir.path());
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .withf(|spec| {
                spec.program() == "makepkg"
                    && spec.arguments() == ["--printsrcinfo"]
                    && spec.is_checked()
                    && spec.working_dir().is_some()
            })
            .returning(move |_| {
                Err(if cancelled {
                    crate::infra::exec::ExecError::Cancelled {
                        command: "makepkg".into(),
                        result: ExecResult::failure("", "interrupted", None),
                    }
                } else {
                    crate::infra::exec::ExecError::spawn(
                        "makepkg",
                        std::io::Error::other("fixture unavailable"),
                    )
                })
            });
        let ctx = make_context(
            dir.path().into(),
            None,
            Platform::new(Os::Linux, true),
            Arc::new(mock),
        )
        .with_home(setup.home().to_path_buf())
        .with_env(crate::infra::env::MapEnv::new().into_handle());
        let result = ConfigureParu.run(&ctx);
        if cancelled {
            assert!(
                result
                    .unwrap_err()
                    .downcast_ref::<crate::infra::exec::ExecError>()
                    .unwrap()
                    .is_cancelled()
            );
            assert!(!paru_config_path(&ctx).unwrap().exists());
        } else {
            assert_eq!(task_batch(&result.unwrap()).failed_count(), 1);
            assert!(
                !std::fs::read_to_string(paru_config_path(&ctx).unwrap())
                    .unwrap()
                    .contains("[dotfiles-tool-bin]")
            );
        }
        assert!(!build_root(&ctx).unwrap().exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("packages/tool-bin/version.sh")).unwrap(),
            "original pins\n"
        );
    }
}

#[test]
fn successful_metadata_validation_is_reused_only_for_identical_intact_inputs() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let dir = fixture();
    let setup = snapshot_context(dir.path());
    let calls = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .withf(|spec| spec.program() == "makepkg")
        .returning(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(ExecResult::success(
                "pkgbase = tool-bin\n\tarch = x86_64\npkgname = tool-bin\n",
            ))
        });
    let ctx = make_context(
        dir.path().into(),
        None,
        Platform::new(Os::Linux, true),
        Arc::new(mock),
    )
    .with_home(setup.home().to_path_buf())
    .with_env(crate::infra::env::MapEnv::new().into_handle());
    let _first = ConfigureParu.run(&ctx).unwrap();
    let snapshot = build_root(&ctx).unwrap().join("tool-bin");
    let stamp = snapshot.join(".git/dotfiles-validated-metadata");
    let modified = std::fs::metadata(&stamp).unwrap().modified().unwrap();
    let _repeat = ConfigureParu.run(&ctx).unwrap();
    let _preview = ConfigureParu.run(&ctx.with_dry_run(true)).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::metadata(&stamp).unwrap().modified().unwrap(),
        modified
    );

    for (path, content) in [
        (
            dir.path().join("packages/tool-bin/PKGBUILD"),
            "pkgname=tool-bin\n# changed\n",
        ),
        (
            dir.path().join("packages/tool-bin/version.sh"),
            "changed pins\n",
        ),
        (snapshot.join("PKGBUILD"), "damaged cache\n"),
    ] {
        let before = calls.load(Ordering::SeqCst);
        std::fs::write(path, content).unwrap();
        let _changed = ConfigureParu.run(&ctx).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), before + 1);
    }
    let descriptor = dir.path().join("packages/tool-bin/release.json");
    let content = std::fs::read_to_string(&descriptor)
        .unwrap()
        .replace("example/tool", "example/other");
    std::fs::write(descriptor, content).unwrap();
    let _descriptor = ConfigureParu.run(&ctx).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 5);
    std::fs::remove_file(stamp).unwrap();
    let _missing_stamp = ConfigureParu.run(&ctx).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 6);
}

#[test]
fn recipe_workers_are_bounded_ordered_and_respect_sequential_mode() {
    use std::sync::{
        Condvar, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    for parallel in [false, true] {
        for length in [0_usize, 1, 2, 8] {
            let dir = fixture();
            let ctx = snapshot_context(dir.path()).with_parallel(parallel);
            let caller = std::thread::current().id();
            let gate = (Mutex::new((0_usize, 0_usize)), Condvar::new());
            let active = AtomicUsize::new(0);
            let peak = AtomicUsize::new(0);
            let results = map_recipes(&ctx, &(0..length).collect::<Vec<_>>(), |item| {
                let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                if parallel && length >= 2 {
                    // A bounded cyclic gate proves overlap, but a broken
                    // dispatch policy fails instead of hanging the test suite.
                    let mut state = gate.0.lock().unwrap();
                    let generation = state.1;
                    state.0 += 1;
                    if state.0 == length.min(4) {
                        state.0 = 0;
                        state.1 += 1;
                        gate.1.notify_all();
                    } else {
                        let (_state, timeout) = gate
                            .1
                            .wait_timeout_while(state, Duration::from_secs(10), |state| {
                                state.1 == generation
                            })
                            .unwrap();
                        assert!(!timeout.timed_out(), "parallel recipes must overlap");
                    }
                } else {
                    assert_eq!(
                        std::thread::current().id(),
                        caller,
                        "sequential and singleton work must stay on the calling thread"
                    );
                }
                active.fetch_sub(1, Ordering::SeqCst);
                Ok(item * 2)
            })
            .unwrap();
            assert_eq!(
                results.into_iter().collect::<Result<Vec<_>>>().unwrap(),
                (0..length).map(|n| n * 2).collect::<Vec<_>>()
            );
            let expected_peak = if length == 0 {
                0
            } else if parallel {
                length.min(4)
            } else {
                1
            };
            assert_eq!(peak.load(Ordering::SeqCst), expected_peak);
        }
    }
}

#[test]
fn failed_metadata_validation_is_retried_without_a_success_stamp() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let dir = fixture();
    let setup = snapshot_context(dir.path());
    let attempts = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&attempts);
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .withf(|spec| spec.program() == "makepkg")
        .times(2)
        .returning(move |_| {
            if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(crate::infra::exec::ExecError::spawn(
                    "makepkg",
                    std::io::Error::other("try again"),
                ));
            }
            Ok(ExecResult::success(
                "pkgbase = tool-bin\n\tarch = x86_64\npkgname = tool-bin\n",
            ))
        });
    let ctx = make_context(
        dir.path().into(),
        None,
        Platform::new(Os::Linux, true),
        Arc::new(mock),
    )
    .with_home(setup.home().to_path_buf())
    .with_env(crate::infra::env::MapEnv::new().into_handle());
    assert_eq!(
        task_batch(&ConfigureParu.run(&ctx).unwrap()).failed_count(),
        1
    );
    let stamp = build_root(&ctx)
        .unwrap()
        .join("tool-bin/.git/dotfiles-validated-metadata");
    assert!(!stamp.exists());
    assert_eq!(
        task_batch(&ConfigureParu.run(&ctx).unwrap()).failed_count(),
        0
    );
    assert!(stamp.exists());
    assert!(matches!(ConfigureParu.run(&ctx).unwrap(), TaskResult::Ok));
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
}

#[test]
fn cancelled_recipe_stops_later_sequential_dispatch_without_caching_failure() {
    let dir = fixture();
    let ctx = snapshot_context(dir.path()).with_parallel(false);
    let results = map_recipes(&ctx, &[0, 1, 2], |item| -> Result<()> {
        assert_eq!(*item, 0, "no recipe may start after cancellation");
        Err(crate::infra::exec::ExecError::Cancelled {
            command: "fixture metadata".into(),
            result: ExecResult::failure("", "", None),
        }
        .into())
    })
    .unwrap();
    for result in results {
        assert!(
            result
                .unwrap_err()
                .downcast_ref::<crate::infra::exec::ExecError>()
                .unwrap()
                .is_cancelled()
        );
    }
}
