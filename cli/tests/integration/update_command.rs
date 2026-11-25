//! Integration tests for the `update` command.
//!
//! These tests exercise the full task list produced by [`all_update_tasks`],
//! the task-selector filtering applied by the `--skip` and `--only` CLI
//! flags, and the structural properties of the update dependency graph.

use crate::common;

use dotfiles_cli::testing as test_api;

use test_api::config::ConfigStore;
use test_api::platform::{Os, Platform};
use test_api::tasks;

fn read_retained_log(log_dir: &std::path::Path, args: &[&str]) -> String {
    let home = log_dir.parent().unwrap();
    let result = common::cli_command(home, home, None, "log", "")
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}

#[cfg(unix)]
fn install_git_wrapper(
    fixture: &std::path::Path,
    script: &str,
) -> std::collections::BTreeMap<&'static str, std::ffi::OsString> {
    use std::os::unix::fs::PermissionsExt as _;

    let path = std::env::var_os("PATH").expect("fixture needs PATH");
    let real_git = which::which("git").expect("fixture needs git");
    let bin = fixture.join("bin");
    std::fs::create_dir(&bin).unwrap();
    let wrapper = bin.join("git");
    std::fs::write(&wrapper, script).unwrap();
    std::fs::set_permissions(wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let wrapped_path =
        std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&path))).unwrap();
    [
        ("PATH", wrapped_path),
        ("FIXTURE_GIT", real_git.into_os_string()),
    ]
    .into()
}

#[cfg(unix)]
fn initialize_local_remote(fixture: &std::path::Path, checkout: &std::path::Path, name: &str) {
    git(fixture, &["init", "--bare", "-b", "main", name]);
    git(checkout, &["init", "-b", "main"]);
    git(checkout, &["add", "."]);
    git(checkout, &["commit", "-m", "initial"]);
    git(
        checkout,
        &["remote", "add", "origin", &format!("../{name}")],
    );
    git(checkout, &["push", "-u", "origin", "main"]);
}

fn git(root: &std::path::Path, args: &[&str]) -> String {
    let mut command = std::process::Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("GIT_")
        {
            command.env_remove(key);
        }
    }
    let output = command
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@test.local",
        ])
        .args([
            "-c",
            "core.hooksPath=unused-hooks",
            "-c",
            "commit.gpgSign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", root.join("no-global-config"))
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn git_fixture_ignores_inherited_configuration_and_repository_paths() {
    const CHILD: &str = "DOTFILES_TEST_GIT_FIXTURE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "--bare", "-b", "main"]);
        assert_eq!(
            git(root.path(), &["rev-parse", "--is-bare-repository"]),
            "true"
        );
        assert_eq!(
            std::fs::canonicalize(git(root.path(), &["rev-parse", "--absolute-git-dir"])).unwrap(),
            std::fs::canonicalize(root.path()).unwrap()
        );
        return;
    }
    let unrelated = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "git_fixture_ignores_inherited_configuration_and_repository_paths",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "safe.bareRepository")
        .env("GIT_CONFIG_VALUE_0", "explicit")
        .env("GIT_DIR", unrelated.path().join("not-a-repository"))
        .env("GIT_WORK_TREE", unrelated.path())
        .env("GIT_INDEX_FILE", unrelated.path().join("index"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(std::fs::read_dir(unrelated.path()).unwrap().count(), 0);
}

#[path = "update_command/history.rs"]
mod history;
#[path = "update_command/repository_refresh.rs"]
mod repository_refresh;
#[path = "update_command/restart.rs"]
mod restart;
#[path = "update_command/selection.rs"]
mod selection;
#[path = "update_command/startup.rs"]
mod startup;
