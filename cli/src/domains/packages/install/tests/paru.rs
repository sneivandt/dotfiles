use super::*;

#[test]
fn installed_target_paru_is_accepted_when_host_path_lookup_would_miss() {
    let mut mock = MockExecutor::new();
    expect_installed_paru_package(&mut mock, 1);
    expect_healthy_paru(&mut mock, 1);
    let ctx = make_package_context(PathBuf::from("/tmp"), None, Os::Linux, true, mock);
    let result = InstallParu::new().run(&ctx).unwrap();
    assert_task_ok(&result);
}

#[test]
fn install_paru_run_returns_ok_when_already_installed_in_dry_run() {
    let mut mock = MockExecutor::new();
    expect_installed_paru_package(&mut mock, 1);
    expect_healthy_paru(&mut mock, 1);
    let mut ctx = make_package_context(PathBuf::from("/tmp"), None, Os::Linux, true, mock);
    ctx = ctx.with_dry_run(true);
    let result = InstallParu::new().run(&ctx).unwrap();
    assert_task_ok(&result);
}

#[test]
fn install_paru_run_returns_dry_run_when_not_installed_in_dry_run() {
    let mut mock = MockExecutor::new();
    expect_missing_paru_package(&mut mock, 1);
    expect_missing_paru_path(&mut mock, 1);
    let mut ctx = make_package_context(PathBuf::from("/tmp"), None, Os::Linux, true, mock);
    ctx = ctx.with_dry_run(true);
    let result = InstallParu::new().run(&ctx).unwrap();
    assert_task_changed(&result);
}

#[test]
fn paru_previews_keep_install_and_rebuild_verbs_separate_from_context() {
    for (plan, verb, subject) in [
        (
            ParuInstallPlan::Install {
                reason: "not installed".into(),
            },
            "install",
            "missing paru from AUR source",
        ),
        (
            ParuInstallPlan::Rebuild {
                path: PathBuf::from("/fixture/bin/paru"),
                reason: "missing shared library".into(),
            },
            "rebuild",
            "broken paru from AUR source",
        ),
    ] {
        let output = Arc::new(CapturingOutput::default());
        let ctx = make_package_context(
            PathBuf::from("/fixture"),
            None,
            Os::Linux,
            true,
            MockExecutor::new(),
        )
        .with_log(Arc::<CapturingOutput>::clone(&output))
        .with_dry_run(true);
        let operation = ParuInstallOperation {
            build_root: Path::new("/fixture"),
            inventory_cache: None,
        };
        assert_task_changed(&operation.preview(&ctx, &plan).unwrap());
        let actions = output.actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].verb, verb);
        assert_eq!(actions[0].subject, subject);
        assert!(actions[0].planned);
        assert!(
            actions[0]
                .message
                .starts_with(&format!("{verb} {subject} · "))
        );
    }
}

