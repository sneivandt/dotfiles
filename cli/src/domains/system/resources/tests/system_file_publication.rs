use super::*;

#[cfg(target_os = "linux")]
const PREPARE_FIXTURE: &str = r#"#!/bin/sh
set -eu
test "$1" = system-file-helper && test "$2" = --target
target=$3
test "$4" = --fragment
case "$6" in --fragment-hash=*) ;; *) exit 1 ;; esac
test "$7" = --merge && test "$8" = toml && test "$9" = --stage
stage=${10}
test "$(stat -c %a -- "$stage")" = 700
printf 'replacement content\n' > "$stage/input"
if [ -f "$target" ]; then cp -- "$target" "$stage/original"; fi
printf 'prepared\n'
"#;

#[cfg(target_os = "linux")]
const INSTALL_FIXTURE: &str = r#"#!/bin/sh
set -eu
test "$1" = -D && test "$2" = --owner=root
test "$3" = --group=root && test "$4" = --mode=0644
test "$5" = --
shift 5
test "$(stat -c %a -- "$(dirname -- "$2")")" = 700
if [ "$FAILURE" = copy ]; then
    printf 'partial' > "$2"
    exit 1
fi
if [ "$FAILURE" = conflict ]; then
    printf 'intervening edit\n' > "$TARGET"
fi
# Exercise the real copy without requiring root in the fixture.
exec "$REAL_INSTALL" --mode=0644 -- "$1" "$2"
"#;

#[cfg(target_os = "linux")]
const PUBLISH_FIXTURE: &str = r#"#!/bin/sh
set -eu
if [ "$FAILURE" = publish ]; then
    exit 1
fi
case "${0##*/}" in
    mv) exec "$REAL_MV" "$@" ;;
    ln) exec "$REAL_LN" "$@" ;;
esac
"#;

