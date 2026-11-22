use super::*;
use std::io::Seek;

#[test]
fn unfiltered_raw_logs_are_copied_without_parsing_or_rewinding() {
    #[derive(Debug)]
    struct NoRewind(std::io::Cursor<Vec<u8>>);
    impl std::io::Read for NoRewind {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(buffer)
        }
    }
    impl BufRead for NoRewind {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            self.0.fill_buf()
        }
        fn consume(&mut self, amount: usize) {
            self.0.consume(amount);
        }
    }
    impl Seek for NoRewind {
        fn seek(&mut self, _: std::io::SeekFrom) -> std::io::Result<u64> {
            panic!("raw output must not perform an index pass")
        }
    }
    let contents = b"legacy \xff bytes\r\nwithout trailing newline";
    let mut output = Vec::new();
    write_selected_log(
        &mut NoRewind(std::io::Cursor::new(contents.to_vec())),
        false,
        true,
        None,
        &mut output,
    )
    .unwrap();
    assert_eq!(output, contents);
}

#[test]
fn empty_history_preserves_machine_readable_formats() {
    for case in ["missing directory", "empty directory", "unmatched command"] {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let logs = fixture.path().join("logs");
        if case == "empty directory" {
            std::fs::create_dir(&logs).unwrap();
        } else if case == "unmatched command" {
            write_run(&logs, "20260731T154210Z-install-1.log", "fixture\n");
        }
        for format in [DiscoveryFormat::Json, DiscoveryFormat::Plain] {
            let output = capture(
                &logs,
                &LogOpts {
                    list: true,
                    format: Some(format),
                    command: Some(crate::app::cli::LogCommand::Update),
                    ..opts()
                },
                false,
            );
            if format == DiscoveryFormat::Json {
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&output).unwrap(),
                    serde_json::json!([]),
                    "{case}: JSON history must always be an array"
                );
            } else {
                assert!(
                    output.is_empty(),
                    "{case}: plain history must contain only rows"
                );
            }
        }
        assert_eq!(
            logs.exists(),
            case != "missing directory",
            "listing history must not create its directory"
        );
    }
}

#[test]
fn prints_latest_run_by_default() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    write_run(dir, "20260731T154210Z-install-1.log", "older\n");
    write_run(dir, "20260731T154902Z-update-2.log", "newer\n");

    assert_eq!(capture(dir, &opts(), false), "newer\n");
}

#[test]
fn latest_run_uses_microsecond_start_time_instead_of_filename_order() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    let older = "20260731T154210Z-update-900.log";
    let newer = "20260731T154210Z-install-1000.log";
    write_run(
        dir,
        older,
        "# Dotfiles test 2026-07-31T15:42:10.000001Z\nolder\n",
    );
    write_run(
        dir,
        newer,
        "# Dotfiles test 2026-07-31T15:42:10.000002Z\nnewer\n",
    );
    // Older runs can finish after newer ones without becoming the latest run.
    std::fs::OpenOptions::new()
        .append(true)
        .open(dir.join(older))
        .and_then(|mut file| std::io::Write::write_all(&mut file, b"older finished last\n"))
        .expect("append older run");

    let runs = discover_runs(dir).expect("discover runs");
    assert_eq!(runs.first().unwrap().path, dir.join(newer));
    assert!(capture(dir, &opts(), false).ends_with("newer\n"));
    let previous = LogOpts {
        run: Some(1),
        ..opts()
    };
    assert!(
        capture(dir, &previous, false).ends_with("older\nolder finished last\n"),
        "indexed history must use the same precise ordering"
    );
}

