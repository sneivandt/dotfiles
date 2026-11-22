use super::*;

#[test]
fn cancelled_winget_task_preserves_completed_and_initially_current_counts() {
    let packages = packages(&[
        ("First.App", false),
        ("Second.App", false),
        ("Third.App", false),
        ("Existing.App", false),
    ]);
    let mut mock = MockExecutor::new();
    mock.expect_which().return_const(true);
    let mut sequence = mockall::Sequence::new();
    mock.expect_execute()
        .once()
        .in_sequence(&mut sequence)
        .returning(|spec| {
            crate::domains::packages::resources::mock_inventory(&spec, &["Existing.App"]);
            Ok(ExecResult::success(""))
        });
    mock.expect_execute()
        .once()
        .in_sequence(&mut sequence)
        .with(mockall::predicate::eq(winget_install("First.App")))
        .returning(|_| Ok(ExecResult::success("installed")));
    mock.expect_execute()
        .once()
        .in_sequence(&mut sequence)
        .with(mockall::predicate::eq(winget_install("Second.App")))
        .returning(|_| {
            Err(ExecError::Cancelled {
                command: "winget install".into(),
                result: ExecResult::failure("", "", None),
            })
        });
    let ctx = make_package_context(
        PathBuf::from("fixture-repository"),
        None,
        Os::Windows,
        false,
        mock,
    );

    let error = InstallPackages::new(packages).run(&ctx).unwrap_err();
    let report = error.downcast_ref::<BatchReport>().unwrap();
    assert_eq!(report.completion(), BatchCompletion::Interrupted);
    assert_eq!(report.stats().changed_count(), 1);
    assert_eq!(report.stats().already_ok_count(), 1);
    assert_eq!(report.stats().failed_count(), 0);
    assert_eq!(report.interrupted_count(), 1);
    assert_eq!(report.not_attempted_count(), 1);
    assert!(error.downcast_ref::<ExecError>().unwrap().is_cancelled());
}

#[test]
fn winget_install_matches_ascii_case_and_installs_only_missing_configured_ids() {
    for (configured_git, install_result, expected) in [
        ("git.git", ExecResult::success(""), (1, 1, 0)),
        (
            "gIt.GIT",
            ExecResult::failure("No available upgrade found.", "", Some(-1_978_335_189)),
            (0, 2, 0),
        ),
    ] {
        let mut mock = MockExecutor::new();
        let mut sequence = mockall::Sequence::new();
        mock.expect_which()
            .once()
            .with(mockall::predicate::eq("winget"))
            .return_const(true);
        mock.expect_execute()
            .once()
            .in_sequence(&mut sequence)
            .returning(|spec| {
                crate::domains::packages::resources::mock_inventory(
                    &spec,
                    &["Git.Git", "Äpp.Tool"],
                );
                Ok(ExecResult::success(""))
            });
        mock.expect_execute()
            .once()
            .in_sequence(&mut sequence)
            .with(mockall::predicate::eq(winget_install("äpp.ToOl")))
            .return_once(|_| Ok(install_result));
        let ctx = make_package_context("fixture-repository".into(), None, Os::Windows, false, mock);
        // ASCII case changes don't make Git missing; ä and Ä are distinct.
        let entries = packages(&[(configured_git, false), ("äpp.ToOl", false)]);
        let result = InstallPackages::new(entries).run(&ctx).unwrap();
        let stats = task_batch(&result);
        assert_eq!(
            (
                stats.changed_count(),
                stats.already_ok_count(),
                stats.failed_count()
            ),
            expected
        );
    }
}

#[test]
fn winget_discovery_parse_failure_never_attempts_installation() {
    for dry_run in [false, true] {
        let packages = packages(&[("Git.Git", false)]);
        let mut mock = MockExecutor::new();
        mock.expect_which()
            .once()
            .withf(|program| program == "winget")
            .returning(|_| true);
        mock.expect_execute()
            .once()
            .withf(|spec| {
                spec.program() == "winget"
                    && spec
                        .arguments()
                        .first()
                        .is_some_and(|argument| argument == "export")
                    && spec.is_checked()
            })
            .returning(|spec| {
                crate::domains::packages::resources::mock_inventory(&spec, &[]);
                std::fs::write(&spec.arguments()[2], r#"{"Sources":[{}]}"#).unwrap();
                Ok(ExecResult::success(""))
            });
        let ctx = make_package_context(
            PathBuf::from("fixture-repository"),
            None,
            Os::Windows,
            false,
            mock,
        )
        .with_dry_run(dry_run);

        let error = InstallPackages::new(packages)
            .run(&ctx)
            .expect_err("unknown inventory must not plan every package for installation");

        assert!(error.to_string().contains("could not parse winget export"));
    }
}