#[test]
fn install_paru_reports_change_and_cleans_its_fixture_build_directory() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let legacy = fixture.path().join("paru-build");
    std::fs::create_dir(&legacy).unwrap();
    std::fs::write(legacy.join("keep"), "existing user checkout").unwrap();
    let unrelated = fixture.path().join("unrelated");
    std::fs::create_dir(&unrelated).unwrap();
    std::fs::write(unrelated.join("keep"), "unrelated content").unwrap();

    let mut mock = MockExecutor::new();
    let package_queries = Arc::new(AtomicUsize::new(0));
    mock.expect_execute()
        .times(2)
        .withf(is_paru_package_query)
        .returning(move |_| {
            if package_queries.fetch_add(1, Ordering::SeqCst) == 0 {
                return Ok(ExecResult::failure(
                    "",
                    "error: package 'paru' was not found",
                    Some(1),
                ));
            }
            Ok(ExecResult::success("paru 2.1.0-2\n"))
        });
    mock.expect_execute()
        .once()
        .withf(is_native_inventory)
        .returning(|_| Ok(ExecResult::success("base-devel 1.0\n")));
    mock.expect_which_path()
        .once()
        .with(mockall::predicate::eq("paru"))
        .returning(|_| anyhow::bail!("paru not found on PATH"));
    expect_paru_build_prerequisites(&mut mock);
    let build_dir = expect_paru_clone(&mut mock, fixture.path());
    mock.expect_execute()
        .once()
        .withf(is_makepkg(Arc::clone(&build_dir)))
        .returning(|_| Ok(ExecResult::success("")));
    expect_healthy_paru(&mut mock, 1);

    let output = Arc::new(CapturingOutput::default());
    let ctx = make_package_context(fixture.path().to_path_buf(), None, Os::Linux, true, mock)
        .with_log(Arc::<CapturingOutput>::clone(&output));
    let result = run_paru_install(&ctx, fixture.path()).unwrap();
    let stats = task_batch(&result);
    assert!(
        stats.changed_count() > 0 && stats.message() == Some("installed paru"),
        "expected changed result after paru install, got {result:?}"
    );
    let actions = output.actions();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].verb, "install");
    assert_eq!(actions[0].subject, "paru");
    assert!(!actions[0].planned);
    assert!(
        actions[0]
            .message
            .starts_with("install paru · target validated · ")
    );
    assert!(
        !build_dir.get().unwrap().exists(),
        "successful installation must clean its checkout"
    );
    assert_eq!(
        std::fs::read_to_string(legacy.join("keep")).unwrap(),
        "existing user checkout",
        "a predictable old build directory is not owned by this invocation"
    );
    assert_eq!(
        std::fs::read_to_string(unrelated.join("keep")).unwrap(),
        "unrelated content"
    );
}

#[test]
fn paru_preview_and_completed_state_preserve_existing_build_files() {
    for (label, installed, dry_run) in [
        ("already installed", true, false),
        ("installed preview", true, true),
        ("missing preview", false, true),
    ] {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let build_dir = fixture.path().join("paru-build");
        std::fs::create_dir(&build_dir).unwrap();
        let marker = build_dir.join("keep");
        std::fs::write(&marker, "existing checkout").unwrap();

        let mut mock = MockExecutor::new();
        if installed {
            expect_installed_paru_package(&mut mock, 1);
            expect_healthy_paru(&mut mock, 1);
        } else {
            expect_missing_paru_package(&mut mock, 1);
            expect_missing_paru_path(&mut mock, 1);
        }
        let ctx = make_package_context(fixture.path().to_path_buf(), None, Os::Linux, true, mock)
            .with_dry_run(dry_run);

        let result = run_paru_install(&ctx, &build_dir).unwrap();

        if installed {
            assert_task_ok(&result);
        } else {
            assert_task_changed(&result);
        }
        assert_eq!(
            std::fs::read_to_string(marker).unwrap(),
            "existing checkout",
            "{label}"
        );
    }
}

#[test]
fn failed_paru_clone_cleans_only_its_partial_checkout() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let build_dir = Arc::new(OnceLock::<PathBuf>::new());
    let marker = fixture.path().join("keep");
    std::fs::write(&marker, "unrelated content").unwrap();

    let mut mock = MockExecutor::new();
    expect_missing_paru_package(&mut mock, 1);
    expect_missing_paru_path(&mut mock, 1);
    expect_paru_build_prerequisites(&mut mock);
    let partial = Arc::clone(&build_dir);
    mock.expect_execute()
        .once()
        .withf(is_git_clone(fixture.path().into()))
        .returning(move |spec| {
            let path = PathBuf::from(spec.arguments().last().unwrap());
            assert!(
                path.is_dir(),
                "the build directory must be reserved before cloning"
            );
            std::fs::write(path.join("partial"), "incomplete checkout").unwrap();
            partial.set(path).unwrap();
            Err(ExecError::non_zero(
                "git clone",
                ExecResult::failure("", "fixture clone failed", Some(128)),
            ))
        });
    let ctx = make_package_context(fixture.path().to_path_buf(), None, Os::Linux, true, mock);

    let error = run_paru_install(&ctx, fixture.path()).unwrap_err();

    assert!(
        format!("{error:#}").contains("fixture clone failed"),
        "{error:#}"
    );
    assert!(
        !build_dir.get().unwrap().exists(),
        "failed clone must clean its partial checkout"
    );
    assert_eq!(
        std::fs::read_to_string(marker).unwrap(),
        "unrelated content"
    );
}