#[test]
fn legacy_and_precise_logs_remain_selectable_together() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    let legacy_id = "20260731T154210Z-update-900";
    write_run(dir, &format!("{legacy_id}.log"), "legacy\n");
    write_run(
        dir,
        "20260731T154210Z-install-1000.log",
        "# Dotfiles test 2026-07-31T15:42:10.000001Z\nprecise\n",
    );
    assert!(capture(dir, &opts(), false).ends_with("precise\n"));
    let legacy = LogOpts {
        id: Some(legacy_id.to_string()),
        ..opts()
    };
    assert_eq!(
        capture(dir, &legacy, false),
        "legacy\n",
        "legacy IDs and headerless contents must remain supported"
    );
}

#[test]
fn selects_run_by_index() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    write_run(dir, "20260731T154210Z-install-1.log", "older\n");
    write_run(dir, "20260731T154902Z-update-2.log", "newer\n");

    let selected = LogOpts {
        run: Some(1),
        ..opts()
    };
    assert_eq!(capture(dir, &selected, false), "older\n");
}

#[test]
fn rejects_out_of_range_index() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    write_run(dir, "20260731T154210Z-install-1.log", "only\n");

    let selected = LogOpts {
        run: Some(7),
        ..opts()
    };
    let mut output = Vec::new();
    let err =
        run_in_dir(dir, &selected, false, &mut output).expect_err("out of range index should fail");
    let message = err.to_string();
    assert!(message.contains("No run at index 7"), "{message}");
    assert!(message.contains("1 retained"), "{message}");
    assert!(message.contains("--list"), "{message}");
}

#[test]
fn explicit_run_indices_fail_when_no_runs_are_retained_or_match() {
    for existing_logs in [false, true] {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let dir = fixture.path().join("logs");
        if existing_logs {
            write_run(&dir, "20260731T154210Z-install-1.log", "installation\n");
        }
        for index in [0, 7] {
            let selected = LogOpts {
                run: Some(index),
                command: Some(crate::app::cli::LogCommand::Check),
                ..opts()
            };
            let mut output = Vec::new();
            let error = run_in_dir(&dir, &selected, false, &mut output)
                .expect_err("an explicit index must identify a retained matching run");
            assert_eq!(
                error.to_string(),
                format!("No run at index {index} (0 retained). {LIST_HINT}")
            );
            assert!(
                output.is_empty(),
                "invalid selection must not emit success output"
            );
            assert_eq!(dir.exists(), existing_logs);
        }
    }
}

#[test]
fn install_and_update_have_separate_command_filters() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    write_run(dir, "20260731T154210Z-install-1.log", "install run\n");
    write_run(dir, "20260731T154902Z-update-2.log", "update run\n");

    let install = LogOpts {
        command: Some(crate::app::cli::LogCommand::Install),
        ..opts()
    };
    assert_eq!(capture(dir, &install, false), "install run\n");
    let update = LogOpts {
        command: Some(crate::app::cli::LogCommand::Update),
        ..opts()
    };
    assert_eq!(capture(dir, &update, false), "update run\n");
}

#[test]
fn canonical_command_filters_include_legacy_aliases() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    write_run(dir, "20260731T154210Z-check-1.log", "check run\n");
    write_run(dir, "20260731T154902Z-test-2.log", "test run\n");

    let filtered = LogOpts {
        command: Some(crate::app::cli::LogCommand::Check),
        ..opts()
    };
    assert_eq!(capture(dir, &filtered, false), "test run\n");

    let listed = LogOpts {
        list: true,
        ..filtered
    };
    let output = capture(dir, &listed, false);
    assert!(output.contains("check"), "{output}");
    assert!(output.contains("test"), "{output}");
}

#[test]
fn lists_runs_newest_first() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    write_run(dir, "20260731T154210Z-install-1.log", "a");
    write_run(dir, "20260731T154902Z-update-2.log", "bb");

    let listed = LogOpts {
        list: true,
        ..opts()
    };
    let output = capture(dir, &listed, false);
    let lines: Vec<&str> = output.lines().collect();
    assert!(lines[0].contains("WHEN"), "{output}");
    assert!(
        lines[1].starts_with("  0  2026-07-31 15:49:02Z  20260731T154902Z-update-2"),
        "{output}"
    );
    assert!(
        lines[2].starts_with("  1  2026-07-31 15:42:10Z  20260731T154210Z-install-1"),
        "{output}"
    );
}

