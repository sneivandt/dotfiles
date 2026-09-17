#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "lifecycle fixture assertions"
)]
//! Real-task lifecycle contracts, including native Windows filesystem behavior.

mod common;
#[path = "common/lifecycle.rs"]
mod lifecycle;

use std::path::Path;

use dotfiles_cli::testing::platform::{Os, Platform};
use dotfiles_cli::testing::tasks::files::symlinks::{InstallSymlinks, UninstallSymlinks};
use dotfiles_cli::testing::tasks::git::git_config::ConfigureGit;
use dotfiles_cli::testing::tasks::git::hooks::{InstallGitHooks, UninstallGitHooks};
use lifecycle::{Fixture, assert_lifecycle, write};

const LINK_SUBJECTS: &[&str] = &[
    "~/.first → symlinks/first",
    "~/.folder → symlinks/folder",
    "~/Documents/profile.ps1 → symlinks/profile file",
];

fn symlink_fixture() -> Fixture {
    let fixture = Fixture::new(Platform::detect());
    fixture.config(
        "symlinks.toml",
        "[base]\nsymlinks = [\"first\", \"folder\", \
         { source = \"profile file\", target = \"Documents/profile.ps1\" }]\n",
    );
    write(
        &fixture.repo.join("symlinks").join("first"),
        "first source\n",
    );
    write(
        &fixture.repo.join("symlinks").join("folder").join("nested"),
        "nested source\n",
    );
    write(
        &fixture.repo.join("symlinks").join("profile file"),
        "profile source\n",
    );
    write(&fixture.home.join(".first"), "obsolete file to replace\n");
    require_native_file_symlinks(&fixture);
    fixture
}

fn require_native_file_symlinks(fixture: &Fixture) {
    let probe = fixture.home.join("capability-probe");
    let source = fixture.repo.join("symlinks").join("first");
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_file(&source, &probe);
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(&source, &probe);
    result.expect(
        "native file symlinks are required by this lifecycle contract; \
         on Windows enable Developer Mode or use a symlink-capable test worker",
    );
    std::fs::remove_file(probe).expect("remove fixture capability probe");
}

fn assert_link(target: &Path, source: &Path) {
    assert!(
        std::fs::read_link(target).is_ok(),
        "managed target must be a link: {}",
        target.display()
    );
    assert_eq!(
        dunce::canonicalize(target).unwrap(),
        dunce::canonicalize(source).unwrap(),
        "managed link must point at its configured source"
    );
}

fn assert_symlinks_installed(fixture: &Fixture) {
    for (target, source) in [
        (
            fixture.home.join(".first"),
            fixture.repo.join("symlinks").join("first"),
        ),
        (
            fixture.home.join(".folder"),
            fixture.repo.join("symlinks").join("folder"),
        ),
        (
            fixture.home.join("Documents").join("profile.ps1"),
            fixture.repo.join("symlinks").join("profile file"),
        ),
    ] {
        assert_link(&target, &source);
    }
    assert_eq!(
        std::fs::read(fixture.home.join(".folder").join("nested")).unwrap(),
        b"nested source\n",
        "directory link content"
    );
}

#[test]
fn symlinks_preview_apply_repeat_and_conservative_uninstall() {
    for parallel in [false, true] {
        let fixture = symlink_fixture();
        assert_lifecycle(
            &fixture,
            parallel,
            |store| Box::new(InstallSymlinks::new(store.symlinks)),
            "link",
            LINK_SUBJECTS,
            3,
            || assert_symlinks_installed(&fixture),
        );

        let replaced = fixture.home.join(".first");
        std::fs::remove_file(&replaced).unwrap();
        write(&replaced, "user replacement must survive\n");
        assert_lifecycle(
            &fixture,
            parallel,
            |store| Box::new(UninstallSymlinks::new(store.symlinks)),
            "materialize",
            &[
                "~/.folder → symlinks/folder",
                "~/Documents/profile.ps1 → symlinks/profile file",
            ],
            3,
            || {
                assert_eq!(
                    std::fs::read(&replaced).unwrap(),
                    b"user replacement must survive\n",
                    "uninstall must retain user-replaced targets"
                );
                let folder = fixture.home.join(".folder");
                assert!(
                    std::fs::read_link(&folder).is_err(),
                    "uninstall materializes managed directories"
                );
                assert_eq!(
                    std::fs::read(folder.join("nested")).unwrap(),
                    b"nested source\n",
                    "materialization retains nested source bytes"
                );
                let profile = fixture.home.join("Documents").join("profile.ps1");
                assert!(
                    std::fs::read_link(&profile).is_err(),
                    "uninstall materializes managed files"
                );
                assert_eq!(
                    std::fs::read(profile).unwrap(),
                    b"profile source\n",
                    "materialization retains file source bytes"
                );
            },
        );
        assert_eq!(
            fixture.command_count(),
            0,
            "native symlinks need no subprocess"
        );
    }
}