#[test]
fn install_aur_packages_errors_when_paru_disappears_after_bootstrap() {
    let packages = packages(&[("paru-bin", true)]);
    let mut mock = MockExecutor::new();
    expect_missing_paru_package(&mut mock, 1);
    expect_missing_paru_path(&mut mock, 1);
    let ctx = make_package_context(PathBuf::from("/tmp"), None, Os::Linux, true, mock);
    let error = InstallAurPackages::new(packages).run(&ctx).unwrap_err();
    assert!(
        error.to_string().contains("became unavailable"),
        "expected explicit post-bootstrap health error, got {error:#}"
    );
}

fn cargo_path() -> PathBuf {
    PathBuf::from("/usr/bin/cargo")
}

fn is_git_clone(build_root: PathBuf) -> impl Fn(&CommandSpec) -> bool {
    move |spec| {
        spec.program() == "git"
            && matches!(spec.arguments(), [command, url, destination]
                if command == "clone"
                    && url == "https://aur.archlinux.org/paru.git"
                    && Path::new(destination).parent() == Some(build_root.as_path())
                    && Path::new(destination).file_name().unwrap().to_str().unwrap().starts_with("paru-build-"))
            && spec.is_checked()
            && spec.working_dir().is_none()
    }
}

fn expect_paru_clone(mock: &mut MockExecutor, build_root: &Path) -> Arc<OnceLock<PathBuf>> {
    let build_dir = Arc::new(OnceLock::new());
    let captured = Arc::clone(&build_dir);
    mock.expect_execute()
        .once()
        .withf(is_git_clone(build_root.into()))
        .returning(move |spec| {
            let path = PathBuf::from(spec.arguments().last().unwrap());
            assert_eq!(
                std::fs::read_dir(&path).unwrap().count(),
                0,
                "clone must receive an exclusively reserved empty directory"
            );
            std::fs::write(path.join("PKGBUILD"), "fixture checkout").unwrap();
            captured.set(path).unwrap();
            Ok(ExecResult::success(""))
        });
    build_dir
}

fn is_cargo_version(spec: &CommandSpec) -> bool {
    spec.program() == cargo_path().as_os_str()
        && spec.arguments() == ["--version"]
        && spec.is_checked()
        && spec.working_dir().is_none()
}

fn is_makepkg(build_dir: Arc<OnceLock<PathBuf>>) -> impl Fn(&CommandSpec) -> bool {
    move |spec| {
        spec.program() == "makepkg"
            && spec.arguments() == ["-si", "--noconfirm"]
            && spec.is_checked()
            && spec.working_dir() == build_dir.get().map(PathBuf::as_path)
            && matches!(spec.environment(), [(key, value)] if key == "MAKEFLAGS"
                && value.to_str().and_then(|value| value.strip_prefix("-j"))
                    .and_then(|jobs| jobs.parse::<usize>().ok()).is_some_and(|jobs| jobs > 0))
    }
}

fn expect_missing_paru_path(mock: &mut MockExecutor, times: usize) {
    mock.expect_which_path()
        .times(times)
        .with(mockall::predicate::eq("paru"))
        .returning(|_| anyhow::bail!("paru not found on PATH"));
}

fn expect_missing_paru_package(mock: &mut MockExecutor, times: usize) {
    mock.expect_execute()
        .times(times)
        .withf(is_paru_package_query)
        .returning(|_| {
            Ok(ExecResult::failure(
                "",
                "error: package 'paru' was not found",
                Some(1),
            ))
        });
    mock.expect_execute()
        .times(times)
        .withf(is_native_inventory)
        .returning(|_| Ok(ExecResult::success("base-devel 1.0\n")));
}

