use anyhow::{Context as _, Result};
use std::path::Path;

use super::platform::{is_link_like, remove_symlink};
use super::state::{link_points_to_source, same_entry};
use crate::infra::exec::Executor;
use crate::infra::fs::{TempGuard, ensure_parent_dir, rename_into_place};

/// Copy `source` into `target`, replacing the symlink that currently lives at
/// `target`. Files are staged to a sibling temp path first so that the window
/// where `target` is absent is as small as possible. Directories are handled
/// recursively via [`crate::infra::fs::copy_dir_recursive`]; symlinks within
/// the source tree are recreated as symlinks rather than followed, preventing
/// unintended traversal outside the source tree.
pub(super) fn copy_into_place(source: &Path, target: &Path, executor: &dyn Executor) -> Result<()> {
    ensure_parent_dir(target)?;

    if source.is_dir() {
        copy_dir_into_place(source, target, executor)
    } else {
        copy_file_into_place(source, target, executor)
    }
}

/// Clear the link that currently occupies `target` so staged content can be
/// renamed over it.
///
/// Replacement is only performed on links that still point to `source`, after
/// staging has finished. Replaced links and ordinary user files must survive.
/// `label` names the link flavour for the error context (`"symlink"` on files,
/// `"symlink/junction"` for directories on Windows).
fn clear_link_target(
    source: &Path,
    target: &Path,
    executor: &dyn Executor,
    label: &str,
) -> Result<()> {
    anyhow::ensure!(
        !same_entry(source, target),
        "refusing to replace the source entry while materializing: {}",
        target.display()
    );
    match crate::infra::fs::symlink_metadata_optional(target, "stat target")? {
        Some(meta) if is_link_like(target, &meta) => {
            let existing = std::fs::read_link(target)
                .with_context(|| format!("read {label}: {}", target.display()))?;
            anyhow::ensure!(
                link_points_to_source(target, &existing, source),
                "refusing to replace {label} that no longer points to the managed source: {}",
                target.display()
            );
            remove_symlink(target, executor)
                .with_context(|| format!("remove {label}: {}", target.display()))
        }
        Some(_) => Err(anyhow::anyhow!(
            "refusing to overwrite non-symlink target: {}",
            target.display()
        )),
        None => Ok(()),
    }
}

/// Copy a regular file: stage privately to a temp sibling, preserve the source
/// permissions, remove the managed symlink, and rename the temp file into place.
fn copy_file_into_place(source: &Path, target: &Path, executor: &dyn Executor) -> Result<()> {
    let (mut guard, mut staged_file) = create_staged_file(target)?;
    let mut source_file = std::fs::File::open(source)
        .with_context(|| format!("open source file {}", source.display()))?;
    std::io::copy(&mut source_file, &mut staged_file)
        .with_context(|| format!("copy source file {}", source.display()))?;
    std::fs::set_permissions(guard.path(), source_file.metadata()?.permissions())?;
    drop(staged_file);

    clear_link_target(source, target, executor, "symlink")?;

    rename_into_place(guard.path(), target)?;

    guard.persist();
    Ok(())
}

fn create_staged_file(target: &Path) -> Result<(TempGuard, std::fs::File)> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    TempGuard::create_unique_file_with_mode(parent, ".dotfiles-materialize-file", "tmp", 0o600)
        .with_context(|| format!("create temporary file beside {}", target.display()))
}

/// Copy a directory: stage into a sibling temp directory, remove the
/// symlink/junction, then rename the temp directory into place. Falls back to a
/// plain copy+delete when the rename crosses a filesystem boundary (EXDEV).
pub(super) fn copy_dir_into_place(
    source: &Path,
    target: &Path,
    executor: &dyn Executor,
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

    clear_link_target(source, target, executor, "symlink/junction")?;

    match std::fs::rename(guard.path(), target) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            crate::infra::fs::copy_dir_recursive(guard.path(), target, false).with_context(
                || {
                    format!(
                        "cross-fs copy {} to {}",
                        guard.path().display(),
                        target.display()
                    )
                },
            )?;
        }
        Err(e) => {
            return Err(anyhow::Error::new(e).context(format!(
                "rename {} to {}",
                guard.path().display(),
                target.display()
            )));
        }
    }

    if !guard.path().exists() {
        guard.persist();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt as _;

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

            let error = copy_into_place(&source, &target, &crate::infra::exec::MockExecutor::new())
                .unwrap_err();

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

        let error = copy_into_place(&source, &source, &crate::infra::exec::MockExecutor::new())
            .unwrap_err();

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
        let executor = crate::infra::exec::MockExecutor::new();

        let error = copy_dir_into_place(&source, &target, &executor).unwrap_err();

        assert!(error.to_string().contains("recursive copy"), "{error:#}");
        assert_eq!(std::fs::read_link(&target).unwrap(), original_link);
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "changed source");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);

        std::fs::remove_file(&source).unwrap();
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("content"), "restored directory").unwrap();
        copy_dir_into_place(&source, &target, &executor).unwrap();
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

        copy_into_place(&source, &target, &crate::infra::exec::MockExecutor::new()).unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "managed content");
        assert_eq!(
            target.metadata().unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 2);
    }
}
