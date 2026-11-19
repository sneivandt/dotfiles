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
  'config github.user'|'config github.username') exit 1 ;;
  'config --get-all')
    [ "$3" = dotfiles.protectedBranch ] || exit 99
    [ "$CASE" != config-error ] || exit 3
    case "$CASE" in split-*) echo release/local; exit 0 ;; esac
    exit 1 ;;
  'remote get-url')
    [ "$3:$4" = '--:upstream' ] || exit 99
    case "$CASE" in split-*) echo synthetic-fetch ;; cached) echo synthetic-remote ;; *) exit 99 ;; esac ;;
  'symbolic-ref --quiet')
    if [ "$3" = refs/remotes/upstream/HEAD ]; then
      case "$CASE" in split-*|cached) echo refs/remotes/upstream/trunk; exit 0 ;; esac
    fi
    [ "$3" = refs/remotes/origin/HEAD ] || exit 1
    echo refs/remotes/origin/trunk ;;
  'ls-remote --symref')
    [ "$3:$4:$5" = '--:synthetic-remote:HEAD' ] || exit 99
    case "$CASE" in
      split-offline|cached) exit 128 ;;
      split-online) printf 'ref: refs/heads/release\tHEAD\n'; exit 0 ;;
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