#[test]
fn paru_health_marks_nonzero_executable_broken() {
    let mut mock = MockExecutor::new();
    expect_installed_paru_package(&mut mock, 1);
    mock.expect_execute()
        .once()
        .withf(is_paru_version)
        .returning(|_| {
            Err(ExecError::non_zero(
                "/usr/bin/paru --version",
                ExecResult::failure("", "unexpected failure", Some(1)),
            ))
        });

    let health = check_paru_health(&mock).unwrap();

    assert!(matches!(
        health,
        ParuHealth::Broken { path, reason }
            if path == paru_path() && reason.contains("unexpected failure")
    ));
}

#[test]
fn paru_health_preserves_missing_libalpm_failure() {
    let mut mock = MockExecutor::new();
    expect_installed_paru_package(&mut mock, 1);
    mock.expect_execute()
        .once()
        .withf(is_paru_version)
        .returning(|_| {
            Err(ExecError::non_zero(
                "/usr/bin/paru --version",
                ExecResult::failure(
                    "",
                    "paru: error while loading shared libraries: libalpm.so.15: cannot open shared object file: No such file or directory",
                    Some(127),
                ),
            ))
        });

    let health = check_paru_health(&mock).unwrap();

    assert!(matches!(
        health,
        ParuHealth::Broken { path, reason }
            if path == paru_path() && reason.contains("libalpm.so.15")
    ));
}

#[test]
fn paru_health_marks_path_executable_without_target_package_broken() {
    let mut mock = MockExecutor::new();
    expect_missing_paru_package(&mut mock, 1);
    mock.expect_which_path()
        .once()
        .with(mockall::predicate::eq("paru"))
        .returning(|_| Ok(PathBuf::from("/usr/local/bin/paru")));

    let health = check_paru_health(&mock).unwrap();

    assert!(matches!(
        health,
        ParuHealth::Broken { path, reason }
            if path == Path::new("/usr/local/bin/paru")
                && reason.contains("not backed by the target package database")
    ));
}

#[test]
fn paru_prerequisites_reject_missing_cargo_with_arch_guidance() {
    let mut mock = MockExecutor::new();
    expect_paru_native_build_tools(&mut mock);
    mock.expect_which_path()
        .once()
        .with(mockall::predicate::eq("cargo"))
        .returning(|_| anyhow::bail!("cargo not found on PATH"));

    let ctx = make_package_context(PathBuf::from("/tmp"), None, Os::Linux, true, mock);
    let error = check_prerequisites(&ctx).unwrap_err();
    let message = format!("{error:#}");

    assert!(message.contains("missing prerequisite: cargo"), "{message}");
    assert!(message.contains("pacman -Syu --needed rust"), "{message}");
}

#[test]
fn paru_inventory_failure_never_plans_a_bootstrap_or_rebuild() {
    for dry_run in [false, true] {
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .withf(is_paru_package_query)
            .returning(|_| {
                Ok(ExecResult::failure(
                    "",
                    "could not open package database",
                    Some(1),
                ))
            });
        mock.expect_execute()
            .once()
            .withf(is_native_inventory)
            .returning(|_| {
                Err(ExecError::non_zero(
                    "pacman -Q",
                    ExecResult::failure("", "fixture unreadable package database", Some(1)),
                ))
            });

        let ctx = make_package_context(
            PathBuf::from("fixture-repository"),
            None,
            Os::Linux,
            true,
            mock,
        )
        .with_dry_run(dry_run);

        let error = InstallParu::new().run(&ctx).unwrap_err();

        assert!(
            format!("{error:#}").contains("fixture unreadable package database"),
            "{error:#}"
        );
    }
}

