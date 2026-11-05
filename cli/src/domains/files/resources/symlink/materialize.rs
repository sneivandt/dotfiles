use anyhow::{Context as _, Result};
use std::path::{Component, Path};

use super::platform::{is_link_like, rename_noreplace};
use super::state::{link_points_to_source, paths_equal, same_entry};
use crate::infra::fs::{TempGuard, ensure_parent_dir};

/// Stage a copy beside the target before replacing its managed link.
pub(super) fn copy_into_place(
    source: &Path,
    target: &Path,
    repositories: &[&Path],
    allow_missing: bool,
) -> Result<()> {
    ensure_parent_dir(target)?;

    if source.is_dir() {
        copy_dir_into_place(source, target, repositories, allow_missing)
    } else {
        copy_file_into_place(source, target, allow_missing)
    }
}

fn recovery_directory(target: &Path) -> std::path::PathBuf {
    use base64::Engine as _;
    use sha2::{Digest as _, Sha256};
    let name = target.file_name().unwrap_or(target.as_os_str());
    let digest = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(name.as_encoded_bytes()));
    target.with_file_name(format!(".dotfiles-materialize-link-{digest}"))
}

pub(super) fn check_recovery(target: &Path) -> Result<()> {
    let recovery = recovery_directory(target);
    if crate::infra::fs::symlink_metadata_optional(&recovery, "inspect removal recovery directory")?
        .is_some()
    {
        anyhow::bail!(
            "unfinished removal for {}. Check {}. If the target is missing and the directory contains original, move original back to the target. If both entries exist, inspect them and move original somewhere safe before retrying. Remove the empty recovery directory when finished",
            target.display(),
            recovery.display()
        );
    }
    Ok(())
}

/// Keep the original entry until publication succeeds. Both moves into the
/// target use no-replace semantics, including recovery after a failed publish.
fn publish(
    source: &Path,
    staged: &Path,
    target: &Path,
    allow_missing: bool,
    publish_copy: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> Result<()> {
    anyhow::ensure!(
        !same_entry(source, target),
        "refusing to replace the source entry while materializing: {}",
        target.display()
    );
    let publish_copy = || {
        publish_copy(staged, target)
            .with_context(|| format!("publish materialized copy at {}", target.display()))
    };
    check_recovery(target)?;
    if crate::infra::fs::symlink_metadata_optional(target, "stat target")?.is_none() {
        anyhow::ensure!(
            allow_missing,
            "managed link disappeared during materialization: {}",
            target.display()
        );
        return publish_copy();
    }
    require_managed_link(source, target, target)?;

    let recovery = recovery_directory(target);
    let builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt as _;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
    builder
        .create(&recovery)
        .with_context(|| format!("reserve recovery directory beside {}", target.display()))?;
    let mut backup = TempGuard::dir(recovery);
    let saved = backup.path().join("original");
    rename_noreplace(target, &saved)
        .with_context(|| format!("save original link at {}", target.display()))?;

    // Another writer could have replaced the link between inspection and the
    // move. Recheck the saved entry before letting the guard dispose of it.
    let result = require_managed_link(source, target, &saved).and_then(|()| publish_copy());
    if let Err(error) = result {
        if let Err(restore) = rename_noreplace(&saved, target) {
            backup.persist();
            return Err(error.context(format!(
                "could not restore {} ({restore}). Original entry saved at {}",
                target.display(),
                saved.display()
            )));
        }
        return Err(error);
    }
    Ok(())
}

fn require_managed_link(source: &Path, target: &Path, entry: &Path) -> Result<()> {
    anyhow::ensure!(
        is_link_like(entry, &entry.symlink_metadata()?),
        "refusing to overwrite non-symlink target: {}",
        target.display()
    );
    let existing =
        std::fs::read_link(entry).with_context(|| format!("read link for {}", target.display()))?;
    anyhow::ensure!(
        link_points_to_source(target, &existing, source),
        "refusing to replace link that no longer points to the managed source: {}",
        target.display()
    );
    Ok(())
}

fn copy_file_into_place(source: &Path, target: &Path, allow_missing: bool) -> Result<()> {
    let (mut guard, mut staged_file) = create_staged_file(target)?;
    let mut source_file = crate::infra::fs::open_regular_source(source)?;
    std::io::copy(&mut source_file, &mut staged_file)
        .with_context(|| format!("copy source file {}", source.display()))?;
    std::fs::set_permissions(guard.path(), source_file.metadata()?.permissions())?;
    drop(staged_file);

    publish(
        source,
        guard.path(),
        target,
        allow_missing,
        rename_noreplace,
    )?;
    guard.persist();
    Ok(())
}

fn create_staged_file(target: &Path) -> Result<(TempGuard, std::fs::File)> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    TempGuard::create_unique_file_with_mode(parent, ".dotfiles-materialize-file", "tmp", 0o600)
        .with_context(|| format!("create temporary file beside {}", target.display()))
}

