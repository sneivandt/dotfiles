//! Resolve the editor launcher and interpret its inventory and installation commands.
use std::collections::HashSet;

use anyhow::Result;

#[cfg(not(target_os = "windows"))]
use crate::infra::exec::CommandSpec;
use crate::infra::exec::{self, Executor};

#[cfg(target_os = "windows")]
const CODE_COMMANDS: [&str; 2] = ["code-insiders.cmd", "code.cmd"];
#[cfg(not(target_os = "windows"))]
const CODE_COMMANDS: [&str; 2] = ["code-insiders", "code"];

/// Query the full set of installed VS Code extension IDs in a single command.
///
/// Returns a `HashSet` of **lower-cased** extension IDs.
///
/// # Errors
///
/// Returns an error if the VS Code command fails to execute, cannot be found,
/// or exits with a non-zero status code.
pub(super) fn get_installed_extensions(
    code_cmd: &str,
    executor: &dyn Executor,
) -> Result<HashSet<String>> {
    let result = run_code_cmd(code_cmd, &["--list-extensions"], executor)?;
    if !result.success {
        anyhow::bail!(
            "code --list-extensions failed (exit {:?}): {}",
            result.code,
            result.stderr.trim()
        );
    }
    Ok(result
        .stdout
        .lines()
        .map(|line| line.trim().to_lowercase())
        .filter(|id| !id.is_empty())
        .collect())
}

/// Request one installation. A successful command is not proof of installation.
pub(super) fn request_install(code_cmd: &str, id: &str, executor: &dyn Executor) -> Result<()> {
    let result = run_code_cmd(code_cmd, &["--install-extension", id, "--force"], executor)?;
    if !result.success {
        let exit = result
            .code
            .map_or_else(|| "unknown".to_string(), |code| code.to_string());
        anyhow::bail!(
            "{code_cmd} failed to install {id} (exit {exit}); stdout: {}; stderr: {}",
            nonempty_output(&result.stdout),
            nonempty_output(&result.stderr)
        )
    }
    Ok(())
}

fn nonempty_output(output: &str) -> &str {
    let output = output.trim();
    if output.is_empty() { "<empty>" } else { output }
}

/// Find the VS Code CLI command, preferring Code Insiders.
///
/// Windows installations include both an extensionless POSIX shell script and
/// a `.cmd` launcher in the same directory. The Windows command must include
/// the extension and be resolved to an absolute path because the launcher uses
/// `%~dp0` to locate the adjacent VS Code executable.
///
/// # Errors
///
/// Returns an error if an available launcher cannot be resolved. That failure
/// must not silently select a different VS Code installation.
pub(super) fn find_code_command(
    executor: &dyn Executor,
    skip_insiders: impl Fn(&std::path::Path) -> Result<bool>,
) -> Result<Option<String>> {
    for command in CODE_COMMANDS {
        if executor.which(command) {
            let path = executor.which_path(command)?;
            if command == CODE_COMMANDS[0] && skip_insiders(&path)? {
                continue;
            }
            return Ok(Some(path.to_string_lossy().into_owned()));
        }
    }
    Ok(None)
}

/// A tracked Insiders launcher can survive removal of the editor package.
#[cfg(target_os = "linux")]
pub(crate) fn managed_insiders_unavailable(
    path: &std::path::Path,
    root: &std::path::Path,
    home: &std::path::Path,
) -> Result<bool> {
    insiders_unavailable(path, root, home, || {
        std::fs::metadata("/usr/bin/code-insiders")
    })
}

#[cfg(not(target_os = "linux"))]
#[expect(clippy::unnecessary_wraps, reason = "matches the fallible Linux probe")]
pub(crate) const fn managed_insiders_unavailable_non_linux(
    _path: &std::path::Path,
    _root: &std::path::Path,
    _home: &std::path::Path,
) -> Result<bool> {
    Ok(false)
}

#[cfg(not(target_os = "linux"))]
pub(crate) use managed_insiders_unavailable_non_linux as managed_insiders_unavailable;