#[test]
fn paru_targeted_query_failure_is_not_absence_when_inventory_contains_paru() {
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .once()
        .withf(is_paru_package_query)
        .returning(|_| Ok(ExecResult::failure("", "fixture query failure", Some(1))));
    mock.expect_execute()
        .once()
        .withf(is_native_inventory)
        .returning(|_| Ok(ExecResult::success("paru 2.1.0-2\n")));

    let error = check_paru_health(&mock).unwrap_err();

    assert!(format!("{error:#}").contains("fixture query failure"));
}

#[test]
fn paru_probe_errors_are_not_converted_to_rebuild_plans() {
    #[derive(Debug, Clone, Copy)]
    enum Failure {
        Cancelled,
        Timeout,
        Spawn,
        Capture,
    }

    for stage in ["package query", "executable"] {
        for failure in [
            Failure::Cancelled,
            Failure::Timeout,
            Failure::Spawn,
            Failure::Capture,
        ] {
            for dry_run in [false, true] {
                let mut mock = MockExecutor::new();
                if stage == "executable" {
                    expect_installed_paru_package(&mut mock, 1);
                }
                mock.expect_execute()
                    .once()
                    .withf(move |spec| {
                        if stage == "executable" {
                            is_paru_version(spec)
                        } else {
                            is_paru_package_query(spec)
                        }
                    })
                    .returning(move |_| {
                        let command = "fixture paru probe".to_string();
                        let result = ExecResult::failure("", "", None);
                        Err(match failure {
                            Failure::Cancelled => ExecError::Cancelled { command, result },
                            Failure::Timeout => ExecError::TimedOut {
                                command,
                                timeout: std::time::Duration::from_secs(1),
                                result,
                            },
                            Failure::Spawn => ExecError::spawn(
                                command,
                                std::io::Error::from(std::io::ErrorKind::PermissionDenied),
                            ),
                            Failure::Capture => ExecError::Io {
                                command,
                                operation: "capturing output",
                                source: std::io::Error::other("fixture capture failure"),
                            },
                        })
                    });

                let ctx = make_package_context(
                    PathBuf::from("fixture-repository"),
                    None,
                    Os::Linux,
                    true,
                    mock,
                )
                .with_dry_run(dry_run);

                let error = InstallParu::new().run(&ctx).unwrap_err();

                let exec_error = error.downcast_ref::<ExecError>().unwrap();
                assert_eq!(
                    exec_error.is_cancelled(),
                    matches!(failure, Failure::Cancelled)
                );
                assert!(
                    format!("{error:#}").contains("fixture paru probe"),
                    "{stage}/{failure:?}: {error:#}"
                );
            }
        }
    }
}

#[test]
fn paru_probe_failure_does_not_request_elevation() {
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .once()
        .withf(is_paru_package_query)
        .returning(|_| {
            Err(ExecError::spawn(
                "pacman",
                std::io::Error::from(std::io::ErrorKind::PermissionDenied),
            ))
        });

    let ctx = make_package_context(
        PathBuf::from("fixture-repository"),
        None,
        Os::Linux,
        true,
        mock,
    );

    assert!(!InstallParu::new().needs_elevation(&ctx));
}

#[test]
fn paru_prerequisites_reject_unconfigured_rustup_cargo() {
    let mut mock = MockExecutor::new();
    expect_paru_native_build_tools(&mut mock);
    expect_cargo_path(&mut mock);
    mock.expect_execute()
        .once()
        .withf(is_cargo_version)
        .returning(|_| {
            Err(ExecError::non_zero(
                "/usr/bin/cargo --version",
                ExecResult::failure(
                    "",
                    "rustup could not choose a version of cargo to run because no default is configured",
                    Some(1),
                ),
            ))
        });

    let ctx = make_package_context(PathBuf::from("/tmp"), None, Os::Linux, true, mock);
    let error = check_prerequisites(&ctx).unwrap_err();
    let message = format!("{error:#}");

    assert!(
        message.contains("Rust/Cargo prerequisite is incomplete"),
        "{message}"
    );
    assert!(message.contains("rustup default stable"), "{message}");
    assert!(message.contains("no default is configured"), "{message}");
}

