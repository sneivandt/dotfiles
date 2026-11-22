use super::*;

#[test]
fn tracked_recipe_descriptors_validate_without_network_or_generated_pins() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let ctx = ContextBuilder::new(root.into()).build();
    let directories = recipe_directories(&ctx).unwrap();
    assert!(
        !directories.is_empty(),
        "expected tracked Arch package recipes"
    );
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
        let f = SnapshotFixture::new();
        let ctx = &f.ctx;
        let path = f.source.join("release.json");
        let mut descriptor: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        descriptor[field] = value;
        std::fs::write(path, descriptor.to_string()).unwrap();
        // The executor has no process expectations: discovery must reject
        // invalid metadata before making a network request.
        assert!(
            task().run(ctx).is_err(),
            "{label}: release discovery accepted invalid input"
        );
        assert!(
            configure_paru(ctx).is_err(),
            "{label}: snapshot preparation accepted invalid input"
        );
        assert!(
            !build_root(ctx).unwrap().exists(),
            "{label}: cache was mutated"
        );
        assert!(
            !ctx.home().join(".config/paru/paru.conf").exists(),
            "{label}: config was published"
        );
        assert_eq!(
            std::fs::read_to_string(f.source.join("version.sh")).unwrap(),
            "original pins\n",
            "{label}: pins were changed"
        );
    }
}

#[test]
fn unreadable_descriptor_is_an_error_not_an_absent_recipe() {
    let f = SnapshotFixture::new();
    let descriptor = f.source.join("release.json");
    std::fs::remove_file(&descriptor).unwrap();
    std::fs::create_dir(&descriptor).unwrap();
    assert!(task().run(&f.ctx).is_err());
    assert!(configure_paru(&f.ctx).is_err());
    assert!(!build_root(&f.ctx).unwrap().exists());
    assert!(!f.ctx.home().join(".config/paru/paru.conf").exists());
}

#[test]
fn recipe_directory_query_failure_does_not_publish_empty_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = snapshot_context(dir.path(), metadata_executor());
    std::fs::write(dir.path().join("packages"), "not a directory\n").unwrap();
    assert!(task().run(&ctx).is_err());
    assert!(configure_paru(&ctx).is_err());
    assert!(!ctx.home().join(".config/paru/paru.conf").exists());
}

