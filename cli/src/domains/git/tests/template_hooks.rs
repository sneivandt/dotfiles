//! Exercise copied template entrypoints without invoking Git or creating commits.
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::{Command, Stdio};

fn install_hook_fixture(home: &Path) {
    let templates = Path::new(env!("CARGO_MANIFEST_DIR")).join("../symlinks/config/git/templates");
    let helpers = home.join(".config/git/templates/hooks.local");
    std::fs::create_dir_all(&helpers).unwrap();
    for hook in ["pre-push", "post-merge"] {
        std::fs::copy(templates.join("hooks").join(hook), home.join(hook)).unwrap();
    }
    for helper in ["pre-push", "post-merge", "clean-merged-branches"] {
        std::fs::copy(
            templates.join("hooks.local").join(helper),
            helpers.join(helper),
        )
        .unwrap();
    }
    let fake = home.join("git");
    std::fs::write(
        &fake,
        r#"#!/bin/sh
set -eu
case "$1 $2" in
  'config github.user')
    [ -n "${GITHUB_USER-}" ] || exit 1
    printf '%s\n' "$GITHUB_USER" ;;
  'config github.username') exit 1 ;;
  'config --get-all')
    case "$3" in
      dotfiles.protectedBranch)
        [ "$CASE" != config-error ] || exit 3
        case "$CASE" in split-*) echo release/local; exit 0 ;; esac
        exit 1 ;;
      remote.upstream.pushurl)
        [ "$CASE" != config-error ] || exit 3
        case "$CASE" in split-*) echo synthetic-remote; exit 0 ;; esac
        exit 1 ;;
      *) exit 99 ;;
    esac ;;
  'remote get-url')
    if [ "$3" = --push ]; then
      [ "$4:${5-}" = '--:upstream' ] || exit 99
      if [ -n "${PUSH_URL-}" ]; then
        printf '%s\n' "$PUSH_URL"
      else
        case "$CASE" in
          split-*) echo synthetic-remote ;;
          *) exit 99 ;;
        esac
      fi
    else
      [ "$3:$4" = '--:upstream' ] || exit 99
      if [ -n "${FETCH_URL-}" ]; then
        printf '%s\n' "$FETCH_URL"
      else
        case "$CASE" in
          split-*) echo synthetic-fetch ;;
          cached) echo synthetic-remote ;;
          *) exit 99 ;;
        esac
      fi
    fi ;;
  'symbolic-ref --quiet')
    if [ "$3" = refs/remotes/upstream/HEAD ]; then
      case "$CASE" in split-*|cached|push-instead-of-*) echo refs/remotes/upstream/trunk; exit 0 ;; esac
    fi
    [ "$3" = refs/remotes/origin/HEAD ] || exit 1
    echo refs/remotes/origin/trunk ;;
  'ls-remote --symref')
    expected_remote=synthetic-remote
    [ -z "${PUSH_URL-}" ] || expected_remote=$PUSH_URL
    [ "$3:$4:$5" = "--:$expected_remote:HEAD" ] || exit 99
    case "$CASE" in
      split-offline|cached|push-instead-of-offline) exit 128 ;;
      split-online|push-instead-of-online) printf 'ref: refs/heads/release\tHEAD\n'; exit 0 ;;
    esac
    [ "$CASE" != remote-error ] || exit 128
    printf 'ref: refs/heads/trunk\tHEAD\n' ;;
  'branch --show-current') echo current ;;
  'worktree list')
    echo 'branch refs/heads/held'
    [ "$CASE" != worktree-error ] || exit 128 ;;
  'for-each-ref '*)
    printf '%s\n' refs/heads/held refs/heads/trunk refs/heads/current refs/heads/old
    [ "$CASE" != refs-error ] || exit 128 ;;
  'branch -d') echo "$4" >> "$HOME/deletions" ;;
  *) exit 99 ;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn installed_hooks_require_complete_policy_and_branch_discovery() {
    let root = tempfile::tempdir_in(".").unwrap();
    let home = root.path().canonicalize().unwrap();
    install_hook_fixture(&home);
    let path =
        std::env::join_paths([home.as_path(), Path::new("/usr/bin"), Path::new("/bin")]).unwrap();
    let run = |hook: &str, case: &str, branch: Option<&str>| {
        let mut command = Command::new("sh");
        command
            .arg(home.join(hook))
            .current_dir(&home)
            .env_clear()
            .env("HOME", &home)
            .env("PATH", &path)
            .env("CASE", case);
        if let Some(branch) = branch {
            command
                .args(["upstream", "synthetic-remote"])
                .stdin(Stdio::piped());
            let mut child = command.spawn().unwrap();
            writeln!(
                child.stdin.take().unwrap(),
                "refs/heads/local 1 refs/heads/{branch} 0"
            )
            .unwrap();
            child.wait().unwrap().success()
        } else {
            command.arg("0").status().unwrap().success()
        }
    };
    for case in ["worktree-error", "refs-error", "healthy"] {
        assert_eq!(run("post-merge", case, None), case == "healthy", "{case}");
        if case == "healthy" {
            assert_eq!(
                std::fs::read_to_string(home.join("deletions")).unwrap(),
                "old\n"
            );
        } else {
            assert!(!home.join("deletions").exists(), "{case} deleted a branch");
        }
    }
    for (case, branch, allowed) in [
        ("config-error", "feature", false),
        ("remote-error", "feature", false),
        ("healthy", "trunk", false),
        ("healthy", "feature", true),
        ("split-offline", "feature", false),
        ("split-online", "release", false),
        ("split-online", "release/local", false),
        ("split-online", "feature", true),
        ("split-online", "trunk", true),
        ("cached", "trunk", false),
        ("cached", "feature", true),
    ] {
        assert_eq!(
            run("pre-push", case, Some(branch)),
            allowed,
            "{case}/{branch}"
        );
    }
}

