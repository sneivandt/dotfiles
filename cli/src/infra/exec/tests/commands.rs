use super::*;

fn echo_result(msg: &str) -> Result<ExecResult> {
    let executor = ProcessExecutor::system();
    #[cfg(windows)]
    {
        Ok(executor.execute(CommandSpec::new("cmd").args(&["/C", "echo", msg]))?)
    }
    #[cfg(not(windows))]
    {
        Ok(executor.execute(CommandSpec::new("echo").arg(msg))?)
    }
}

#[test]
fn run_echo() {
    let result = echo_result("hello").unwrap();
    assert!(result.success, "echo command should succeed");
    assert_eq!(result.stdout.trim(), "hello");
}

#[test]
fn checked_and_unchecked_failures_retain_exit_code_and_both_streams() {
    let executor = ProcessExecutor::system();
    for checked in [true, false] {
        #[cfg(windows)]
        let spec = CommandSpec::new("cmd").args(&[
            "/D",
            "/C",
            "(echo output)&(echo diagnostic 1>&2)&exit /b 7",
        ]);
        #[cfg(not(windows))]
        let spec = CommandSpec::new("sh").args(&[
            "-c",
            "printf 'output\\n'; printf 'diagnostic\\n' >&2; exit 7",
        ]);
        let label = spec.label();
        let result = executor.execute(if checked { spec } else { spec.unchecked() });
        let result = if checked {
            let ExecError::NonZero { command, result } = result.unwrap_err() else {
                panic!("checked failures must report NonZero");
            };
            assert_eq!(command, label);
            result
        } else {
            result.unwrap()
        };
        assert!(!result.success, "checked={checked}");
        assert_eq!(result.code, Some(7), "checked={checked}");
        assert_eq!(result.stdout.trim(), "output", "checked={checked}");
        assert_eq!(result.stderr.trim(), "diagnostic", "checked={checked}");
    }
}

#[test]
fn which_finds_known_program() {
    let executor = ProcessExecutor::system();
    #[cfg(windows)]
    assert!(executor.which("cmd"), "cmd should be found on Windows");
    #[cfg(not(windows))]
    assert!(executor.which("echo"), "echo should be found on Unix");
}

#[test]
fn which_missing_program() {
    let executor = ProcessExecutor::system();
    assert!(
        !executor.which("this-program-does-not-exist-12345"),
        "non-existent program should not be found"
    );
}

#[test]
fn which_path_finds_known_program() {
    let executor = ProcessExecutor::system();
    #[cfg(windows)]
    let result = executor.which_path("cmd");
    #[cfg(not(windows))]
    let result = executor.which_path("echo");
    assert!(result.is_ok(), "which_path should find a known program");
    let path = result.unwrap();
    assert!(
        path.is_absolute(),
        "which_path should return an absolute path"
    );
}

#[test]
#[cfg(unix)]
fn path_lookup_observes_an_executable_created_after_an_initial_miss() {
    use std::os::unix::fs::PermissionsExt as _;

    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("dotfiles-newly-installed-program");
    let program = executable.to_str().unwrap();
    let executor = ProcessExecutor::system();
    assert!(
        executor.which_path(program).is_err(),
        "program should initially be absent"
    );

    std::fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
    let mut permissions = executable.metadata().unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).unwrap();

    assert_eq!(
        executor.which_path(program).unwrap(),
        executable,
        "a fresh lookup must observe a newly installed executable"
    );
}

#[test]
fn which_path_fails_for_missing_program() {
    let executor = ProcessExecutor::system();
    let result = executor.which_path("this-program-does-not-exist-12345");
    assert!(
        result.is_err(),
        "which_path should fail for a missing program"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("not found on PATH"),
        "error message should mention 'not found on PATH'"
    );
}

#[test]
fn command_spec_applies_working_directory_and_child_only_environment_overrides() {
    let executor = ProcessExecutor::system();
    let dir = tempfile::tempdir().unwrap();
    let working_dir = crate::infra::fs::canonicalize(dir.path()).unwrap();
    let original_env = std::env::var_os("DOTFILES_EXEC_TEST_VALUE");
    #[cfg(windows)]
    let spec = CommandSpec::new("cmd").args(&["/D", "/C", "cd & echo %DOTFILES_EXEC_TEST_VALUE%"]);
    #[cfg(not(windows))]
    let spec = CommandSpec::new("sh").args(&[
        "-c",
        "printf '%s\\n' \"$PWD\" \"$DOTFILES_EXEC_TEST_VALUE\"",
    ]);
    let result = executor
        .execute(
            spec.current_dir(&working_dir)
                .env("DOTFILES_EXEC_TEST_VALUE", "overridden")
                .envs(&[("DOTFILES_EXEC_TEST_VALUE", "child-only value")]),
        )
        .unwrap();
    assert!(result.success);
    assert_eq!(result.code, Some(0));
    assert!(result.stderr.is_empty());
    let lines: Vec<_> = result.stdout.lines().map(str::trim).collect();
    assert_eq!(
        lines,
        [working_dir.to_str().unwrap(), "child-only value"],
        "both cwd and the last environment override must reach the child"
    );
    assert_eq!(std::env::var_os("DOTFILES_EXEC_TEST_VALUE"), original_env);
}

#[test]
fn every_command_kind_preserves_working_directory_and_environment() {
    for spec in [
        CommandSpec::new("fixture"),
        CommandSpec::windows_cmd("echo fixture"),
    ] {
        let command = spec
            .current_dir("working directory")
            .env("DOTFILES_EXEC_TEST_VALUE", "first")
            .envs(&[("DOTFILES_EXEC_TEST_VALUE", "last")])
            .into_command();

        assert_eq!(
            command.get_current_dir(),
            Some(Path::new("working directory"))
        );
        assert_eq!(
            command.get_envs().collect::<Vec<_>>(),
            [(
                OsStr::new("DOTFILES_EXEC_TEST_VALUE"),
                Some(OsStr::new("last"))
            )]
        );
    }
}

#[cfg(windows)]
#[test]
fn windows_cmd_applies_working_directory_and_child_only_environment_overrides() {
    let root = tempfile::tempdir_in(".").unwrap();
    let working_dir = crate::infra::fs::canonicalize(root.path()).unwrap();
    let key = "DOTFILES_EXEC_CMD_TEST_VALUE";
    let original_env = std::env::var_os(key);
    let result = ProcessExecutor::system()
        .execute(
            CommandSpec::windows_cmd(r#""cd & echo %DOTFILES_EXEC_CMD_TEST_VALUE%""#)
                .current_dir(&working_dir)
                .env(key, "child-only value"),
        )
        .unwrap();

    assert!(result.success, "{result:?}");
    assert_eq!(
        result.stdout.lines().map(str::trim).collect::<Vec<_>>(),
        [working_dir.to_str().unwrap(), "child-only value"]
    );
    assert_eq!(std::env::var_os(key), original_env);
}