#[test]
fn interrupted_second_validation_leaves_the_published_first_snapshot_usable() {
    let f = SnapshotFixture::new();
    let _installed = f.configure();
    let path = &f.cache;
    let config = paru_config_path(&f.ctx).unwrap();
    let before_config = std::fs::read(&config).unwrap();
    let before_recipe = std::fs::read(path.join("PKGBUILD")).unwrap();
    let changed = "pkgname=tool-bin\n# updated\n";
    std::fs::write(f.source.join("PKGBUILD"), changed).unwrap();
    std::fs::write(
        add_recipe(f.dir.path(), "z-bin").join("version.sh"),
        "pins\n",
    )
    .unwrap();

    let mut mock = MockExecutor::new();
    mock.expect_execute().times(2).returning(|spec| {
        assert_eq!(spec.program(), "makepkg");
        assert_eq!(spec.arguments(), ["--printsrcinfo"]);
        let recipe = std::fs::read_to_string(spec.working_dir().unwrap().join("PKGBUILD")).unwrap();
        if recipe.contains("pkgname=z-bin\n") {
            return Err(crate::infra::exec::ExecError::Cancelled {
                command: "z-bin metadata".into(),
                result: ExecResult::failure("", "interrupted", None),
            });
        }
        assert_eq!(recipe, "pkgname=tool-bin\n# updated\n");
        Ok(ExecResult::success(
            "pkgbase = tool-bin\n\tarch = x86_64\npkgname = tool-bin\n",
        ))
    });
    let ctx = arch_context(f.dir.path(), mock).with_parallel(false);
    let error = configure_paru(&ctx).unwrap_err();
    assert!(
        error
            .downcast_ref::<crate::infra::exec::ExecError>()
            .unwrap()
            .is_cancelled()
    );
    assert_eq!(std::fs::read(path.join("PKGBUILD")).unwrap(), before_recipe);
    assert_eq!(std::fs::read(&config).unwrap(), before_config);
    assert!(!build_root(&ctx).unwrap().join("z-bin").exists());
    assert_eq!(task_batch(&f.configure()).changed_count(), 3);
    assert_eq!(
        std::fs::read_to_string(path.join("PKGBUILD")).unwrap(),
        changed
    );
    assert!(matches!(f.configure(), TaskResult::Ok));
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
    let f = SnapshotFixture::new();
    let _prepared = f.configure();
    let cache = &f.cache;
    let original = std::fs::read(cache.join("PKGBUILD")).unwrap();
    let original_metadata = std::fs::read(cache.join(".SRCINFO")).unwrap();
    let recipe = f.source.join("PKGBUILD");
    std::fs::write(&recipe, "pkgname=wrong-name\n").unwrap();
    let other = add_recipe(f.dir.path(), "other-bin");
    std::fs::write(other.join("version.sh"), "pins\n").unwrap();
    let result = f.configure();
    assert_eq!(task_batch(&result).failed_count(), 1);
    assert_eq!(std::fs::read(cache.join("PKGBUILD")).unwrap(), original);
    assert_eq!(
        std::fs::read(cache.join(".SRCINFO")).unwrap(),
        original_metadata
    );
    let config = std::fs::read_to_string(paru_config_path(&f.ctx).unwrap()).unwrap();
    assert!(!config.contains("[dotfiles-tool-bin]"));
    assert!(config.contains("[dotfiles-other-bin]"));
    assert!(
        unavailable_packages(&f.ctx)
            .unwrap()
            .contains_key("tool-bin")
    );
    // Descriptor-only changes must also be checked when snapshot inputs match.
    std::fs::write(recipe, original).unwrap();
    let path = f.source.join("release.json");
    let mut descriptor: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    descriptor["packages"] = serde_json::json!(["different-name"]);
    std::fs::write(path, descriptor.to_string()).unwrap();
    assert_eq!(task_batch(&f.configure()).failed_count(), 1);
    assert!(
        !std::fs::read_to_string(paru_config_path(&f.ctx).unwrap())
            .unwrap()
            .contains("[dotfiles-tool-bin]")
    );
}

#[test]
fn metadata_execution_failure_preserves_snapshot_and_cancellation_type() {
    for cancelled in [false, true] {
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
        let f = SnapshotFixture::with_executor(mock);
        let result = configure_paru(&f.ctx);
        if cancelled {
            assert!(
                result
                    .unwrap_err()
                    .downcast_ref::<crate::infra::exec::ExecError>()
                    .unwrap()
                    .is_cancelled()
            );
            assert!(!paru_config_path(&f.ctx).unwrap().exists());
        } else {
            assert_eq!(task_batch(&result.unwrap()).failed_count(), 1);
            assert!(
                !std::fs::read_to_string(paru_config_path(&f.ctx).unwrap())
                    .unwrap()
                    .contains("[dotfiles-tool-bin]")
            );
        }
        assert!(!build_root(&f.ctx).unwrap().exists());
        assert_eq!(
            std::fs::read_to_string(f.source.join("version.sh")).unwrap(),
            "original pins\n"
        );
    }
}

