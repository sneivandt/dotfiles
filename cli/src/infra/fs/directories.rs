//! Read-only preflight for managed directory trees.

use std::path::{Component, Path, PathBuf};

use anyhow::{Result, ensure};

/// Inspect a directory chain beneath a trusted root without following links.
/// Missing directories are allowed; callers must repeat this check before mutation.
///
/// # Errors
/// Returns an error for escaping components, links, non-directories or inspection failures.
pub(crate) fn checked_directory_chain(root: &Path, relative: &Path) -> Result<PathBuf> {
    let mut directory = root.to_path_buf();
    check_directory(&directory)?;
    for component in relative.components() {
        ensure!(
            matches!(component, Component::Normal(_)),
            "unsafe directory component in {}",
            relative.display()
        );
        directory.push(component);
        check_directory(&directory)?;
    }
    Ok(directory)
}

fn check_directory(directory: &Path) -> Result<()> {
    super::reject_destination_link(directory)?;
    if let Some(metadata) =
        super::symlink_metadata_optional(directory, "inspect managed directory")?
    {
        ensure!(
            metadata.is_dir(),
            "managed directory is not a directory: {}",
            directory.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_directories_are_inspected_without_creation_and_unsafe_paths_fail() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            checked_directory_chain(root.path(), Path::new("a/b")).unwrap(),
            root.path().join("a/b")
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        std::fs::create_dir_all(root.path().join("existing/child")).unwrap();
        assert_eq!(
            checked_directory_chain(root.path(), Path::new("existing/child")).unwrap(),
            root.path().join("existing/child")
        );
        for path in ["../outside", "/outside", "a/../../outside", "bad\0path"] {
            assert!(
                checked_directory_chain(root.path(), Path::new(path)).is_err(),
                "{path}"
            );
        }
        std::fs::write(root.path().join("a"), "keep").unwrap();
        assert!(checked_directory_chain(root.path(), Path::new("a/b")).is_err());
        assert_eq!(
            std::fs::read_to_string(root.path().join("a")).unwrap(),
            "keep"
        );
    }

    #[cfg(unix)]
    #[test]
    fn links_at_every_depth_and_dangling_links_are_rejected() {
        for relative in ["", "a", "a/b"] {
            for dangling in [false, true] {
                let fixture = tempfile::tempdir().unwrap();
                let root = fixture.path().join("root");
                let external = fixture.path().join("external");
                if !dangling {
                    std::fs::create_dir(&external).unwrap();
                }
                let link = if relative.is_empty() {
                    root.clone()
                } else {
                    root.join(relative)
                };
                std::fs::create_dir_all(link.parent().unwrap()).unwrap();
                std::os::unix::fs::symlink(&external, &link).unwrap();
                assert!(checked_directory_chain(&root, Path::new("a/b/c")).is_err());
                assert!(link.is_symlink());
                if !dangling {
                    assert_eq!(std::fs::read_dir(external).unwrap().count(), 0);
                }
            }
        }
    }
}
