mod exit_pause_tests {
    use super::super::should_pause_on_exit;

    #[test]
    fn exit_pause_requires_an_interactive_elevated_window() {
        for (name, non_interactive, elevated, ci, terminal, expected) in [
            ("normal terminal", false, false, false, true, false),
            ("interactive child", false, true, false, true, true),
            ("non-interactive child", true, true, false, true, false),
            ("CI child", false, true, true, true, false),
            ("redirected input", false, true, false, false, false),
        ] {
            assert_eq!(
                should_pause_on_exit(non_interactive, elevated, ci, terminal),
                expected,
                "{name}"
            );
        }
    }
}

#[cfg(not(windows))]
mod tests {
    use super::super::*;

    #[test]
    fn is_elevated_returns_false_on_non_windows() {
        // On Linux/macOS, is_elevated() is a const fn that always returns false.
        assert!(!is_elevated());
    }

    #[test]
    fn wait_if_elevated_is_noop_on_non_windows() {
        // Should complete without blocking or panicking.
        wait_if_elevated(false);
        wait_if_elevated(true);
    }

    #[test]
    fn elevated_child_flag_tracks_env_and_explicit_marking() {
        assert!(!env_marks_elevated_child(None));
        assert!(!env_marks_elevated_child(Some(std::ffi::OsStr::new(""))));
        assert!(env_marks_elevated_child(Some(std::ffi::OsStr::new("1"))));
        mark_elevated_child();
        assert!(
            is_elevated_child(),
            "mark_elevated_child must enable is_elevated_child"
        );
    }

    #[test]
    #[cfg(unix)]
    fn sudo_available_reflects_the_executor_lookup() {
        use crate::infra::exec::MockExecutor;

        let mut present = MockExecutor::new();
        present
            .expect_which()
            .withf(|program| program == "sudo")
            .times(1)
            .returning(|_| true);
        assert!(sudo_available(&present));

        let mut absent = MockExecutor::new();
        absent
            .expect_which()
            .withf(|program| program == "sudo")
            .times(1)
            .returning(|_| false);
        assert!(
            !sudo_available(&absent),
            "sudo must be reported unavailable when it is not on PATH"
        );
    }
}

#[cfg(all(test, windows))]
mod delegation_tests {
    use super::super::*;
    use crate::infra::exec::{ExecError, ExecResult, MockExecutor};
    use crate::infra::logging::Output as _;

    #[test]
    fn elevation_uses_one_encoded_unchecked_invocation_and_interprets_every_exit_outcome() {
        for available in [false, true] {
            for (code, expected) in [
                (Some(0), ElevationOutcome::Completed),
                (
                    Some(ELEVATION_DECLINED_EXIT_CODE),
                    ElevationOutcome::Declined,
                ),
                (Some(17), ElevationOutcome::Failed(17)),
                (None, ElevationOutcome::Failed(-1)),
            ] {
                let (log, _tmp, _guard) = crate::infra::logging::isolated_logger_for("update");
                let args = [
                    "update",
                    "--only",
                    "developer-mode",
                    "--elevated-child",
                    "--parent-run-id=ancestor",
                ]
                .map(str::to_string);
                let expected_args = [
                    "update",
                    "--only",
                    "developer-mode",
                    "--elevated-child",
                    "--parent-run-id",
                    &log.run_log().unwrap().id(),
                ]
                .map(str::to_string);
                let exe = std::env::current_exe().unwrap();
                let encoded = powershell_encode_command(&build_elevated_child_script(
                    &exe.display().to_string(),
                    &expected_args,
                    &std::env::current_dir().unwrap().display().to_string(),
                ));
                let mut executor = MockExecutor::new();
                executor
                    .expect_which()
                    .withf(|program| program == "pwsh")
                    .times(1)
                    .return_const(available);
                executor
                    .expect_execute()
                    .withf(move |spec| {
                        spec.program() == if available { "pwsh" } else { "powershell" }
                            && spec.arguments()
                                == ["-NoProfile", "-EncodedCommand", encoded.as_str()]
                            && !spec.is_checked()
                            && spec.working_dir().is_none()
                            && spec.environment().is_empty()
                    })
                    .times(1)
                    .returning(move |_| {
                        Ok(ExecResult {
                            stdout: String::new(),
                            stderr: String::new(),
                            success: code == Some(0),
                            code,
                        })
                    });
                assert_eq!(
                    run_elevated_child(&executor, &log, &args).unwrap(),
                    expected,
                    "pwsh={available}, code={code:?}"
                );
            }
        }
    }