#[cfg(target_os = "linux")]
fn insiders_unavailable(
    path: &std::path::Path,
    root: &std::path::Path,
    home: &std::path::Path,
    probe_backing: impl FnOnce() -> std::io::Result<std::fs::Metadata>,
) -> Result<bool> {
    use anyhow::Context as _;
    use std::os::unix::fs::PermissionsExt as _;
    if !is_managed_insiders(path, root, home) {
        return Ok(false);
    }
    match probe_backing() {
        Ok(meta) => {
            anyhow::ensure!(
                meta.is_file() && meta.permissions().mode() & 0o111 != 0,
                "managed Insiders launcher requires an executable /usr/bin/code-insiders"
            );
            Ok(false)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => {
            Err(error).context("checking /usr/bin/code-insiders for managed Insiders launcher")
        }
    }
}

#[cfg(target_os = "linux")]
fn is_managed_insiders(
    path: &std::path::Path,
    root: &std::path::Path,
    home: &std::path::Path,
) -> bool {
    let source = root.join("symlinks/local/bin/code-insiders");
    if let Ok(expected) = source.canonicalize()
        && path.canonicalize().is_ok_and(|found| found == expected)
    {
        return true;
    }
    // Removal can materialize the managed shim while keeping its exact bytes.
    path == home.join(".local/bin/code-insiders")
        && std::fs::read(&source)
            .is_ok_and(|expected| std::fs::read(path).is_ok_and(|contents| contents == expected))
}

/// Run a VS Code CLI command. On Windows, `.cmd` wrappers need `cmd.exe /C`.
///
/// # Errors
///
/// Returns an error if the command execution fails or if the command cannot be found.
fn run_code_cmd(cmd: &str, args: &[&str], executor: &dyn Executor) -> Result<exec::ExecResult> {
    #[cfg(target_os = "windows")]
    {
        exec::windows::CmdCommand::new(cmd)
            .args(args)
            .run_unchecked(executor)
    }

    #[cfg(not(target_os = "windows"))]
    {
        Ok(executor.execute(CommandSpec::new(cmd).args(args).unchecked())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::exec::{ExecError, ExecResult, MockExecutor};

    #[cfg(target_os = "linux")]
    #[test]
    fn an_orphaned_managed_insiders_shim_yields_to_stable_code() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        let home = root.join("home");
        let shim = root.join("symlinks/local/bin/code-insiders");
        let installed = home.join(".local/bin/code-insiders");
        let stable = home.join(".local/bin/code");
        let backing = root.join("missing-package-binary");
        std::fs::create_dir_all(shim.parent().unwrap()).unwrap();
        std::fs::create_dir_all(installed.parent().unwrap()).unwrap();
        std::fs::write(&shim, "managed shim").unwrap();
        for materialized in [false, true] {
            if materialized {
                std::fs::remove_file(&installed).unwrap();
                std::fs::copy(&shim, &installed).unwrap();
            } else {
                std::os::unix::fs::symlink(&shim, &installed).unwrap();
            }
            let mut executor = MockExecutor::new();
            executor.expect_which().return_const(true);
            for (command, path) in [("code-insiders", &installed), ("code", &stable)] {
                let path = path.clone();
                executor
                    .expect_which_path()
                    .with(mockall::predicate::eq(command))
                    .once()
                    .return_once(move |_| Ok(path));
            }
            let chosen = find_code_command(&executor, |path| {
                insiders_unavailable(path, root, &home, || std::fs::metadata(&backing))
            })
            .unwrap();
            assert_eq!(chosen.as_deref(), stable.to_str());
        }
        std::fs::write(&installed, "custom launcher").unwrap();
        assert!(!managed_insiders_unavailable(&installed, root, &home).unwrap());
        assert!(
            !insiders_unavailable(&installed, root, &home, || panic!(
                "custom launchers do not require the packaged binary"
            ))
            .unwrap()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn installed_insiders_with_a_nonexecutable_backing_does_not_redirect_extensions() {
        use std::os::unix::fs::PermissionsExt as _;

        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        let home = root.join("home");
        let shim = root.join("symlinks/local/bin/code-insiders");
        let installed = home.join(".local/bin/code-insiders");
        let backing = root.join("insiders-binary");
        std::fs::create_dir_all(shim.parent().unwrap()).unwrap();
        std::fs::create_dir_all(installed.parent().unwrap()).unwrap();
        std::fs::write(
            &shim,
            include_bytes!("../../../../../symlinks/local/bin/code-insiders"),
        )
        .unwrap();
        std::fs::copy(&shim, &installed).unwrap();
        std::fs::write(&backing, "installed").unwrap();
        for (mode, denied) in [(0o700, true), (0o600, false), (0o700, false)] {
            std::fs::set_permissions(&backing, std::fs::Permissions::from_mode(mode)).unwrap();
            let mut executor = MockExecutor::new();
            executor
                .expect_which()
                .with(mockall::predicate::eq("code-insiders"))
                .once()
                .return_const(true);
            let resolved = installed.clone();
            executor
                .expect_which_path()
                .with(mockall::predicate::eq("code-insiders"))
                .once()
                .return_once(move |_| Ok(resolved));
            let chosen = find_code_command(&executor, |path| {
                insiders_unavailable(path, root, &home, || {
                    if denied {
                        Err(std::io::ErrorKind::PermissionDenied.into())
                    } else {
                        std::fs::metadata(&backing)
                    }
                })
            });
            if denied {
                let error = chosen.unwrap_err();
                assert_eq!(
                    error.downcast_ref::<std::io::Error>().unwrap().kind(),
                    std::io::ErrorKind::PermissionDenied
                );
            } else if mode == 0o700 {
                assert_eq!(chosen.unwrap().as_deref(), installed.to_str());
            } else {
                assert!(
                    chosen
                        .unwrap_err()
                        .to_string()
                        .contains("/usr/bin/code-insiders")
                );
            }
        }
    }

    fn expect_code_command(
        mock: &mut MockExecutor,
        args: &'static [&'static str],
        result: std::result::Result<ExecResult, ExecError>,
    ) {
        #[cfg(target_os = "windows")]
        {
            let command_line = format!("\"code {}\"", args.join(" "));
            mock.expect_execute()
                .once()
                .withf(move |spec| {
                    spec.windows_command_line() == Some(command_line.as_str())
                        && !spec.is_checked()
                        && spec.working_dir().is_none()
                })
                .return_once(|_| result);
        }

        #[cfg(not(target_os = "windows"))]
        mock.expect_execute()
            .once()
            .withf(move |spec| {
                spec.program() == "code"
                    && spec.arguments() == args
                    && !spec.is_checked()
                    && spec.working_dir().is_none()
            })
            .return_once(|_| result);
    }

    fn expect_list_extensions(mock: &mut MockExecutor, result: ExecResult) {
        expect_code_command(mock, &["--list-extensions"], Ok(result));
    }

    fn expect_install_extension(mock: &mut MockExecutor, result: ExecResult) {
        expect_code_command(
            mock,
            &["--install-extension", "ms-python.python", "--force"],
            Ok(result),
        );
    }

    #[test]
    fn find_code_command_prefers_platform_specific_insiders_launcher() {
        let mut mock = MockExecutor::new();
        let expected = std::path::PathBuf::from(CODE_COMMANDS[0]);
        mock.expect_which()
            .once()
            .with(mockall::predicate::eq(CODE_COMMANDS[0]))
            .return_const(true);
        mock.expect_which_path()
            .once()
            .with(mockall::predicate::eq(CODE_COMMANDS[0]))
            .return_once({
                let expected = expected.clone();
                |_| Ok(expected)
            });

        assert_eq!(
            find_code_command(&mock, |_| Ok(false)).unwrap().as_deref(),
            expected.to_str(),
            "the platform-specific Code Insiders launcher should be preferred"
        );
    }

    #[test]
    fn find_code_command_falls_back_to_platform_specific_stable_launcher() {
        let mut sequence = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        let expected = std::path::PathBuf::from(CODE_COMMANDS[1]);
        mock.expect_which()
            .once()
            .with(mockall::predicate::eq(CODE_COMMANDS[0]))
            .in_sequence(&mut sequence)
            .return_const(false);
        mock.expect_which()
            .once()
            .with(mockall::predicate::eq(CODE_COMMANDS[1]))
            .in_sequence(&mut sequence)
            .return_const(true);
        mock.expect_which_path()
            .once()
            .with(mockall::predicate::eq(CODE_COMMANDS[1]))
            .in_sequence(&mut sequence)
            .return_once({
                let expected = expected.clone();
                |_| Ok(expected)
            });

        assert_eq!(
            find_code_command(&mock, |_| Ok(false)).unwrap().as_deref(),
            expected.to_str(),
            "the platform-specific stable launcher should be used as fallback"
        );
    }

    #[test]
    fn launcher_resolution_errors_do_not_fall_back_to_another_installation() {
        let mut mock = MockExecutor::new();
        mock.expect_which()
            .once()
            .with(mockall::predicate::eq(CODE_COMMANDS[0]))
            .return_const(true);
        mock.expect_which_path()
            .once()
            .with(mockall::predicate::eq(CODE_COMMANDS[0]))
            .returning(|_| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "launcher inaccessible",
                )
                .into())
            });
        let error = find_code_command(&mock, |_| Ok(false)).unwrap_err();
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::PermissionDenied,
        );
    }

    #[test]
    fn apply_failure_preserves_cli_diagnostics() {
        let mut mock = MockExecutor::new();
        expect_install_extension(
            &mut mock,
            ExecResult::failure(
                "Installing extensions...",
                "Signature verification failed",
                Some(1),
            ),
        );
        let error = request_install("code", "ms-python.python", &mock).unwrap_err();
        assert_eq!(
            error.to_string(),
            "code failed to install ms-python.python (exit 1); stdout: Installing extensions...; stderr: Signature verification failed"
        );
    }

    #[test]
    fn apply_success_and_empty_failure_output_are_distinguished() {
        for (result, expected) in [
            (ExecResult::success("installed"), None),
            (
                ExecResult::failure(" \n", "\n ", None),
                Some(
                    "code failed to install ms-python.python (exit unknown); stdout: <empty>; stderr: <empty>",
                ),
            ),
        ] {
            let mut mock = MockExecutor::new();
            expect_install_extension(&mut mock, result);
            let error = request_install("code", "ms-python.python", &mock)
                .err()
                .map(|e| e.to_string());
            assert_eq!(error.as_deref(), expected);
        }
    }

    #[test]
    fn spawn_failure_is_not_reported_as_an_extension_skip_or_empty_inventory() {
        for install in [false, true] {
            let mut mock = MockExecutor::new();
            let args: &'static [&'static str] = if install {
                &["--install-extension", "ms-python.python", "--force"]
            } else {
                &["--list-extensions"]
            };
            expect_code_command(
                &mut mock,
                args,
                Err(ExecError::spawn(
                    "code",
                    std::io::Error::other("launcher unavailable"),
                )),
            );
            let error = if install {
                request_install("code", "ms-python.python", &mock)
                    .unwrap_err()
                    .to_string()
            } else {
                get_installed_extensions("code", &mock)
                    .unwrap_err()
                    .to_string()
            };
            assert!(error.contains("launcher unavailable"), "{error}");
        }
    }

    // ------------------------------------------------------------------
    // get_installed_extensions
    // ------------------------------------------------------------------

    #[test]
    fn get_installed_extensions_parses_and_lowercases() {
        let mut mock = MockExecutor::new();
        expect_list_extensions(
            &mut mock,
            ExecResult::success(
                " GitHub.Copilot \r\n\nms-python.python\nRust-lang.Rust-analyzer\nGITHUB.COPILOT\n \t\n",
            ),
        );
        let installed = get_installed_extensions("code", &mock).unwrap();
        assert_eq!(
            installed,
            HashSet::from([
                "github.copilot".to_string(),
                "ms-python.python".to_string(),
                "rust-lang.rust-analyzer".to_string(),
            ])
        );
    }

    #[test]
    fn get_installed_extensions_returns_error_when_command_fails() {
        let mut mock = MockExecutor::new();
        expect_list_extensions(
            &mut mock,
            ExecResult::failure("partial.extension", " inventory failed \n", Some(2)),
        );
        let error = get_installed_extensions("code", &mock)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("exit Some(2)") && error.contains("inventory failed"),
            "{error}"
        );
    }
}
