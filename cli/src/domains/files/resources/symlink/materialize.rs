use anyhow::{Context as _, Result};
use std::path::{Component, Path};

use super::platform::{is_link_like, rename_noreplace};
use super::state::{link_points_to_source, paths_equal, same_entry};
use crate::infra::fs::{TempGuard, ensure_parent_dir};

/// Stage a copy beside the target before replacing its managed link.
pub(super) fn copy_into_place(source: &Path, target: &Path, repository: &Path) -> Result<()> {
    ensure_parent_dir(target)?;

    if source.is_dir() {
        copy_dir_into_place(source, target, repository)
    } else {
        copy_file_into_place(source, target)
    }
}

/// Keep the original entry until publication succeeds. Both moves into the
/// target use no-replace semantics, including recovery after a failed publish.
fn publish(
    source: &Path,
    staged: &Path,
    target: &Path,
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
    if crate::infra::fs::symlink_metadata_optional(target, "stat target")?.is_none() {
        return publish_copy();
    }
    require_managed_link(source, target, target)?;

    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let mut backup = TempGuard::create_unique_dir(parent, ".dotfiles-materialize-link", "tmp")
        .with_context(|| format!("reserve recovery directory beside {}", target.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(backup.path(), std::fs::Permissions::from_mode(0o700))?;
    }
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

fn copy_file_into_place(source: &Path, target: &Path) -> Result<()> {
    let (mut guard, mut staged_file) = create_staged_file(target)?;
    let mut source_file = std::fs::File::open(source)
        .with_context(|| format!("open source file {}", source.display()))?;
    std::io::copy(&mut source_file, &mut staged_file)
        .with_context(|| format!("copy source file {}", source.display()))?;
    std::fs::set_permissions(guard.path(), source_file.metadata()?.permissions())?;
    drop(staged_file);

    publish(source, guard.path(), target, rename_noreplace)?;
    guard.persist();
    Ok(())
}

fn create_staged_file(target: &Path) -> Result<(TempGuard, std::fs::File)> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    TempGuard::create_unique_file_with_mode(parent, ".dotfiles-materialize-file", "tmp", 0o600)
        .with_context(|| format!("create temporary file beside {}", target.display()))
}

pub(super) fn copy_dir_into_place(source: &Path, target: &Path, repository: &Path) -> Result<()> {
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
    validate_relocated_links(guard.path(), guard.path(), repository)
        .with_context(|| format!("cannot safely materialize directory {}", source.display()))?;

    publish(source, guard.path(), target, rename_noreplace)?;
    guard.persist();
    Ok(())
}

fn validate_relocated_links(root: &Path, directory: &Path, repository: &Path) -> Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = path.symlink_metadata()?;
        if is_link_like(&path, &metadata) {
            let target = std::fs::read_link(&path)?;
            let relative = path.strip_prefix(root)?;
            if target.is_absolute() {
                anyhow::ensure!(
                    !target
                        .ancestors()
                        .any(|ancestor| paths_equal(ancestor, repository)),
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
            validate_relocated_links(root, &path, repository)?;
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

        let error = publish(&source, &staged, &target, |_, destination| {
            assert!(!destination.exists());
            Err(std::io::ErrorKind::PermissionDenied.into())
        })
        .unwrap_err();

        assert!(error.to_string().contains("publish materialized copy"));
        assert_eq!(std::fs::read_link(&target).unwrap(), referent);
        assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 3);
        publish(&source, &staged, &target, rename_noreplace).unwrap();
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

            let error = publish(&source, &staged, &target, |staged, destination| {
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

            let error = copy_into_place(&source, &target, &source).unwrap_err();

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

        let error = copy_into_place(&source, &source, &source).unwrap_err();

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

        let error = copy_dir_into_place(&source, &target, &root).unwrap_err();

        assert!(error.to_string().contains("recursive copy"), "{error:#}");
        assert_eq!(std::fs::read_link(&target).unwrap(), original_link);
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "changed source");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);

        std::fs::remove_file(&source).unwrap();
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("content"), "restored directory").unwrap();
        copy_dir_into_place(&source, &target, &root).unwrap();
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

        copy_into_place(&source, &target, &source).unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "managed content");
        assert_eq!(
            target.metadata().unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 2);
    }
}