    #[test]
    fn failure_to_spawn_elevation_helper_is_not_reported_as_user_declining() {
        let (log, _tmp, _guard) = crate::infra::logging::isolated_logger();
        let mut executor = MockExecutor::new();
        executor
            .expect_which()
            .withf(|program| program == "pwsh")
            .times(1)
            .return_const(true);
        executor
            .expect_execute()
            .withf(|spec| spec.program() == "pwsh" && !spec.is_checked())
            .times(1)
            .returning(|_| {
                Err(ExecError::spawn(
                    "pwsh",
                    std::io::Error::other("fixture spawn failure"),
                ))
            });

        let error = run_elevated_child(&executor, &log, &["install".into()]).unwrap_err();

        assert_eq!(error.to_string(), "failed to start elevated process");
        assert!(matches!(
            error.downcast_ref::<ExecError>(),
            Some(ExecError::Spawn { .. })
        ));
        assert!(format!("{error:#}").contains("fixture spawn failure"));
    }
}

mod escaping_tests {
    use super::super::*;

    // --- build_elevated_child_script ---

    #[test]
    fn elevated_child_script_waits_and_forwards_the_exit_code() {
        let script = build_elevated_child_script(
            r"C:\dotfiles\dotfiles.exe",
            &["install".to_string(), "--only".to_string()],
            r"C:\dotfiles",
        );

        assert!(
            script.contains("-Verb RunAs -Wait -PassThru"),
            "the parent must block on the elevated child: {script}"
        );
        assert!(
            script.contains("exit $exitCode"),
            "the child exit code must reach the parent: {script}"
        );
        assert!(
            script.contains(r"-FilePath 'C:\dotfiles\dotfiles.exe'"),
            "the executable path must be single-quoted: {script}"
        );
        assert!(
            script.contains("-ArgumentList 'install --only'"),
            "arguments must be passed as one Windows command line: {script}"
        );
        assert!(
            script.contains(r"-WorkingDirectory 'C:\dotfiles'"),
            "UAC must not substitute its own default working directory: {script}"
        );
    }

    #[test]
    fn elevated_child_working_directory_preserves_windows_path_forms() {
        for cwd in [
            r"C:\",
            r"C:\Path With Spaces",
            r"\\server\share\checkout",
            r"\\?\C:\long path\checkout",
            r"c:\mixedCase",
        ] {
            let script = build_elevated_child_script("dotfiles.exe", &[], cwd);
            assert!(
                script.contains(&format!("-WorkingDirectory '{cwd}'")),
                "working directory must retain its native path spelling: {script}"
            );
        }
    }