#[test]
#[cfg(target_os = "linux")]
#[expect(
    clippy::too_many_lines,
    reason = "shared setup for publication and preservation cases"
)]
fn atomic_install_preserves_destination_on_failures_and_intervening_edits() {
    use std::os::unix::fs::PermissionsExt as _;

    for existing in [false, true] {
        for failure in ["copy", "publish", "conflict", "chmod", "none"] {
            if !existing && failure == "chmod" {
                continue;
            }
            let root = tempfile::tempdir_in(".").unwrap();
            let root_path = root.path().canonicalize().unwrap();
            let tools = root.path().join("tools");
            fs::create_dir(&tools).unwrap();
            // Quotes and shell syntax must be passed as data, even through sudo sh.
            let parent = root_path.join("parent ' $(false)");
            let target = parent.join("target ' $(false).toml");
            let helper = root_path.join("helper ' $(false)");
            fs::write(&helper, PREPARE_FIXTURE).unwrap();
            fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
            if existing {
                fs::create_dir(&parent).unwrap();
                fs::write(&target, "original content\n").unwrap();
                rustix::fs::setxattr(
                    &target,
                    "user.dotfiles-fixture",
                    b"preserve me",
                    rustix::fs::XattrFlags::empty(),
                )
                .unwrap();
                fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
            }
            let original = fs::metadata(&target).ok();
            let install = tools.join("install");
            fs::write(&install, INSTALL_FIXTURE).unwrap();
            let cp = tools.join("cp");
            fs::write(
                &cp,
                r#"#!/bin/sh
set -eu
"$REAL_CP" "$@"
if [ "$1" = --attributes-only ] && [ "$FAILURE" = chmod ]; then chmod 0400 -- "$TARGET"; fi
"#,
            )
            .unwrap();
            fs::set_permissions(cp, fs::Permissions::from_mode(0o755)).unwrap();
            for tool in ["mv", "ln"] {
                let publish = tools.join(tool);
                fs::write(&publish, PUBLISH_FIXTURE).unwrap();
                fs::set_permissions(publish, fs::Permissions::from_mode(0o755)).unwrap();
            }
            fs::set_permissions(install, fs::Permissions::from_mode(0o755)).unwrap();
            let path = std::env::join_paths(
                std::iter::once(tools)
                    .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
            )
            .unwrap();
            let result = std::process::Command::new("sh")
                .args(["-c", ATOMIC_INSTALL, "dotfiles-system-file"])
                .arg(&helper)
                .arg(&target)
                .arg("public fragment")
                .arg(&root_path)
                .arg("toml")
                .arg("fragment snapshot")
                .env("PATH", path)
                .env("FAILURE", failure)
                .env("TARGET", &target)
                .env("REAL_CP", which::which("cp").unwrap())
                .env("REAL_INSTALL", which::which("install").unwrap())
                .env("REAL_MV", which::which("mv").unwrap())
                .env("REAL_LN", which::which("ln").unwrap())
                .output()
                .unwrap();

            assert_eq!(
                result.status.success(),
                failure == "none",
                "existing={existing}, failure={failure}: {result:?}"
            );
            if failure == "none" {
                assert_eq!(fs::read(&target).unwrap(), b"replacement content\n");
                if existing {
                    let mut value = [0_u8; 32];
                    let count =
                        rustix::fs::getxattr(&target, "user.dotfiles-fixture", &mut value[..])
                            .unwrap();
                    assert_eq!(&value[..count], b"preserve me");
                }
                assert_eq!(
                    fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                    if existing { 0o600 } else { 0o644 }
                );
            } else if failure == "chmod" {
                assert!(String::from_utf8_lossy(&result.stderr).contains("retry the update"));
                assert_eq!(fs::read(&target).unwrap(), b"original content\n");
                assert_eq!(
                    fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                    0o400
                );
            } else if failure == "conflict" {
                assert_eq!(fs::read(&target).unwrap(), b"intervening edit\n");
                if existing {
                    assert!(String::from_utf8_lossy(&result.stderr).contains("retry the update"));
                }
            } else if let Some(original) = original {
                assert_eq!(fs::read(&target).unwrap(), b"original content\n");
                let after = fs::metadata(&target).unwrap();
                assert_eq!(after.permissions().mode(), original.permissions().mode());
                assert_eq!(after.modified().unwrap(), original.modified().unwrap());
            } else {
                assert!(
                    !target.exists(),
                    "failed creation must leave the target absent"
                );
            }
            assert!(
                fs::read_dir(&parent).unwrap().all(|entry| {
                    !entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".dotfiles-system-file.")
                }),
                "replacement staging must be cleaned after {failure}"
            );
        }
    }
}

#[test]
#[cfg(target_os = "linux")]
fn system_file_publication_refuses_escaping_and_changed_parents() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let fixture_path = fixture.path().canonicalize().unwrap();
    let root = fixture_path.join("etc");
    let outside = fixture_path.join("outside");
    let actual = root.join("actual");
    for directory in [&root, &outside, &actual] {
        fs::create_dir(directory).unwrap();
    }
    let target = root.join("alias/subdir/config.toml");
    let alias = root.join("alias");
    std::os::unix::fs::symlink(&outside, &alias).unwrap();
    let resource = fixture_resource(
        entry(&root, &target, "fragment.toml", MergeStrategy::Toml),
        Arc::new(MockExecutor::new()),
    );
    assert!(
        resource
            .checked_target()
            .unwrap_err()
            .to_string()
            .contains("resolves outside")
    );
    fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(&actual, &alias).unwrap();
    let resolved = resource.checked_target().unwrap();
    assert_eq!(resolved, actual.join("subdir/config.toml"));

    // Simulate a parent replaced after discovery. The helper must reject it
    // before it creates either the missing subdirectory or staging files.
    fs::remove_dir(&actual).unwrap();
    std::os::unix::fs::symlink(&outside, &actual).unwrap();
    let result = std::process::Command::new("sh")
        .args(["-c", ATOMIC_INSTALL, "dotfiles-system-file", "unused-input"])
        .arg(&resolved)
        .arg("")
        .arg(&root)
        .arg("toml")
        .arg("fragment snapshot")
        .output()
        .unwrap();
    assert!(!result.status.success(), "{result:?}");
    assert!(String::from_utf8_lossy(&result.stderr).contains("became a symlink"));
    assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
}