#[test]
fn pre_push_only_reuses_cached_head_for_matching_repo_identity() {
    let root = tempfile::tempdir_in(".").unwrap();
    let home = root.path().canonicalize().unwrap();
    install_hook_fixture(&home);
    let path =
        std::env::join_paths([home.as_path(), Path::new("/usr/bin"), Path::new("/bin")]).unwrap();
    let run_push = |case: &str, fetch_url: &str, push_url: &str, branch: &str| {
        let mut command = Command::new("sh");
        command
            .arg(home.join("pre-push"))
            .current_dir(&home)
            .env_clear()
            .env("HOME", &home)
            .env("PATH", &path)
            .env("CASE", case)
            .env("FETCH_URL", fetch_url)
            .env("PUSH_URL", push_url)
            .args(["upstream", push_url])
            .stdin(Stdio::piped());
        let mut child = command.spawn().unwrap();
        writeln!(
            child.stdin.take().unwrap(),
            "refs/heads/local 1 refs/heads/{branch} 0"
        )
        .unwrap();
        child.wait().unwrap().success()
    };
    let offline = "push-instead-of-offline";
    let online = "push-instead-of-online";
    let same_fetch = "https://github.com/org/repo.git";
    let same_push = "git@github.com:org/repo.git";
    let div_fetch = "https://github.com/org/repo-a.git";
    let div_push = "git@github.com:org/repo-b.git";
    let ssh_fetch = "https://GitHub.COM:443/org/repo/";
    let ssh_push = "ssh://git@github.com:22/org/repo";
    let other_host = "https://gitlab.com/org/repo.git";
    let ssh_alt_port = "ssh://git@github.com:2222/org/repo.git";
    let git_ssh_alt_port = "git+ssh://git@github.com:2222/org/repo";
    let https_alt_port = "https://github.com:8443/org/repo.git";
    let plain_http_alt_port = "http://github.com:8080/org/repo.git";
    let custom_scp = "git@test.local:repos/work.git";
    let custom_ssh_abs = concat!("ssh://git", "@test.local/repos/work.git");
    let custom_https = "https://test.local/repos/work.git";
    for (case, fetch_url, push_url, branch, allowed) in [
        (offline, same_fetch, same_push, "trunk", false),
        (offline, same_fetch, same_push, "feature", true),
        (offline, ssh_fetch, ssh_push, "feature", true),
        (offline, same_fetch, ssh_fetch, "feature", true),
        (offline, same_push, ssh_push, "feature", true),
        (offline, ssh_alt_port, git_ssh_alt_port, "feature", true),
        (offline, ssh_alt_port, git_ssh_alt_port, "trunk", false),
        (offline, div_fetch, div_push, "feature", false),
        (offline, div_fetch, div_push, "trunk", false),
        (online, div_fetch, div_push, "release", false),
        (online, div_fetch, div_push, "trunk", true),
        (online, div_fetch, div_push, "feature", true),
        (offline, ssh_push, ssh_alt_port, "feature", false),
        (online, ssh_push, ssh_alt_port, "release", false),
        (online, ssh_push, ssh_alt_port, "trunk", true),
        (offline, same_fetch, https_alt_port, "feature", false),
        (online, same_fetch, https_alt_port, "release", false),
        (offline, same_push, ssh_alt_port, "feature", false),
        (offline, plain_http_alt_port, ssh_alt_port, "feature", false),
        (offline, same_fetch, other_host, "feature", false),
        (offline, custom_scp, custom_ssh_abs, "feature", false),
        (online, custom_scp, custom_ssh_abs, "release", false),
        (online, custom_scp, custom_ssh_abs, "trunk", true),
        (online, custom_scp, custom_ssh_abs, "feature", true),
        (offline, custom_https, custom_scp, "feature", false),
        (online, custom_https, custom_scp, "release", false),
        (online, custom_https, custom_scp, "trunk", true),
        (offline, custom_scp, custom_scp, "trunk", false),
        (offline, custom_scp, custom_scp, "feature", true),
        (
            offline,
            "synthetic-fetch",
            "synthetic-remote",
            "feature",
            false,
        ),
    ] {
        assert_eq!(
            run_push(case, fetch_url, push_url, branch),
            allowed,
            "{case}: {fetch_url} -> {push_url} ({branch})"
        );
    }
}