    #[test]
    fn elevated_child_script_maps_a_declined_prompt_to_a_distinct_code() {
        let script = build_elevated_child_script("dotfiles.exe", &[], r"C:\dotfiles");

        assert!(
            script.contains(&format!(
                "$exception.NativeErrorCode -eq {ELEVATION_DECLINED_EXIT_CODE}"
            )),
            "only an explicit Windows cancellation must count as declined: {script}"
        );
    }

    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "make optional runtime coverage gaps visible to the test harness"
    )]
    fn elevated_child_script_distinguishes_decline_from_start_failure() {
        use crate::infra::exec::{CommandSpec, Executor as _, ProcessExecutor};

        let executor = ProcessExecutor::system();
        let powershell = if executor.which("pwsh") {
            "pwsh"
        } else if cfg!(windows) {
            "powershell"
        } else {
            eprintln!("SKIP: PowerShell is unavailable for elevation-script fixtures");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let cases = [
            ("success", "[pscustomobject]@{ ExitCode = 0 }", 0),
            ("child failure", "[pscustomobject]@{ ExitCode = 7 }", 7),
            (
                "declined",
                "throw [System.ComponentModel.Win32Exception]::new(1223)",
                ELEVATION_DECLINED_EXIT_CODE,
            ),
            (
                "wrapped decline",
                "throw [System.InvalidOperationException]::new('wrapped', \
                 [System.ComponentModel.Win32Exception]::new(1223))",
                ELEVATION_DECLINED_EXIT_CODE,
            ),
            (
                "missing executable",
                "throw [System.ComponentModel.Win32Exception]::new(2)",
                1,
            ),
            (
                "other launch failure",
                "throw [System.InvalidOperationException]::new('launch failed')",
                1,
            ),
            ("missing process handle", "$null", 1),
            // PowerShell returns null when a Process property getter fails.
            ("exit code not available", "Get-Process -Id $PID", 1),
        ];
        // Use an ordinary name containing the quote characters recognized by
        // PowerShell. The fake validates all three strings without elevating.
        let cwd = r"C:\O’Brien\‘2026‚‛";
        let inner = build_elevated_child_script(&format!(r"{cwd}\fixture.exe"), &[cwd.into()], cwd);
        let mut driver = String::new();
        for (index, (name, behavior, _)) in cases.iter().enumerate() {
            let script_path = dir.path().join(format!("case-{index}.ps1"));
            let script = format!(
                "function Start-Process {{ param($FilePath, $WorkingDirectory, $ArgumentList, \
                  $Verb, [switch]$Wait, [switch]$PassThru)\n\
                  if ($WorkingDirectory -cne \"{cwd}\" -or $FilePath -cne \"{cwd}\\fixture.exe\" \
                      -or $ArgumentList -cne '\"' + \"{cwd}\" + '\"') {{ exit 99 }}\n{behavior} }}\n{inner}"
            );
            std::fs::write(&script_path, script).unwrap();
            let quoted = powershell_single_quote(&script_path.to_string_lossy());
            std::fmt::Write::write_fmt(
                &mut driver,
                format_args!(
                    "$global:LASTEXITCODE = 0\n\
                     $err = ''\n\
                     try {{ $null = & {quoted}; $code = $LASTEXITCODE }}\n\
                     catch {{ $code = 1; $err = $_.ToString() }}\n\
                     Write-Output ('{name}|' + $code + '|' + [bool]$err)\n"
                ),
            )
            .unwrap();
        }
        let result = executor
            .execute(
                CommandSpec::new(powershell)
                    .args(&[
                        "-NoProfile",
                        "-NonInteractive",
                        "-EncodedCommand",
                        &powershell_encode_command(&driver),
                    ])
                    .redact_arguments()
                    .timeout(std::time::Duration::from_mins(1))
                    .unchecked(),
            )
            .unwrap_or_else(|error| panic!("elevation script driver failed: {error}"));
        assert_eq!(result.code, Some(0), "{result:?}");
        let lines: Vec<&str> = result.stdout.lines().collect();
        let expected: Vec<String> = cases
            .iter()
            .map(|(name, _, code)| {
                format!(
                    "{name}|{code}|{}",
                    if *code == 1 { "True" } else { "False" }
                )
            })
            .collect();
        assert_eq!(
            lines, expected,
            "stdout:\n{}\nstderr:\n{}",
            result.stdout, result.stderr
        );
    }

    #[test]
    fn elevated_child_script_omits_the_argument_list_when_there_are_no_arguments() {
        let script = build_elevated_child_script("dotfiles.exe", &[], r"C:\dotfiles");

        assert!(
            !script.contains("-ArgumentList"),
            "an empty argument list must not be emitted: {script}"
        );
    }

    #[test]
    fn elevated_child_script_escapes_quotes_in_arguments() {
        let script =
            build_elevated_child_script("dotfiles.exe", &["O'Brien".to_string()], r"C:\O'Brien");

        assert!(
            script.contains("'O''Brien'"),
            "single quotes must be doubled inside the argument list: {script}"
        );
        assert!(
            script.contains(r"-WorkingDirectory 'C:\O''Brien'"),
            "the child's explicit working directory must be escaped: {script}"
        );
    }

    // --- powershell_single_quote ---

    #[test]
    fn single_quote_wraps_plain_string() {
        assert_eq!(powershell_single_quote("hello"), "'hello'");
    }

    #[test]
    fn single_quote_preserves_spaces() {
        assert_eq!(
            powershell_single_quote("path with spaces"),
            "'path with spaces'"
        );
    }

    #[test]
    fn single_quote_escapes_single_quote() {
        assert_eq!(powershell_single_quote("O'Brien"), "'O''Brien'");
    }

    #[test]
    fn single_quote_escapes_multiple_single_quotes() {
        assert_eq!(powershell_single_quote("a''b"), "'a''''b'");
    }

    #[test]
    fn single_quote_preserves_newline() {
        // Literal newlines are valid inside PS single-quoted strings; the
        // encoding layer (Base64) makes them safe at the command level.
        assert_eq!(powershell_single_quote("foo\nbar"), "'foo\nbar'");
    }

    #[test]
    fn single_quote_preserves_carriage_return_lf() {
        assert_eq!(powershell_single_quote("foo\r\nbar"), "'foo\r\nbar'");
    }

    // --- powershell_argument_line ---

    #[test]
    fn argument_line_empty_produces_empty_string() {
        let args: Vec<String> = vec![];
        assert_eq!(powershell_argument_line(&args), "''");
    }

    #[test]
    fn argument_line_single_arg() {
        let args = vec!["install".to_string()];
        assert_eq!(powershell_argument_line(&args), "'install'");
    }

    #[test]
    fn argument_line_preserves_argument_with_spaces() {
        let args = vec![
            "--root".to_string(),
            "C:\\My Documents\\dotfiles".to_string(),
        ];
        assert_eq!(
            powershell_argument_line(&args),
            "'--root \"C:\\My Documents\\dotfiles\"'"
        );
    }

    #[test]
    fn argument_line_handles_commas_inside_args() {
        let args = vec!["a,b".to_string(), "c,d".to_string()];
        assert_eq!(powershell_argument_line(&args), "'a,b c,d'");
    }

    #[test]
    fn argument_line_escapes_single_quotes_for_powershell() {
        let args = vec!["O'Brien".to_string(), "it's fine".to_string()];
        assert_eq!(powershell_argument_line(&args), "'O''Brien \"it''s fine\"'");
    }

    #[test]
    fn argument_line_combines_spaces_and_single_quotes() {
        let args = vec![
            "C:\\Temp\\Path With Space".to_string(),
            "O'Brien".to_string(),
        ];
        assert_eq!(
            powershell_argument_line(&args),
            "'\"C:\\Temp\\Path With Space\" O''Brien'"
        );
    }

    #[test]
    fn argument_line_handles_newline_inside_arg() {
        let args = vec!["foo\nbar".to_string()];
        assert_eq!(powershell_argument_line(&args), "'\"foo\nbar\"'");
    }

    #[test]
    fn argument_line_handles_carriage_return_inside_arg() {
        let args = vec!["foo\r\nbar".to_string()];
        assert_eq!(powershell_argument_line(&args), "'\"foo\r\nbar\"'");
    }

    // --- powershell_encode_command ---

    #[test]
    fn encode_command_empty_string_produces_empty() {
        assert_eq!(powershell_encode_command(""), "");
    }

    #[test]
    fn encode_command_produces_utf16le_base64() {
        // "abc" in UTF-16LE is 61 00 62 00 63 00.
        // base64("61 00 62 00 63 00") == "YQBiAGMA"  (verified externally).
        assert_eq!(powershell_encode_command("abc"), "YQBiAGMA");
    }

    #[test]
    fn encode_command_output_contains_only_base64_chars() {
        let script =
            "Start-Process -FilePath 'C:\\foo\\bar' -ArgumentList @('install') -Verb RunAs";
        let encoded = powershell_encode_command(script);
        assert!(
            encoded
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
        );
    }
}