#[test]
fn lists_runs_as_plain_text_and_json() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    write_run(dir, "20260731T154210Z-install-1.log", "a");

    let plain = capture(
        dir,
        &LogOpts {
            list: true,
            format: Some(DiscoveryFormat::Plain),
            ..opts()
        },
        false,
    );
    assert!(
        plain.starts_with("0\t2026-07-31 15:42:10Z\tinstall\t"),
        "{plain}"
    );

    let json = capture(
        dir,
        &LogOpts {
            list: true,
            format: Some(DiscoveryFormat::Json),
            ..opts()
        },
        false,
    );
    let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert_eq!(value[0]["command"], "install");
    assert_eq!(value[0]["size_bytes"], 1);
}

#[test]
fn distinguishes_absent_logs_from_an_empty_filter_result() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_run(tmp.path(), "20260731T154210Z-install-1.log", "a");
    let output = capture(
        tmp.path(),
        &LogOpts {
            command: Some(crate::app::cli::LogCommand::Check),
            ..opts()
        },
        false,
    );
    assert_eq!(output, "No retained runs match the requested filters.\n");
}

#[test]
fn hides_debug_lines_unless_verbose() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    let contents = concat!(
        "# Dotfiles dev-0.0.0\n",
        "000001 +          10 2026-07-31T15:42:10.000000Z [main] [info] visible\n",
        "000002 +          20 2026-07-31T15:42:10.000000Z [main] [debug] hidden\n",
    );
    write_run(dir, "20260731T154210Z-install-1.log", contents);

    let quiet = capture(dir, &opts(), false);
    assert!(quiet.contains("visible"), "{quiet}");
    assert!(!quiet.contains("hidden"), "{quiet}");
    assert!(quiet.contains("# Dotfiles"), "{quiet}");

    let loud = capture(dir, &opts(), true);
    assert_eq!(loud, contents);
}

#[test]
fn ignores_unrelated_files() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    write_run(dir, "notes.txt", "not a run log\n");
    write_run(dir, "install.log", "legacy name\n");

    assert_eq!(capture(dir, &opts(), false), "No dotfiles log found yet.\n");
}

#[test]
fn ignores_directories_named_like_retained_logs() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let dir = fixture.path();
    std::fs::create_dir(dir.join("20260731T154902Z-update-2.log")).unwrap();
    write_run(dir, "20260731T154210Z-update-1.log", "actual log\n");

    assert_eq!(capture(dir, &opts(), false), "actual log\n");
    let listed = capture(
        dir,
        &LogOpts {
            list: true,
            format: Some(DiscoveryFormat::Json),
            ..opts()
        },
        false,
    );
    let runs: Vec<serde_json::Value> = serde_json::from_str(&listed).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["id"], "20260731T154210Z-update-1");
}

#[test]
fn invalid_log_directory_is_reported_instead_of_empty_history() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let dir = fixture.path().join("logs");
    std::fs::write(&dir, "not a directory").unwrap();
    for options in [
        opts(),
        LogOpts {
            list: true,
            format: Some(DiscoveryFormat::Json),
            ..opts()
        },
    ] {
        let mut output = Vec::new();
        let error = run_in_dir(&dir, &options, false, &mut output)
            .expect_err("invalid log directory must not masquerade as absent history");
        assert!(error.to_string().contains(&dir.display().to_string()));
        assert!(error.downcast_ref::<std::io::Error>().is_some());
        assert!(
            output.is_empty(),
            "failed discovery must not emit an empty listing"
        );
    }
}

#[test]
fn prints_missing_log_message() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("absent");

    assert_eq!(
        capture(&dir, &opts(), false),
        "No dotfiles log found yet.\n"
    );
}
