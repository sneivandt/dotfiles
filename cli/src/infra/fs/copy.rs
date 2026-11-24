use anyhow::{Context as _, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Recursively copy a directory tree, preserving Unix directory permissions.
///
/// When `skip_git` is `true`, `.git` entries are skipped, including worktree
/// and submodule gitfiles — useful when Git metadata is unwanted.
///
/// Symlinks and Windows junctions within the source tree are **not followed**:
/// each link is recreated as a symlink in `dst` pointing to the same target.
/// On Windows this requires Developer Mode or elevated privileges.
/// Failure to recreate a link fails the copy rather than silently omitting it.
/// This prevents unexpected traversal of symlinks that point outside the
/// intended source tree. Existing destination links are rejected rather than
/// followed, so copying into an existing tree cannot write through those links.
/// A destination inside the source tree is rejected before anything is changed,
/// including when an existing ancestor aliases the source through a link.
///
/// # Errors
///
/// Returns an error if the destination directory cannot be created, a source
/// entry cannot be read, a file cannot be copied, a symlink cannot be recreated,
/// or (on Unix) directory permissions cannot be preserved.
pub fn copy_dir_recursive(src: &Path, dst: &Path, skip_git: bool) -> Result<()> {
    copy_tree(src, dst, skip_git, &mut HashMap::new())
}

fn copy_tree(
    src: &Path,
    dst: &Path,
    skip_git: bool,
    hard_links: &mut HashMap<(u64, u64), PathBuf>,
) -> Result<()> {
    let source_metadata = std::fs::metadata(src)
        .with_context(|| format!("reading directory metadata for {}", src.display()))?;
    anyhow::ensure!(
        source_metadata.is_dir(),
        "copy source is not a directory: {}",
        src.display()
    );
    reject_destination_link(dst)?;
    reject_recursive_destination(src, dst)?;
    std::fs::create_dir_all(dst)
        .with_context(|| format!("creating directory {}", dst.display()))?;
    #[cfg(windows)]
    super::place::verify_copy_entry_access(&std::fs::canonicalize(src)?, dst)
        .with_context(|| format!("checking directory access for {}", src.display()))?;
    // Keep staged contents private while copying, and defer the source mode
    // until children are populated so read-only directories can be copied.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(dst, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("restricting directory {}", dst.display()))?;
    }
    for entry in
        std::fs::read_dir(src).with_context(|| format!("reading directory {}", src.display()))?
    {
        let entry = entry.with_context(|| format!("reading entry in {}", src.display()))?;
        if skip_git && entry.file_name() == ".git" {
            continue;
        }
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        // Use symlink_metadata() so we detect symlinks without following them.
        let meta = src_path
            .symlink_metadata()
            .with_context(|| format!("reading metadata for {}", src_path.display()))?;

        if is_link_like(&src_path, &meta) {
            // Recreate the symlink in dst rather than following it, preventing
            // traversal of symlinks that point outside the intended source tree.
            let link_target = std::fs::read_link(&src_path)
                .with_context(|| format!("reading symlink {}", src_path.display()))?;
            #[cfg(windows)]
            verify_link_creation_access(&src_path, dst, super::is_dir_like(&meta))?;
            super::create_native_symlink(&link_target, &dst_path, super::is_dir_like(&meta))
                .with_context(|| {
                    format!(
                        "creating symlink {} -> {}",
                        dst_path.display(),
                        link_target.display()
                    )
                })?;
            #[cfg(target_os = "linux")]
            preserve_link_attributes(&src_path, &dst_path)?;
            #[cfg(windows)]
            super::place::verify_copy_entry_access(&src_path, &dst_path)?;
        } else if meta.is_dir() {
            copy_tree(&src_path, &dst_path, skip_git, hard_links)?;
        } else {
            reject_destination_link(&dst_path)?;
            anyhow::ensure!(
                meta.is_file(),
                "unsupported special file: {}",
                src_path.display()
            );
            let mut source = open_regular_source(&src_path)?;
            let identity = super::mutation::shared_file_identity(&source)?;
            if let Some(previous) = identity.and_then(|identity| hard_links.get(&identity)) {
                std::fs::hard_link(previous, &dst_path).with_context(|| {
                    format!(
                        "preserve hard link {} to {}",
                        dst_path.display(),
                        previous.display()
                    )
                })?;
                continue;
            }
            #[cfg(windows)]
            let destination = super::place::copy_destination_options()
                .create(true)
                .open(&dst_path);
            #[cfg(not(windows))]
            let destination = std::fs::File::create(&dst_path);
            let mut destination =
                destination.with_context(|| format!("create {}", dst_path.display()))?;
            copy_regular_contents(&mut source, &mut destination).with_context(|| {
                format!("copying {} to {}", src_path.display(), dst_path.display())
            })?;
            preserve_copy_metadata(&source, &destination)
                .with_context(|| format!("preserving metadata for {}", src_path.display()))?;
            if let Some(identity) = identity {
                hard_links.insert(identity, dst_path);
            }
        }
    }
    #[cfg(unix)]
    {
        let source = std::fs::File::open(src)?;
        let destination = std::fs::File::open(dst)?;
        preserve_copy_metadata(&source, &destination)
            .with_context(|| format!("preserving metadata for {}", src.display()))?;
    }
    Ok(())
}