pub(super) fn copy_dir_into_place(
    source: &Path,
    target: &Path,
    repositories: &[&Path],
    allow_missing: bool,
) -> Result<()> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let mut guard = TempGuard::create_unique_dir(parent, ".dotfiles-materialize-dir", "tmp")
        .with_context(|| format!("create temporary directory beside {}", target.display()))?;

    crate::infra::fs::copy_dir_recursive(source, guard.path(), false).with_context(|| {
        format!(
            "recursive copy {} to {}",
            source.display(),
            guard.path().display()
        )
    })?;
    validate_relocated_links(guard.path(), guard.path(), repositories)
        .with_context(|| format!("cannot safely materialize directory {}", source.display()))?;

    publish(
        source,
        guard.path(),
        target,
        allow_missing,
        rename_noreplace,
    )?;
    guard.persist();
    Ok(())
}

pub(super) fn validate_source(source: &Path, repositories: &[&Path]) -> Result<()> {
    let metadata = source
        .metadata()
        .with_context(|| format!("inspect materialization source {}", source.display()))?;
    if metadata.is_dir() {
        validate_relocated_links(source, source, repositories)?;
    } else {
        anyhow::ensure!(
            metadata.is_file(),
            "unsupported special file: {}",
            source.display()
        );
    }
    Ok(())
}

fn validate_relocated_links(root: &Path, directory: &Path, repositories: &[&Path]) -> Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = path.symlink_metadata()?;
        if is_link_like(&path, &metadata) {
            let target = std::fs::read_link(&path)?;
            let relative = path.strip_prefix(root)?;
            if target.is_absolute() {
                anyhow::ensure!(
                    !target.ancestors().any(|ancestor| {
                        let resolved = dunce::canonicalize(ancestor);
                        repositories.iter().any(|repository| {
                            paths_equal(ancestor, repository)
                                || resolved.as_ref().is_ok_and(|destination| {
                                    dunce::canonicalize(repository)
                                        .is_ok_and(|checkout| destination.starts_with(checkout))
                                })
                        })
                    }),
                    "absolute link {} -> {} depends on the checkout. Change it before retrying",
                    relative.display(),
                    target.display()
                );
            } else {
                anyhow::ensure!(
                    stays_in_tree(root, directory, &target)?,
                    "relative link {} -> {} escapes the copied directory",
                    relative.display(),
                    target.display()
                );
            }
        } else if metadata.is_dir() {
            validate_relocated_links(root, &path, repositories)?;
        } else {
            anyhow::ensure!(
                metadata.is_file(),
                "unsupported special file: {}",
                path.display()
            );
        }
    }
    Ok(())
}