#[test]
fn symlink_nth_failure_preserves_success_and_retry_converges() {
    let fixture = symlink_fixture();
    let blocker = fixture.home.join(".folder");
    write(&blocker.join("user-file"), "do not recursively delete\n");
    let run = fixture.run(false, false, |store| {
        Box::new(InstallSymlinks::new(store.symlinks))
    });
    run.assert_partial_failure(1, 1);
    run.assert_actions("link", &["~/.first → symlinks/first"], false);
    assert_link(
        &fixture.home.join(".first"),
        &fixture.repo.join("symlinks").join("first"),
    );
    assert_eq!(
        std::fs::read(blocker.join("user-file")).unwrap(),
        b"do not recursively delete\n",
        "Nth apply failure must leave the blocked target untouched"
    );
    assert!(
        !fixture.home.join("Documents").exists(),
        "third resource must not be attempted after strict failure"
    );

    std::fs::remove_file(blocker.join("user-file")).unwrap();
    std::fs::remove_dir(&blocker).unwrap();
    assert_lifecycle(
        &fixture,
        false,
        |store| Box::new(InstallSymlinks::new(store.symlinks)),
        "link",
        &[
            "~/.folder → symlinks/folder",
            "~/Documents/profile.ps1 → symlinks/profile file",
        ],
        3,
        || assert_symlinks_installed(&fixture),
    );
}

fn hook_fixture() -> Fixture {
    let fixture = Fixture::new(Platform::detect());
    for hook in ["pre-commit", "commit-msg", "post-checkout"] {
        write(
            &fixture.repo.join("hooks").join(hook),
            &format!("#!/bin/sh\n# fixture {hook}\nexit 0\n"),
        );
    }
    write(
        &fixture.repo.join("hooks").join("helper.sh"),
        "not an installable entry point\n",
    );
    write(
        &fixture.repo.join(".git").join("hooks").join("pre-commit"),
        "obsolete hook\n",
    );
    fixture
}

fn assert_hooks_installed(fixture: &Fixture) {
    for hook in ["pre-commit", "commit-msg", "post-checkout"] {
        let target = fixture.repo.join(".git").join("hooks").join(hook);
        assert_eq!(
            std::fs::read(&target).unwrap(),
            std::fs::read(fixture.repo.join("hooks").join(hook)).unwrap(),
            "installed hook bytes: {hook}"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_ne!(
                std::fs::metadata(target).unwrap().permissions().mode() & 0o111,
                0,
                "installed hook is executable: {hook}"
            );
        }
    }
    assert!(
        !fixture
            .repo
            .join(".git")
            .join("hooks")
            .join("helper.sh")
            .exists(),
        "helper files must not create logical resources"
    );
}

#[test]
fn git_hooks_preview_apply_repeat_and_conservative_uninstall() {
    for parallel in [false, true] {
        let fixture = hook_fixture();
        assert_lifecycle(
            &fixture,
            parallel,
            |_| Box::new(InstallGitHooks::new()),
            "install",
            &["pre-commit", "commit-msg", "post-checkout"],
            3,
            || assert_hooks_installed(&fixture),
        );

        let hooks = fixture.repo.join(".git").join("hooks");
        write(&hooks.join("pre-commit"), "user hook must survive\n");
        write(&hooks.join("pre-push"), "unmanaged hook must survive\n");
        assert_lifecycle(
            &fixture,
            parallel,
            |_| Box::new(UninstallGitHooks::new()),
            "remove",
            &["commit-msg", "post-checkout"],
            3,
            || {
                for removed in ["commit-msg", "post-checkout"] {
                    assert!(!hooks.join(removed).exists(), "remove managed {removed}");
                }
                assert_eq!(
                    std::fs::read(hooks.join("pre-commit")).unwrap(),
                    b"user hook must survive\n",
                    "preserve user replacement"
                );
                assert_eq!(
                    std::fs::read(hooks.join("pre-push")).unwrap(),
                    b"unmanaged hook must survive\n",
                    "preserve unrelated hooks"
                );
            },
        );
        assert_eq!(fixture.command_count(), 0, "Git hooks use no subprocesses");
    }
}