#[cfg(windows)]
fn verify_link_creation_access(source: &Path, parent: &Path, directory: bool) -> Result<()> {
    // CreateSymbolicLinkW takes no security descriptor. Verify inheritance
    // on an empty sibling of the same type before exposing the link target.
    let probe = if directory {
        super::TempGuard::create_unique_dir(parent, ".dotfiles-access", "tmp")?
    } else {
        super::TempGuard::create_unique_file(parent, ".dotfiles-access", "tmp")?.0
    };
    super::place::verify_copy_entry_access(source, probe.path())
}

pub(crate) fn create_staged_file(target: &Path) -> Result<(super::TempGuard, std::fs::File)> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let prefix = ".dotfiles-materialize-file";
    #[cfg(windows)]
    let stage = super::TempGuard::reserve_file_with(parent, prefix, "tmp", |path| {
        super::place::copy_destination_options()
            .create_new(true)
            .open(path)
    });
    #[cfg(not(windows))]
    let stage = super::TempGuard::create_unique_file_with_mode(parent, prefix, "tmp", 0o600);
    stage.with_context(|| format!("create temporary file beside {}", target.display()))
}

/// Copy regular file contents, preserving sparse extents on supporting Linux filesystems.
pub(crate) fn copy_regular_contents(
    source: &mut std::fs::File,
    destination: &mut std::fs::File,
) -> Result<()> {
    #[cfg(windows)]
    {
        // Do not truncate an existing destination before checking its access.
        super::place::verify_copy_access(source, destination)?;
        destination.set_len(0)?;
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt as _;
        let metadata = source.metadata()?;
        if metadata.blocks().saturating_mul(512) < metadata.len()
            && copy_sparse_contents(source, destination, metadata.len())?
        {
            return Ok(());
        }
    }
    std::io::copy(source, destination)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn copy_sparse_contents(
    source: &mut std::fs::File,
    destination: &mut std::fs::File,
    length: u64,
) -> Result<bool> {
    use rustix::fs::{SeekFrom, seek};
    use rustix::io::Errno;
    use std::io::{Read as _, Seek as _};

    let mut data = match seek(&*source, SeekFrom::Data(0)) {
        Ok(offset) => offset,
        Err(Errno::NXIO) => length,
        Err(Errno::INVAL | Errno::NOTSUP) => {
            source.rewind()?;
            return Ok(false);
        }
        Err(error) => return Err(error.into()),
    };
    while data < length {
        let hole = seek(&*source, SeekFrom::Hole(data))?.min(length);
        anyhow::ensure!(hole > data, "invalid sparse file extent");
        seek(&*source, SeekFrom::Start(data))?;
        seek(&*destination, SeekFrom::Start(data))?;
        let count = hole - data;
        let copied = std::io::copy(&mut (&mut *source).take(count), destination)?;
        anyhow::ensure!(
            copied == count,
            "source changed while copying a sparse extent"
        );
        data = match seek(&*source, SeekFrom::Data(hole)) {
            Ok(offset) => offset,
            Err(Errno::NXIO) => break,
            Err(error) => return Err(error.into()),
        };
    }
    destination.set_len(length)?;
    Ok(true)
}

#[cfg(target_os = "linux")]
fn is_acl_xattr(name: &[u8]) -> bool {
    name.starts_with(b"system.posix_acl_") || name == b"system.nfs4_acl"
}

#[cfg(target_os = "linux")]
fn is_user_preservable_xattr(name: &[u8]) -> bool {
    name.starts_with(b"user.") || is_acl_xattr(name)
}

/// Preserve metadata before publishing a staged copy.
pub(crate) fn preserve_copy_metadata(
    source: &std::fs::File,
    destination: &std::fs::File,
) -> Result<()> {
    let metadata = source.metadata()?;
    #[cfg(target_os = "linux")]
    preserve_attributes_and_permissions(
        source,
        destination,
        access_identity(&metadata),
        is_user_preservable_xattr,
        || destination.set_permissions(metadata.permissions()),
    )?;
    #[cfg(not(target_os = "linux"))]
    destination.set_permissions(metadata.permissions())?;
    #[cfg(windows)]
    super::place::verify_copy_access(source, destination)?;
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn preserve_replacement_metadata(
    source: &std::fs::File,
    destination: &std::fs::File,
) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    // Content signatures and capabilities must not be attached to new bytes.
    let metadata = source.metadata()?;
    let mode = metadata.permissions().mode() & 0o777;
    preserve_attributes_and_permissions(
        source,
        destination,
        access_identity(&metadata),
        is_user_preservable_xattr,
        || destination.set_permissions(std::fs::Permissions::from_mode(mode)),
    )?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn access_identity(metadata: &std::fs::Metadata) -> (u32, u32) {
    use std::os::unix::fs::MetadataExt as _;
    (metadata.uid(), metadata.gid())
}

#[cfg(target_os = "linux")]
fn preserve_attributes_and_permissions(
    source: &std::fs::File,
    destination: &std::fs::File,
    source_identity: (u32, u32),
    preserve: impl Fn(&[u8]) -> bool,
    set_permissions: impl FnOnce() -> std::io::Result<()>,
) -> Result<()> {
    use rustix::fs::{
        XattrFlags, fgetxattr, fgetxattr_query, flistxattr, flistxattr_query, fremovexattr,
        fsetxattr,
    };
    use rustix::io::Errno;

    // The owner and group interpret the mode bits even when there is no
    // access ACL. Never restore attributes or modes onto different principals.
    anyhow::ensure!(
        access_identity(&destination.metadata()?) == source_identity,
        "the staged copy has a different owner or group and would change access"
    );
    let names = read_attribute_names(flistxattr_query(source), |buffer| {
        flistxattr(source, buffer)
    })?;
    let names: Vec<_> = names
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty() && preserve(name))
        .collect();
    for acl in ["system.posix_acl_access", "system.posix_acl_default"] {
        if !names.contains(&acl.as_bytes()) {
            match fremovexattr(destination, acl) {
                Ok(()) | Err(Errno::NODATA | Errno::NOTSUP) => {}
                Err(error) => {
                    return Err(error).with_context(|| format!("clear inherited {acl}"));
                }
            }
        }
    }
    let copy_xattr = |name: &[u8]| -> Result<()> {
        let name = std::ffi::OsStr::from_bytes(name);
        let mut value = vec![0; fgetxattr_query(source, name)?.max(1)];
        let size = fgetxattr(source, name, value.as_mut_slice())?;
        value.truncate(size);
        fsetxattr(destination, name, &value, XattrFlags::empty())
            .with_context(|| format!("copy extended attribute {}", name.to_string_lossy()))?;
        Ok(())
    };
    // Write user.* xattrs before fchmod so read-only targets remain writable.
    for name in names.iter().copied().filter(|name| !is_acl_xattr(name)) {
        copy_xattr(name)?;
    }
    set_permissions()?;
    // Restore ACL xattrs after fchmod: on NFSv4 (RFC 7530/8881), SETATTR
    // mode rewrites/masks an existing system.nfs4_acl if applied afterward.
    for name in names.iter().copied().filter(|name| is_acl_xattr(name)) {
        copy_xattr(name)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStrExt as _;

#[cfg(target_os = "linux")]
fn read_attribute_names(
    size: rustix::io::Result<usize>,
    read: impl FnOnce(&mut [u8]) -> rustix::io::Result<usize>,
) -> rustix::io::Result<Vec<u8>> {
    match size {
        Err(rustix::io::Errno::NOTSUP) => Ok(Vec::new()),
        other => {
            let mut names = vec![0; other?.max(1)];
            let used = read(&mut names)?;
            names.truncate(used);
            Ok(names)
        }
    }
}

#[cfg(target_os = "linux")]
fn preserve_link_attributes(source: &Path, destination: &Path) -> Result<()> {
    use rustix::fs::{
        XattrFlags, lgetxattr, lgetxattr_query, llistxattr, llistxattr_query, lsetxattr,
    };

    let names = read_attribute_names(llistxattr_query(source), |buffer| {
        llistxattr(source, buffer)
    })?;
    for name in names
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty() && is_user_preservable_xattr(name))
    {
        let name = std::ffi::OsStr::from_bytes(name);
        let mut value = vec![0; lgetxattr_query(source, name)?.max(1)];
        let size = lgetxattr(source, name, value.as_mut_slice())?;
        value.truncate(size);
        lsetxattr(destination, name, &value, XattrFlags::empty()).with_context(|| {
            format!(
                "copy extended attribute {} on {}",
                name.to_string_lossy(),
                source.display()
            )
        })?;
    }
    Ok(())
}

fn reject_recursive_destination(src: &Path, dst: &Path) -> Result<()> {
    let source = std::fs::canonicalize(src)
        .with_context(|| format!("resolving copy source {}", src.display()))?;
    let destination = std::path::absolute(dst)
        .with_context(|| format!("resolving copy destination {}", dst.display()))?;

    // Missing destination components have no canonical path yet. Resolve the
    // nearest existing ancestor first so symlinks followed by `..` retain their
    // filesystem meaning instead of being normalized lexically too early.
    for ancestor in destination.ancestors() {
        match std::fs::canonicalize(ancestor) {
            Ok(mut resolved) => {
                for component in destination.strip_prefix(ancestor)?.components() {
                    if component == std::path::Component::ParentDir {
                        resolved.pop();
                    } else {
                        resolved.push(component.as_os_str());
                    }
                }
                anyhow::ensure!(
                    !resolved.starts_with(&source),
                    "refusing to copy {} into itself: {}",
                    src.display(),
                    dst.display()
                );
                return Ok(());
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound && ancestor.parent().is_some() => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("resolving copy destination {}", dst.display()));
            }
        }
    }
    anyhow::bail!("cannot resolve copy destination {}", dst.display())
}

/// Reject an existing symlink or junction before copying into `path`.
///
/// # Errors
///
/// Returns an error if metadata cannot be read or the destination is a link.
pub fn reject_destination_link(path: &Path) -> Result<()> {
    if let Some(metadata) = super::symlink_metadata_optional(path, "reading copy destination")? {
        anyhow::ensure!(
            !is_link_like(path, &metadata),
            "refusing to copy through destination link {}",
            path.display()
        );
    }
    Ok(())
}

fn is_link_like(path: &Path, metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        is_windows_reparse_link(path, metadata)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        metadata.is_symlink()
    }
}