#[test]
fn broken_paru_is_rebuilt_and_revalidated() {
    let fixture = tempfile::tempdir_in(".").unwrap();

    let mut mock = MockExecutor::new();
    expect_installed_paru_package(&mut mock, 2);
    let checks = Arc::new(AtomicUsize::new(0));
    mock.expect_execute()
        .times(2)
        .withf(is_paru_version)
        .returning(move |_| {
            if checks.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(ExecError::non_zero(
                    "/usr/bin/paru --version",
                    ExecResult::failure("", "libalpm.so.15 not found", Some(127)),
                ));
            }
            Ok(ExecResult::success("paru v2.1.0 - libalpm v16.0.1\n"))
        });
    expect_paru_build_prerequisites(&mut mock);
    let build_dir = expect_paru_clone(&mut mock, fixture.path());
    mock.expect_execute()
        .once()
        .withf(is_makepkg(Arc::clone(&build_dir)))
        .returning(|_| Ok(ExecResult::success("")));

    let output = Arc::new(CapturingOutput::default());
    let ctx = make_package_context(fixture.path().to_path_buf(), None, Os::Linux, true, mock)
        .with_log(Arc::<CapturingOutput>::clone(&output));
    let result = run_paru_install(&ctx, fixture.path()).unwrap();
    let stats = task_batch(&result);

    assert_eq!(stats.message(), Some("rebuilt paru"));
    let actions = output.actions();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].verb, "rebuild");
    assert_eq!(actions[0].subject, "paru");
    assert!(!actions[0].planned);
    assert!(
        actions[0]
            .message
            .starts_with("rebuild paru · target validated · ")
    );
    assert!(
        !build_dir.get().unwrap().exists(),
        "successful rebuild must clean its checkout"
    );
}

#[test]
fn failed_paru_rebuild_returns_clear_root_error() {
    let fixture = tempfile::tempdir_in(".").unwrap();

    let mut mock = MockExecutor::new();
    let build_dir = expect_failed_paru_rebuild(&mut mock, fixture.path());

    let ctx = make_package_context(fixture.path().to_path_buf(), None, Os::Linux, true, mock);
    let error = run_paru_install(&ctx, fixture.path()).unwrap_err();
    let message = format!("{error:#}");

    assert!(message.contains("paru rebuild attempt failed"), "{message}");
    assert!(message.contains("package build failed"), "{message}");
    assert!(
        !build_dir.get().unwrap().exists(),
        "failed rebuild must clean its checkout"
    );
}