fn stays_in_tree(root: &Path, directory: &Path, target: &Path) -> Result<bool> {
    let root = dunce::canonicalize(root)?;
    let mut resolved = dunce::canonicalize(directory)?;
    for part in target.components() {
        match part {
            Component::Normal(name) => {
                resolved.push(name);
                match dunce::canonicalize(&resolved) {
                    Ok(path) => resolved = path,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        if let Some(metadata) = crate::infra::fs::symlink_metadata_optional(
                            &resolved,
                            "stat link component",
                        )? && is_link_like(&resolved, &metadata)
                        {
                            return Err(error).with_context(|| {
                                format!("cannot resolve link component {}", resolved.display())
                            });
                        }
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Prefix(_) | Component::RootDir => return Ok(false),
        }
        if !resolved.starts_with(&root) {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt as _;

    const INTERRUPTION_ROOT: &str = "DOTFILES_TEST_MATERIALIZATION_INTERRUPTION_ROOT";

    #[test]
    fn a_disappeared_link_stays_missing_without_explicit_restoration() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        let source = root.join("source");
        let target = root.join("target");
        std::fs::write(&source, "source").unwrap();
        let error = copy_into_place(&source, &target, &[root], false).unwrap_err();
        assert!(format!("{error:#}").contains("managed link disappeared"));
        assert!(!target.exists());
        copy_into_place(&source, &target, &[root], true).unwrap();
        assert_eq!(std::fs::read_to_string(target).unwrap(), "source");
    }

    #[test]
    #[ignore = "subprocess fixture for interrupted publication"]
    fn interrupt_publication() {
        let root = std::path::PathBuf::from(std::env::var_os(INTERRUPTION_ROOT).unwrap());
        publish(
            &root.join("source"),
            &root.join("stage"),
            &root.join("target"),
            true,
            |_, _| std::process::exit(73),
        )
        .unwrap();
    }

    #[test]
    fn interrupted_publication_identifies_the_original_link_for_recovery() {
        use crate::infra::exec::{CommandSpec, Executor as _, ProcessExecutor};
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        let source = root.join("source");
        let target = root.join("target");
        let staged = root.join("stage");
        std::fs::write(&source, "source").unwrap();
        std::fs::write(&staged, "replacement").unwrap();
        crate::infra::fs::create_native_symlink(Path::new("source"), &target, false).unwrap();
        let result = ProcessExecutor::system().execute(
            CommandSpec::new(std::env::current_exe().unwrap()).args(&[
                "--ignored", "--exact",
                "domains::files::resources::symlink::materialize::tests::interrupt_publication",
            ]).env(INTERRUPTION_ROOT, root.as_os_str())
              .timeout(std::time::Duration::from_secs(20)).unchecked()
        ).unwrap();
        assert_eq!(result.code, Some(73), "{result:?}");
        let backup = recovery_directory(&target);
        assert!(!target.exists());
        assert!(
            check_recovery(&target)
                .unwrap_err()
                .to_string()
                .contains(&backup.display().to_string())
        );
        rename_noreplace(&backup.join("original"), &target).unwrap();
        std::fs::remove_dir(backup).unwrap();
        publish(&source, &staged, &target, true, rename_noreplace).unwrap();
        assert_eq!(std::fs::read_to_string(target).unwrap(), "replacement");
    }

    #[test]
    fn update_preserves_interrupted_removal_before_and_after_publication() {
        use crate::engine::{IntrinsicState, Resource, ResourceState};
        use crate::infra::exec::MockExecutor;
        use std::sync::Arc;

        for published in [false, true] {
            let fixture = tempfile::tempdir().unwrap();
            let source = fixture.path().join("source");
            let target = fixture.path().join("target");
            let recovery = recovery_directory(&target);
            std::fs::write(&source, "source").unwrap();
            std::fs::create_dir(&recovery).unwrap();
            let original = recovery.join("original");
            crate::infra::fs::create_native_symlink(&source, &original, false).unwrap();
            if published {
                std::fs::write(&target, "replacement").unwrap();
            }

            let resource =
                super::super::SymlinkResource::new(&source, &target, Arc::new(MockExecutor::new()));
            assert!(matches!(
                resource.current_state().unwrap(),
                ResourceState::Invalid { reason } if reason.contains("unfinished removal")
            ));
            assert!(resource.apply().is_err());
            assert_eq!(std::fs::read_link(original).unwrap(), source);
            if published {
                assert_eq!(std::fs::read_to_string(target).unwrap(), "replacement");
            } else {
                assert!(target.symlink_metadata().is_err());
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn named_pipes_are_rejected_before_or_during_copy() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        let source = root.join("source");
        std::fs::create_dir(&source).unwrap();
        let pipe = source.join("pipe");
        rustix::fs::mkfifoat(rustix::fs::CWD, &pipe, rustix::fs::Mode::RUSR).unwrap();
        for error in [
            validate_source(&source, &[root]).unwrap_err(),
            copy_dir_into_place(&source, &root.join("target"), &[root], true).unwrap_err(),
            crate::infra::fs::open_regular_source(&pipe).unwrap_err(),
        ] {
            assert!(format!("{error:#}").contains("unsupported special file"));
        }
    }

    #[test]
    fn failed_publication_restores_original_link_and_can_retry() {
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("source");
        let target = fixture.path().join("target");
        let staged = fixture.path().join("staged");
        std::fs::write(&source, "source content").unwrap();
        std::fs::write(&staged, "staged content").unwrap();
        let referent = Path::new("source");
        crate::infra::fs::create_native_symlink(referent, &target, false).unwrap();

        let error = publish(&source, &staged, &target, true, |_, destination| {
            assert!(!destination.exists());
            Err(std::io::ErrorKind::PermissionDenied.into())
        })
        .unwrap_err();

        assert!(error.to_string().contains("publish materialized copy"));
        assert_eq!(std::fs::read_link(&target).unwrap(), referent);
        assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 3);
        publish(&source, &staged, &target, true, rename_noreplace).unwrap();
        assert_eq!(std::fs::read_to_string(target).unwrap(), "staged content");
    }

    #[test]
    fn publication_preserves_an_intervening_file_or_directory() {
        for directory in [false, true] {
            let fixture = tempfile::tempdir().unwrap();
            let source = fixture.path().join("source");
            let target = fixture.path().join("target");
            let staged = fixture.path().join("staged");
            if directory {
                std::fs::create_dir(&source).unwrap();
                std::fs::create_dir(&staged).unwrap();
            } else {
                std::fs::write(&source, "source content").unwrap();
                std::fs::write(&staged, "staged content").unwrap();
            }
            crate::infra::fs::create_native_symlink(&source, &target, directory).unwrap();

            let error = publish(&source, &staged, &target, true, |staged, destination| {
                if directory {
                    std::fs::create_dir(destination)?;
                } else {
                    std::fs::write(destination, "user content")?;
                }
                rename_noreplace(staged, destination)
            })
            .unwrap_err();

            let message = error.to_string();
            let (_, saved) = message.split_once("Original entry saved at ").unwrap();
            assert_eq!(std::fs::read_link(saved).unwrap(), source);
            if directory {
                assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
            } else {
                assert_eq!(std::fs::read_to_string(&target).unwrap(), "user content");
            }
        }
    }

    #[test]
    fn materialization_refuses_user_files_and_directories_and_cleans_staging() {
        for directory in [false, true] {
            let fixture = tempfile::tempdir().unwrap();
            let source = fixture.path().join("source");
            let target = fixture.path().join("target");
            let content = |path: &Path| {
                if directory {
                    path.join("content")
                } else {
                    path.to_path_buf()
                }
            };
            if directory {
                std::fs::create_dir(&source).unwrap();
                std::fs::create_dir(&target).unwrap();
            }
            std::fs::write(content(&source), "managed content").unwrap();
            std::fs::write(content(&target), "user replacement").unwrap();

            let error = copy_into_place(&source, &target, &[&source], true).unwrap_err();

            assert!(
                error
                    .to_string()
                    .contains("refusing to overwrite non-symlink target"),
                "{error:#}"
            );
            assert_eq!(
                std::fs::read_to_string(content(&target)).unwrap(),
                "user replacement"
            );
            assert_eq!(
                std::fs::read_to_string(content(&source)).unwrap(),
                "managed content"
            );
            assert_eq!(
                std::fs::read_dir(fixture.path()).unwrap().count(),
                2,
                "failed staging must leave no temporary entries"
            );
        }
    }

    #[test]
    fn materialization_cannot_replace_its_own_source() {
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("source");
        std::fs::write(&source, "source must survive").unwrap();

        let error = copy_into_place(&source, &source, &[&source], true).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("refusing to replace the source entry"),
            "{error:#}"
        );
        assert_eq!(
            std::fs::read_to_string(&source).unwrap(),
            "source must survive"
        );
        assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 1);
    }

    #[test]
    fn failed_directory_staging_preserves_link_and_retry_materializes_content() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let source = root.join("source");
        let target = root.join("target");
        std::fs::create_dir(&source).unwrap();
        crate::infra::fs::create_native_symlink(&source, &target, true).unwrap();
        let original_link = std::fs::read_link(&target).unwrap();
        // The directory changed after the caller classified it, before staging.
        std::fs::remove_dir(&source).unwrap();
        std::fs::write(&source, "changed source").unwrap();

        let error = copy_dir_into_place(&source, &target, &[&root], true).unwrap_err();

        assert!(error.to_string().contains("recursive copy"), "{error:#}");
        assert_eq!(std::fs::read_link(&target).unwrap(), original_link);
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "changed source");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);

        std::fs::remove_file(&source).unwrap();
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("content"), "restored directory").unwrap();
        copy_dir_into_place(&source, &target, &[&root], true).unwrap();
        assert!(std::fs::read_link(&target).is_err());
        assert_eq!(
            std::fs::read_to_string(target.join("content")).unwrap(),
            "restored directory"
        );
        assert_eq!(
            std::fs::read_to_string(source.join("content")).unwrap(),
            "restored directory"
        );
        assert_eq!(std::fs::read_dir(root).unwrap().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn staging_file_is_private_before_copying_source_content() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let (guard, file) = create_staged_file(&fixture.path().join("target")).unwrap();
        assert_eq!(
            file.metadata().unwrap().permissions().mode() & 0o077,
            0,
            "source content must never be copied into a group/world-readable staging file"
        );
        assert_eq!(file.metadata().unwrap().len(), 0);
        drop(file);
        drop(guard);
        assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn private_staging_retains_the_final_source_permissions() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let source = fixture.path().join("source");
        let target = fixture.path().join("target");
        std::fs::write(&source, "managed content").unwrap();
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o640)).unwrap();

        copy_into_place(&source, &target, &[&source], true).unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "managed content");
        assert_eq!(
            target.metadata().unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 2);
    }
}