#[cfg(windows)]
fn is_windows_reparse_link(path: &Path, metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    metadata.is_symlink()
        || (metadata.file_attributes() & 0x400 != 0 && std::fs::read_link(path).is_ok())
}

/// Open a copy source without waiting on a FIFO that replaced a regular file.
pub(crate) fn open_regular_source(path: &Path) -> Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(nix::libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .with_context(|| format!("open source file {}", path.display()))?;
    anyhow::ensure!(
        file.metadata()?.is_file(),
        "unsupported special file: {}",
        path.display()
    );
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn windows_copy_checks_access_before_truncating_or_writing() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        let dst = root.path().join("destination");
        std::fs::create_dir(&src).unwrap();
        std::fs::create_dir(&dst).unwrap();
        let source = src.join("entry");
        let destination = dst.join("entry");
        crate::infra::fs::write_atomic(&source, "private").unwrap();
        std::fs::write(&destination, "original").unwrap();
        let error = copy_dir_recursive(&src, &dst, false).unwrap_err();
        assert!(
            format!("{error:#}").contains("Windows access policy"),
            "{error:#}"
        );
        assert_eq!(std::fs::read_to_string(&destination).unwrap(), "original");

        // A destination with the same protected descriptor can receive the bytes.
        std::fs::remove_file(&destination).unwrap();
        crate::infra::fs::write_atomic(&destination, "original").unwrap();
        copy_dir_recursive(&src, &dst, false).unwrap();
        assert_eq!(std::fs::read_to_string(&destination).unwrap(), "private");
        super::super::place::verify_copy_entry_access(&source, &destination).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn mode_only_files_and_default_acl_only_dirs_require_matching_principals() {
        use rustix::fs::{XattrFlags, fgetxattr, fgetxattr_query, fsetxattr};
        use rustix::io::Errno;
        let root = tempfile::tempdir_in(".").unwrap();
        // Linux POSIX ACL v2 with just owner, owning group and other.
        let mut acl = 2_u32.to_le_bytes().to_vec();
        for (tag, rights) in [(1_u16, 7_u16), (4, 5), (32, 0)] {
            acl.extend(tag.to_le_bytes());
            acl.extend(rights.to_le_bytes());
            acl.extend(u32::MAX.to_le_bytes());
        }
        let default = "system.posix_acl_default";
        for is_dir in [false, true] {
            let create = |name: &str| {
                let path = root.path().join(format!("{name}-{is_dir}"));
                if is_dir {
                    std::fs::create_dir(&path).unwrap();
                    std::fs::File::open(path).unwrap()
                } else {
                    std::fs::File::create_new(path).unwrap()
                }
            };
            let source = create("source");
            let staged = create("staged");
            if is_dir {
                fsetxattr(&source, default, &acl, XattrFlags::empty()).unwrap();
            }
            assert_eq!(
                fgetxattr_query(&source, "system.posix_acl_access"),
                Err(Errno::NODATA)
            );
            let (uid, gid) = access_identity(&source.metadata().unwrap());
            // Inject only the IDs: no chown, other account, or privileged test runner.
            for mismatch in [(uid ^ 1, gid), (uid, gid ^ 1)] {
                let error = preserve_attributes_and_permissions(
                    &source,
                    &staged,
                    mismatch,
                    |_| true,
                    || Ok(()),
                )
                .unwrap_err();
                assert!(
                    error.to_string().contains("different owner or group"),
                    "{error:#}"
                );
                assert_eq!(fgetxattr_query(&staged, default), Err(Errno::NODATA));
            }
            preserve_copy_metadata(&source, &staged).unwrap();
            if is_dir {
                let mut actual = vec![0; acl.len()];
                assert_eq!(
                    fgetxattr(&staged, default, actual.as_mut_slice()).unwrap(),
                    acl.len()
                );
                assert_eq!(actual, acl);
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn preserve_copy_metadata_restores_user_xattr_and_acl_on_readonly_file() {
        use rustix::fs::{XattrFlags, fgetxattr, fgetxattr_query, fsetxattr};
        use std::os::unix::fs::PermissionsExt as _;

        let root = tempfile::tempdir_in(".").unwrap();
        let source_path = root.path().join("source-ro");
        let staged_path = root.path().join("staged-ro");
        let source = std::fs::File::create_new(&source_path).unwrap();
        let staged = std::fs::File::create_new(&staged_path).unwrap();

        let (uid, _) = access_identity(&source.metadata().unwrap());
        // Extended POSIX access ACL (owner, named user, group, mask, other).
        let mut acl = 2_u32.to_le_bytes().to_vec();
        for (tag, rights, id) in [
            (1_u16, 4_u16, u32::MAX),
            (2, 4, uid),
            (4, 0, u32::MAX),
            (16, 4, u32::MAX),
            (32, 0, u32::MAX),
        ] {
            acl.extend(tag.to_le_bytes());
            acl.extend(rights.to_le_bytes());
            acl.extend(id.to_le_bytes());
        }
        fsetxattr(&source, "user.dotfiles.tag", b"ok", XattrFlags::empty()).unwrap();
        source
            .set_permissions(std::fs::Permissions::from_mode(0o400))
            .unwrap();
        fsetxattr(
            &source,
            "system.posix_acl_access",
            &acl,
            XattrFlags::empty(),
        )
        .unwrap();

        let mut acl_present_during_chmod = false;
        preserve_attributes_and_permissions(
            &source,
            &staged,
            access_identity(&source.metadata().unwrap()),
            is_user_preservable_xattr,
            || {
                acl_present_during_chmod =
                    fgetxattr_query(&staged, "system.posix_acl_access").is_ok();
                staged.set_permissions(source.metadata()?.permissions())
            },
        )
        .unwrap();

        assert!(
            !acl_present_during_chmod,
            "ACL xattrs must be restored after set_permissions so fchmod cannot rewrite NFSv4/POSIX ACLs"
        );
        let mut actual_tag = vec![0; 2];
        assert_eq!(
            fgetxattr(&staged, "user.dotfiles.tag", actual_tag.as_mut_slice()).unwrap(),
            2
        );
        assert_eq!(actual_tag, b"ok");
        let mut actual_acl = vec![0; acl.len()];
        assert_eq!(
            fgetxattr(
                &staged,
                "system.posix_acl_access",
                actual_acl.as_mut_slice()
            )
            .unwrap(),
            acl.len()
        );
        assert_eq!(actual_acl, acl);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn user_preservable_xattr_filter_excludes_privileged_namespaces() {
        for allowed in [
            b"user.dotfiles.tag".as_slice(),
            b"system.posix_acl_access",
            b"system.posix_acl_default",
            b"system.nfs4_acl",
        ] {
            assert!(
                is_user_preservable_xattr(allowed),
                "should preserve {}",
                String::from_utf8_lossy(allowed)
            );
        }
        for excluded in [
            b"security.selinux".as_slice(),
            b"security.capability",
            b"security.ima",
            b"trusted.overlay.opaque",
        ] {
            assert!(
                !is_user_preservable_xattr(excluded),
                "must exclude privileged xattr {}",
                String::from_utf8_lossy(excluded)
            );
        }
    }

    #[test]
    fn destination_resolution_preserves_non_missing_io_errors() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        std::fs::create_dir(&source).unwrap();
        let destination = root.path().join("invalid\0destination");
        let expected = std::fs::canonicalize(&destination).unwrap_err().kind();
        assert_ne!(expected, std::io::ErrorKind::NotFound);

        let error = reject_recursive_destination(&source, &destination).unwrap_err();

        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            expected
        );
        assert_eq!(
            error.to_string(),
            format!("resolving copy destination {}", destination.display())
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn copy_sparse_and_xattr_filter_and_identity_edge_cases() {
        use rustix::fs::{XattrFlags, fgetxattr_query, fsetxattr};
        use rustix::io::Errno;
        use std::os::unix::fs::MetadataExt as _;

        let root = tempfile::tempdir_in(".").unwrap();

        // All-holes sparse file exercises `data == length` after SEEK_DATA returns ENXIO.
        let sparse_src_path = root.path().join("all-holes-src");
        let sparse_dst_path = root.path().join("all-holes-dst");
        let mut sparse_src = std::fs::File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&sparse_src_path)
            .unwrap();
        sparse_src.set_len(64 * 1024).unwrap();
        let mut sparse_dst = std::fs::File::create_new(&sparse_dst_path).unwrap();
        assert!(copy_sparse_contents(&mut sparse_src, &mut sparse_dst, 64 * 1024).unwrap());
        assert_eq!(sparse_dst.metadata().unwrap().len(), 64 * 1024);

        // access_identity must report the file's real (uid, gid), never a constant pair.
        let src_meta = sparse_src.metadata().unwrap();
        assert_eq!(access_identity(&src_meta), (src_meta.uid(), src_meta.gid()));

        // preserve_attributes_and_permissions must skip xattrs rejected by `preserve`.
        let xattr_src = std::fs::File::create_new(root.path().join("xattr-src")).unwrap();
        let xattr_dst = std::fs::File::create_new(root.path().join("xattr-dst")).unwrap();
        fsetxattr(&xattr_src, "user.keep", b"1", XattrFlags::empty()).unwrap();
        fsetxattr(&xattr_src, "user.skip", b"2", XattrFlags::empty()).unwrap();
        preserve_attributes_and_permissions(
            &xattr_src,
            &xattr_dst,
            access_identity(&xattr_src.metadata().unwrap()),
            |name| name == b"user.keep",
            || Ok(()),
        )
        .unwrap();
        assert_eq!(fgetxattr_query(&xattr_dst, "user.keep"), Ok(1));
        assert_eq!(fgetxattr_query(&xattr_dst, "user.skip"), Err(Errno::NODATA));
    }
}