#[test]
fn failed_paru_rebuild_blocks_aur_package_task() {
    use std::collections::HashMap;

    use crate::engine::graph::ResolvedTaskGraph;
    use crate::engine::{TaskAssessment, TaskId, TaskMeta};
    use crate::infra::logging::{Log, TaskStatus};

    struct FixtureParuTask {
        inner: InstallParu,
        build_dir: PathBuf,
    }

    impl Task for FixtureParuTask {
        fn meta(&self) -> TaskMeta<'_> {
            self.inner.meta()
        }

        fn task_id(&self) -> TaskId {
            // Preserve the real AUR dependency while injecting only the checkout path.
            self.inner.task_id()
        }

        fn run(&self, ctx: &Context) -> Result<TaskResult> {
            run_paru_install(ctx, &self.build_dir)
        }
    }

    let fixture = tempfile::tempdir_in(".").unwrap();
    let packages = packages(&[("apm-bin", true)]);
    let mut mock = MockExecutor::new();
    let build_dir = expect_failed_paru_rebuild(&mut mock, fixture.path());

    let (log, _tmp, _guard) = crate::infra::logging::isolated_logger();
    let log = Arc::new(log);
    let log_output: Arc<dyn Log> = Arc::<crate::infra::logging::Logger>::clone(&log);
    let ctx = make_package_context(fixture.path().to_path_buf(), None, Os::Linux, true, mock)
        .with_log(log_output);
    let install_paru = FixtureParuTask {
        inner: InstallParu::new(),
        build_dir: fixture.path().into(),
    };
    let install_aur = InstallAurPackages::new(packages);
    let tasks: Vec<&dyn Task> = vec![&install_paru, &install_aur];
    let graph = ResolvedTaskGraph::resolve(&tasks).unwrap();
    let assessments = tasks
        .iter()
        .map(|task| (task.task_id(), TaskAssessment::applicable()))
        .collect::<HashMap<_, _>>();

    let summary = crate::engine::scheduler::run_tasks(
        &tasks,
        &graph,
        &assessments,
        &ctx.with_parallel(false),
        &log,
        None,
    );

    assert_eq!(summary.failure_count(), 1);
    assert!(
        !build_dir.get().unwrap().exists(),
        "scheduler failure must clean the fixture checkout"
    );
    let entries = log.task_entries();
    let paru_entry = entries
        .iter()
        .find(|entry| entry.name == "Paru package manager")
        .expect("paru task entry");
    assert_eq!(paru_entry.status, TaskStatus::Failed);
    assert!(
        paru_entry
            .message
            .as_deref()
            .is_some_and(|message| message.contains("paru rebuild attempt failed")),
        "root task should retain the rebuild failure: {paru_entry:?}"
    );
    let aur_entry = entries
        .iter()
        .find(|entry| entry.name == "AUR packages")
        .expect("AUR task entry");
    assert_eq!(aur_entry.status, TaskStatus::Blocked);
    assert_eq!(
        aur_entry.message.as_deref(),
        Some("blocked by failed dependency: Paru package manager")
    );

    let run_log = std::fs::read_to_string(log.log_path().expect("run log path")).unwrap();
    for diagnostic in [
        "paru status: broken",
        "executable /usr/bin/paru",
        "libalpm.so.15 not found",
        "paru rebuild attempted",
        "paru rebuild attempt failed",
    ] {
        assert!(
            run_log.contains(diagnostic),
            "run log should contain {diagnostic:?}:\n{run_log}"
        );
    }
}

fn expect_failed_paru_rebuild(
    mock: &mut MockExecutor,
    build_root: &Path,
) -> Arc<OnceLock<PathBuf>> {
    expect_installed_paru_package(mock, 1);
    mock.expect_execute()
        .once()
        .withf(is_paru_version)
        .returning(|_| {
            Err(ExecError::non_zero(
                "/usr/bin/paru --version",
                ExecResult::failure("", "libalpm.so.15 not found", Some(127)),
            ))
        });
    expect_paru_build_prerequisites(mock);
    let build_dir = expect_paru_clone(mock, build_root);
    mock.expect_execute()
        .once()
        .withf(is_makepkg(Arc::clone(&build_dir)))
        .returning(|_| {
            Err(ExecError::non_zero(
                "makepkg -si --noconfirm",
                ExecResult::failure("", "package build failed", Some(4)),
            ))
        });
    build_dir
}

fn expect_paru_native_build_tools(mock: &mut MockExecutor) {
    for dependency in ["git", "makepkg", "sudo"] {
        mock.expect_which()
            .once()
            .with(mockall::predicate::eq(dependency))
            .returning(|_| true);
    }
}

fn expect_cargo_path(mock: &mut MockExecutor) {
    mock.expect_which_path()
        .once()
        .with(mockall::predicate::eq("cargo"))
        .returning(|_| Ok(cargo_path()));
}

fn expect_working_cargo(mock: &mut MockExecutor) {
    expect_cargo_path(mock);
    mock.expect_execute()
        .once()
        .withf(is_cargo_version)
        .returning(|_| Ok(ExecResult::success("cargo 1.97.1\n")));
}

fn expect_paru_build_prerequisites(mock: &mut MockExecutor) {
    expect_paru_native_build_tools(mock);
    expect_working_cargo(mock);
}