#[test]
fn pre_push_matches_personal_github_owners_case_insensitively() {
    let root = tempfile::tempdir_in(".").unwrap();
    let home = root.path().canonicalize().unwrap();
    install_hook_fixture(&home);
    let path =
        std::env::join_paths([home.as_path(), Path::new("/usr/bin"), Path::new("/bin")]).unwrap();
    let run_owner = |remote_url: &str, github_user: &str| {
        let mut command = Command::new("sh");
        command
            .arg(home.join("pre-push"))
            .current_dir(&home)
            .env_clear()
            .env("HOME", &home)
            .env("PATH", &path)
            .env("CASE", "remote-error")
            .env("GITHUB_USER", github_user)
            .args(["upstream", remote_url])
            .stdin(Stdio::piped());
        let mut child = command.spawn().unwrap();
        writeln!(
            child.stdin.take().unwrap(),
            "refs/heads/main 1 refs/heads/main 0"
        )
        .unwrap();
        child.wait().unwrap().success()
    };
    for (remote_url, github_user, allowed) in [
        ("git@github.com:MyUser/dotfiles.git", "myuser", true),
        ("https://myuser/repo.git", "myuser", false),
        (
            concat!("https://token", "@github.com/MyUser/dotfiles.git"),
            "myuser",
            true,
        ),
        (
            concat!("https://evil.example/a", "@github.com/myuser/repo.git"),
            "myuser",
            false,
        ),
        (
            "ssh://git@github.com:22/MyUser/dotfiles.git",
            "myuser",
            true,
        ),
        (
            "ssh://git@github.com:bad/myuser/dotfiles.git",
            "myuser",
            false,
        ),
        ("git@github-internal:MyUser/dotfiles.git", "myuser", true),
        (
            "ssh://git@github-internal:22/MyUser/dotfiles.git",
            "myuser",
            true,
        ),
        ("git@github-internal:other/dotfiles.git", "myuser", false),
    ] {
        assert_eq!(
            run_owner(remote_url, github_user),
            allowed,
            "owner check: {remote_url} as {github_user}"
        );
    }
}
