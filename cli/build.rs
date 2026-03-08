//! Build script for dotfiles-cli.
//!
//! Resolves the build version from the `DOTFILES_VERSION` environment variable
//! or from `git describe`, and exposes it as the `DOTFILES_VERSION` compile-time
//! environment variable.

use std::process::Command;

fn main() {
    // Prefer DOTFILES_VERSION env var if set (e.g., by CI release workflow),
    // otherwise fall back to git describe for local development builds.
    if let Ok(version) = std::env::var("DOTFILES_VERSION") {
        println!("cargo:rustc-env=DOTFILES_VERSION={version}");
    } else if let Ok(output) = Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty"])
        .output()
        && output.status.success()
    {
        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        println!("cargo:rustc-env=DOTFILES_VERSION={version}");
    }

    // Re-run if git HEAD changes or env var changes.
    //
    // Use `git rev-parse` to locate the actual git directory rather than
    // hard-coding `../.git/`, which is wrong for Git worktrees (where `.git`
    // is a file pointing to a separate directory) and other non-standard
    // repository layouts.
    register_git_rerun_triggers();
    println!("cargo:rerun-if-env-changed=DOTFILES_VERSION");
}

/// Register `cargo:rerun-if-changed` directives for the Git HEAD and refs.
///
/// - **HEAD** lives in the per-worktree git directory (`git rev-parse
///   --absolute-git-dir`), which may be a `worktrees/<name>/` subdirectory of
///   the main `.git/` directory rather than `.git/` itself.
/// - **refs/** and **packed-refs** live in the common git directory (`git
///   rev-parse --git-common-dir`), which is the same as the git dir for
///   non-worktree clones but points to the shared root for worktrees.
///
/// Falls back to the conventional `../.git/` paths if `git` is unavailable or
/// the working directory is not inside a git repository.
fn register_git_rerun_triggers() {
    // Always returns an absolute path; available since git 2.13 (2017).
    let git_dir = git_output(&["rev-parse", "--absolute-git-dir"]);

    // May return a relative path on older git — resolve it against cwd.
    // Available since git 2.5 (2015); equals git_dir for non-worktree clones.
    let git_common_dir = git_output(&["rev-parse", "--git-common-dir"]).and_then(|s| {
        let p = std::path::Path::new(&s);
        if p.is_absolute() {
            Some(s)
        } else {
            std::env::current_dir()
                .ok()
                .map(|cwd| cwd.join(p).to_string_lossy().into_owned())
        }
    });

    // HEAD is per-worktree.
    if let Some(ref dir) = git_dir {
        println!("cargo:rerun-if-changed={dir}/HEAD");
    } else {
        println!("cargo:rerun-if-changed=../.git/HEAD");
    }

    // refs/ and packed-refs are shared across all worktrees.
    let refs_base = git_common_dir.as_deref().or(git_dir.as_deref());
    if let Some(dir) = refs_base {
        println!("cargo:rerun-if-changed={dir}/refs/");
        println!("cargo:rerun-if-changed={dir}/packed-refs");
    } else {
        println!("cargo:rerun-if-changed=../.git/refs/");
    }
}

/// Run a git subcommand and return trimmed stdout on success, `None` otherwise.
fn git_output(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if s.is_empty() { None } else { Some(s) }
    } else {
        None
    }
}