#[test]
fn successful_metadata_validation_is_reused_only_for_identical_intact_inputs() {
    use std::sync::atomic::{AtomicUsize, Ordering};
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
    let f = SnapshotFixture::with_executor(mock);
    let _first = f.configure();
    let stamp = f.cache.join(".git/dotfiles-validated-metadata");
    let modified = std::fs::metadata(&stamp).unwrap().modified().unwrap();
    let _repeat = f.configure();
    let op = ParuConfig {
        base: &f.ctx.symlinks_dir().join("config/paru/paru.conf.base"),
    };
    assert!(matches!(
        op.current_state(&f.ctx).unwrap(),
        OperationState::Complete
    ));
    let _preview = f.preview();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::metadata(&stamp).unwrap().modified().unwrap(),
        modified
    );

    for (path, content) in [
        (f.source.join("PKGBUILD"), "pkgname=tool-bin\n# changed\n"),
        (f.source.join("version.sh"), "changed pins\n"),
        (f.cache.join("PKGBUILD"), "damaged cache\n"),
    ] {
        let before = calls.load(Ordering::SeqCst);
        std::fs::write(path, content).unwrap();
        let _changed = f.configure();
        assert_eq!(calls.load(Ordering::SeqCst), before + 1);
    }
    let descriptor = f.source.join("release.json");
    let content = std::fs::read_to_string(&descriptor)
        .unwrap()
        .replace("example/tool", "example/other");
    std::fs::write(descriptor, content).unwrap();
    let _descriptor = f.configure();
    assert_eq!(calls.load(Ordering::SeqCst), 5);
    std::fs::remove_file(&stamp).unwrap();
    assert!(matches!(
        op.current_state(&f.ctx).unwrap(),
        OperationState::NeedsRun { .. }
    ));
    let preview = f.preview();
    assert_eq!(task_batch(&preview).changed_count(), 1);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        5,
        "preview must not evaluate recipes"
    );
    assert!(!stamp.exists());
    assert_eq!(task_batch(&f.configure()).changed_count(), 1);
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
            let f = SnapshotFixture::new();
            let ctx = f.ctx.with_parallel(parallel);
            let caller = std::thread::current().id();
            let gate = (Mutex::new(std::collections::HashSet::new()), Condvar::new());
            let active = AtomicUsize::new(0);
            let peak = AtomicUsize::new(0);
            let workers = length.min(4).min(rayon::current_num_threads());
            let results = map_recipes(&ctx, &(0..length).collect::<Vec<_>>(), |item| {
                let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                if parallel && workers >= 2 {
                    // A first-visit gate proves overlap, but a broken
                    // dispatch policy fails instead of hanging the test suite.
                    let mut threads = gate.0.lock().unwrap();
                    if threads.insert(std::thread::current().id()) {
                        if threads.len() == workers {
                            gate.1.notify_all();
                        } else {
                            let (_threads, timeout) = gate
                                .1
                                .wait_timeout_while(threads, Duration::from_secs(10), |threads| {
                                    threads.len() < workers
                                })
                                .unwrap();
                            assert!(!timeout.timed_out(), "parallel recipes must overlap");
                        }
                    }
                } else if !parallel || length < 2 {
                    assert_eq!(
                        std::thread::current().id(),
                        caller,
                        "sequential and singleton work must stay on the calling thread"
                    );
                }
                active.fetch_sub(1, Ordering::SeqCst);
                Ok(item * 2)
            });
            assert_eq!(
                results.into_iter().collect::<Result<Vec<_>>>().unwrap(),
                (0..length).map(|n| n * 2).collect::<Vec<_>>()
            );
            let expected_peak = if length == 0 {
                0
            } else if parallel {
                workers
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
    let f = SnapshotFixture::with_executor(mock);
    assert_eq!(task_batch(&f.configure()).failed_count(), 1);
    let stamp = f.cache.join(".git/dotfiles-validated-metadata");
    assert!(!stamp.exists());
    assert_eq!(task_batch(&f.configure()).failed_count(), 0);
    assert!(stamp.exists());
    assert!(matches!(f.configure(), TaskResult::Ok));
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
}

#[test]
fn cancelled_recipe_stops_later_sequential_dispatch_without_caching_failure() {
    let f = SnapshotFixture::new();
    let ctx = f.ctx.with_parallel(false);
    let results = map_recipes(&ctx, &[0, 1, 2], |item| -> Result<()> {
        assert_eq!(*item, 0, "no recipe may start after cancellation");
        Err(crate::infra::exec::ExecError::Cancelled {
            command: "fixture metadata".into(),
            result: ExecResult::failure("", "", None),
        }
        .into())
    });
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