const fn platform(os: Os) -> Platform {
    Platform {
        os,
        is_arch: false,
        is_wsl: false,
    }
}

const GIT_SETTINGS: &str = "[base]\nsettings = [\
    { key = 'fixture.first', value = 'one' }, \
    { key = 'fixture.second', value = 'two' }, \
    { key = 'fixture.third', value = 'three' }]\n";

fn git_fixture(os: Os) -> Fixture {
    let fixture = Fixture::new(platform(os));
    fixture.config("git-config.toml", GIT_SETTINGS);
    write(
        &fixture.home.join(".gitconfig"),
        "[fixture]\nfirst = obsolete\nunmanaged = retain\n[core]\nautocrlf = true\n",
    );
    fixture
}

fn assert_git_settings(fixture: &Fixture, os: Os) {
    let config = git2::Config::open(&fixture.home.join(".gitconfig")).unwrap();
    for (key, expected) in [
        ("fixture.first", "one"),
        ("fixture.second", "two"),
        ("fixture.third", "three"),
        ("fixture.unmanaged", "retain"),
    ] {
        assert_eq!(
            config.get_string(key).unwrap(),
            expected,
            "Git setting {key}"
        );
    }
    match os {
        Os::Windows => assert_eq!(
            config.get_string("core.autocrlf").unwrap_err().code(),
            git2::ErrorCode::NotFound,
            "Windows removes unmanaged autocrlf"
        ),
        Os::Linux => assert_eq!(
            config.get_string("core.autocrlf").unwrap(),
            "true",
            "Unix does not manage implicit autocrlf"
        ),
    }
}

#[test]
fn git_settings_preview_apply_and_fresh_repeat_include_windows_cleanup() {
    for os in [Os::Linux, Os::Windows] {
        for parallel in [false, true] {
            let fixture = git_fixture(os);
            let mut subjects = vec![
                "fixture.first = one",
                "fixture.second = two",
                "fixture.third = three",
            ];
            if os == Os::Windows {
                subjects.push("core.autocrlf is unset");
            }
            assert_lifecycle(
                &fixture,
                parallel,
                |store| {
                    Box::new(ConfigureGit::with_config_path(
                        store.git_settings,
                        fixture.home.join(".gitconfig"),
                    ))
                },
                "configure",
                &subjects,
                u32::try_from(subjects.len()).unwrap(),
                || assert_git_settings(&fixture, os),
            );
            assert_eq!(
                fixture.command_count(),
                0,
                "Git lifecycle mutations use isolated native config, never commands"
            );
        }
    }
}

#[test]
fn git_settings_nth_failure_preserves_success_and_retry_converges() {
    for os in [Os::Linux, Os::Windows] {
        let fixture = git_fixture(os);
        fixture.config(
            "git-config.toml",
            &GIT_SETTINGS.replace("fixture.second", "invalid"),
        );
        let run = fixture.run(false, true, |store| {
            Box::new(ConfigureGit::with_config_path(
                store.git_settings,
                fixture.home.join(".gitconfig"),
            ))
        });
        run.assert_partial_failure(1, if os == Os::Windows { 2 } else { 1 });
        run.assert_actions("configure", &["fixture.first = one"], false);
        let config = git2::Config::open(&fixture.home.join(".gitconfig")).unwrap();
        assert_eq!(
            config.get_string("fixture.first").unwrap(),
            "one",
            "earlier successful mutation survives strict failure"
        );
        assert_eq!(
            config.get_string("fixture.third").unwrap_err().code(),
            git2::ErrorCode::NotFound,
            "later setting must not run"
        );
        assert_eq!(
            config.get_string("core.autocrlf").unwrap(),
            "true",
            "later Windows cleanup must not run"
        );
        drop(config);

        fixture.config("git-config.toml", GIT_SETTINGS);
        let mut subjects = vec!["fixture.second = two", "fixture.third = three"];
        if os == Os::Windows {
            subjects.push("core.autocrlf is unset");
        }
        assert_lifecycle(
            &fixture,
            true,
            |store| {
                Box::new(ConfigureGit::with_config_path(
                    store.git_settings,
                    fixture.home.join(".gitconfig"),
                ))
            },
            "configure",
            &subjects,
            if os == Os::Windows { 4 } else { 3 },
            || assert_git_settings(&fixture, os),
        );
    }
}