#[test]
fn failed_install_preserves_target_without_user_staging_and_can_be_retried() {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt as _;

    let root = tempfile::tempdir_in(".").unwrap();
    fs::create_dir(root.path().join("system")).unwrap();
    let staging = root.path().join("staging");
    fs::create_dir(&staging).unwrap();
    fs::write(
        root.path().join("system/fragment.toml"),
        "[managed]\nenabled = true\n",
    )
    .unwrap();
    let target = root.path().join("target.toml");
    fs::write(&target, "unmanaged = 'keep'\n").unwrap();
    #[cfg(unix)]
    fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
    let mut executor = MockExecutor::new();
    let mut sequence = mockall::Sequence::new();
    for fail in [true, false] {
        let expected_target = crate::infra::fs::resolve_target_entry(&target).unwrap();
        let expected_fragment =
            dunce::canonicalize(root.path().join("system/fragment.toml")).unwrap();
        let expected_staging = staging.clone();
        executor
            .expect_execute()
            .once()
            .in_sequence(&mut sequence)
            .returning(move |spec| {
                assert_eq!(spec.program(), "sudo");
                assert_eq!(spec.arguments().len(), 12);
                assert_eq!(
                    &spec.arguments()[..6],
                    [
                        "-n",
                        "--",
                        "sh",
                        "-c",
                        ATOMIC_INSTALL,
                        "dotfiles-system-file"
                    ]
                );
                assert_eq!(spec.arguments()[7], expected_target.as_os_str());
                assert_eq!(spec.arguments()[8], expected_fragment.as_os_str());
                assert_eq!(spec.arguments()[10], "toml");
                assert_eq!(
                    spec.arguments()[11],
                    crate::infra::fs::content_hash("[managed]\nenabled = true\n").as_str()
                );
                assert_eq!(
                    Path::new(&spec.arguments()[6]),
                    std::env::current_exe().unwrap()
                );
                assert_eq!(fs::read_dir(&expected_staging).unwrap().count(), 0);
                assert_eq!(spec.working_dir(), None);
                assert!(spec.is_checked());
                if fail {
                    Err(ExecError::non_zero(
                        "sudo sh",
                        ExecResult::failure("", "fixture install denied", Some(1)),
                    ))
                } else {
                    fs::write(
                        &expected_target,
                        "unmanaged = 'keep'\n[managed]\nenabled = true\n",
                    )
                    .unwrap();
                    #[cfg(unix)]
                    fs::set_permissions(&expected_target, fs::Permissions::from_mode(0o644))
                        .unwrap();
                    Ok(ExecResult::success("applied\n"))
                }
            });
    }
    let resource = fixture_resource(
        entry(root.path(), &target, "fragment.toml", MergeStrategy::Toml),
        Arc::new(executor),
    );

    let error = resource.apply().unwrap_err();
    assert!(
        format!("{error:#}").contains("fixture install denied"),
        "{error:#}"
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "unmanaged = 'keep'\n");
    assert_eq!(fs::read_dir(&staging).unwrap().count(), 0);
    assert!(matches!(
        resource.current_state().unwrap(),
        ResourceState::Incorrect { .. }
    ));
    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    assert_eq!(resource.apply().unwrap(), ResourceChange::AlreadyCorrect);
    assert_eq!(fs::read_dir(&staging).unwrap().count(), 0);
    let content = fs::read_to_string(target).unwrap();
    assert!(content.contains("unmanaged = 'keep'"));
    assert!(content.contains("enabled = true"));
}
