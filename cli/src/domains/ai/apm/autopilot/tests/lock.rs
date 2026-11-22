use super::*;

#[test]
fn parse_autopilot_result_cases() {
    let cases: Vec<AutopilotResultCase> = vec![
        (
            "reads header and ids",
            "3 3\napm--a\napm--b\napm--c\n",
            Some((3, id_set(&["apm--a", "apm--b", "apm--c"]))),
        ),
        (
            "allows a header-only, zero-id output",
            "0 0\n",
            Some((0, id_set(&[]))),
        ),
        ("rejects a three-token header", "3 3 oops\napm--a\n", None),
        ("rejects fully empty output", "", None),
        ("rejects blank-line-only output", "\n\n", None),
    ];
    for (case, input, expected) in cases {
        assert_eq!(parse_autopilot_result(input), expected, "case: {case}");
    }
}

#[test]
fn parse_desired_ids_cases() {
    let cases = [
        (
            "filters blank lines and trims whitespace",
            "apm--a\n\n  apm--b  \n\n",
            id_set(&["apm--a", "apm--b"]),
        ),
        ("empty input is an empty set", "", id_set(&[])),
    ];
    for (case, input, expected) in cases {
        assert_eq!(parse_desired_ids(input), expected, "case: {case}");
    }
}

#[test]
fn parse_deployed_workflow_ids_extracts_only_workflow_uris() {
    let lock = "\
lockfile_version: '1'
dependencies:
- repo_url: _local/dot-code
  deployed_files:
  - .agents/skills/project-hygiene
- repo_url: github/awesome-copilot
  deployed_files:
  - copilot-app-db://workflows/apm--awesome-copilot--planning--triage
  - .agents/skills/foo
  - copilot-app-db://workflows/apm--awesome-copilot--planning--report
- repo_url: dotnet/skills
  deployed_files:
  - copilot-app-db://workflows/apm--dotnet--diag--collect
";
    let ids = parse_deployed_workflow_ids(lock).unwrap();
    let expected: BTreeSet<String> = [
        "apm--awesome-copilot--planning--triage",
        "apm--awesome-copilot--planning--report",
        "apm--dotnet--diag--collect",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();
    assert_eq!(ids, expected);
}

#[test]
fn parse_deployed_workflow_ids_empty_cases() {
    let cases = [
        (
            "no workflows deployed",
            "dependencies:\n- repo_url: _local/dot-code\n  deployed_files:\n  - \
             .agents/skills/project-hygiene\n",
        ),
        ("mapping without dependencies", "name: x\nversion: 1\n"),
        (
            "dependency without deployed_files",
            "dependencies:\n- repo_url: a/b\n",
        ),
    ];
    for (case, lock) in cases {
        assert!(
            parse_deployed_workflow_ids(lock).unwrap().is_empty(),
            "case: {case}"
        );
    }
}

#[test]
fn malformed_workflow_ownership_is_not_a_successful_empty_repair() {
    let home = tempfile::tempdir_in(".").unwrap();
    let ctx = make_home_context_with_executor(home.path(), MockExecutor::new());
    std::fs::create_dir(home.path().join(".apm")).unwrap();
    let lock = home.path().join(".apm/apm.lock.yaml");
    for input in [
        "lock",
        ": : not yaml : :",
        "dependencies: {}",
        "dependencies: [malformed]",
        "dependencies: [{deployed_files: {}}]",
        "dependencies: [{deployed_files: [false]}]",
        "dependencies: [{deployed_files: [copilot-app-db://workflows/]}]",
    ] {
        std::fs::write(&lock, input).unwrap();
        assert!(
            super::super::apply_workflow_autopilot_fixup(&ctx, &DesiredApmWorkflows::FirstInstall)
                .is_err(),
            "{input}"
        );
        assert_eq!(std::fs::read_to_string(&lock).unwrap(), input);
    }
    std::fs::write(&lock, "dependencies: []").unwrap();
    assert_eq!(
        super::super::apply_workflow_autopilot_fixup(&ctx, &DesiredApmWorkflows::FirstInstall)
            .unwrap(),
        super::super::WorkflowRepair::NotApplicable
    );
}

#[test]
fn build_workflow_script_args_appends_ids_in_order() {
    let ids = vec!["apm--a".to_string(), "apm--b".to_string()];
    let args = build_workflow_script_args(WORKFLOW_AUTOPILOT_SCRIPT, "/db", &ids, None);
    assert_eq!(
        args,
        ["-c", WORKFLOW_AUTOPILOT_SCRIPT, "/db", "apm--a", "apm--b"]
    );
}
