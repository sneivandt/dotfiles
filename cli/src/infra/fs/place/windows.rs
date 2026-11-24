//! Stage under the intended access policy, not just the destination's parent.
#![allow(
    unsafe_code,
    reason = "Win32 exposes file security descriptors only through pointer APIs"
)]

use anyhow::{Context as _, Result};
use std::ffi::CStr;
use std::fs::File;
use std::io::{self, Write as _};
use std::os::windows::{
    ffi::OsStrExt as _, fs::MetadataExt as _, fs::OpenOptionsExt as _, io::AsRawHandle as _,
    io::FromRawHandle as _,
};
use std::path::Path;
use std::ptr;
use windows_sys::Win32::{
    Foundation::{ERROR_SUCCESS, GENERIC_WRITE, INVALID_HANDLE_VALUE, LocalFree},
    Security::{
        ATTRIBUTE_SECURITY_INFORMATION,
        Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorA, GetSecurityInfo, SDDL_REVISION_1,
            SE_FILE_OBJECT,
        },
        DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION, GetSecurityDescriptorControl,
        GetSecurityDescriptorDacl, GetSecurityDescriptorLength, LABEL_SECURITY_INFORMATION,
        OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, QuerySecurityAccessMask,
        SCOPE_SECURITY_INFORMATION, SE_DACL_PROTECTED, SE_SELF_RELATIVE, SECURITY_ATTRIBUTES,
    },
    Storage::FileSystem::{
        CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_ENCRYPTED, FILE_ATTRIBUTE_NORMAL,
        FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
        GetVolumeInformationByHandleW, READ_CONTROL,
    },
    System::SystemServices::{
        ACCESS_FILTER_SECURITY_INFORMATION, FILE_PERSISTENT_ACLS,
        PROCESS_TRUST_LABEL_SECURITY_INFORMATION,
    },
};

use super::TempGuard;

mod private_dir;
#[cfg(test)]
pub(crate) use private_dir::check_private_fixture;
pub(in crate::infra::fs) use private_dir::{create_private_dir, verify_private_child};

pub(super) fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
    // Resolve only the parent, for verbatim long paths without following a final link.
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path.file_name().ok_or(io::ErrorKind::InvalidInput)?;
    let absolute = std::fs::canonicalize(parent)?.join(name);
    let mut wide: Vec<_> = absolute.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    wide.push(0);
    Ok(wide)
}

const ACCESS_FLAGS: [u32; 8] = [
    OWNER_SECURITY_INFORMATION,
    GROUP_SECURITY_INFORMATION,
    DACL_SECURITY_INFORMATION,
    LABEL_SECURITY_INFORMATION,
    ATTRIBUTE_SECURITY_INFORMATION,
    SCOPE_SECURITY_INFORMATION,
    PROCESS_TRUST_LABEL_SECURITY_INFORMATION.cast_unsigned(),
    ACCESS_FILTER_SECURITY_INFORMATION.cast_unsigned(),
];

fn require_query_support() -> Result<()> {
    for flag in ACCESS_FLAGS {
        let mut rights = 0;
        // SAFETY: pure mask query with a writable out pointer. Query flags individually
        // so another flag requiring READ_CONTROL cannot mask an unrecognized bit.
        unsafe { QuerySecurityAccessMask(flag, &raw mut rights) };
        anyhow::ensure!(
            rights == READ_CONTROL,
            "unsupported access-policy query {flag:#x} (required rights {rights:#x})"
        );
    }
    Ok(())
}

/// Compare the complete self-relative projection, including inheritance control bits.
#[derive(Debug)]
struct Access {
    descriptor: LocalDescriptor,
    len: usize,
}

impl PartialEq for Access {
    fn eq(&self, other: &Self) -> bool {
        self.bytes() == other.bytes()
    }
}

impl Eq for Access {}

const ACCESS_INFO_MASK: u32 = OWNER_SECURITY_INFORMATION
    | GROUP_SECURITY_INFORMATION
    | DACL_SECURITY_INFORMATION
    | LABEL_SECURITY_INFORMATION
    | ATTRIBUTE_SECURITY_INFORMATION
    | SCOPE_SECURITY_INFORMATION
    | PROCESS_TRUST_LABEL_SECURITY_INFORMATION.cast_unsigned()
    | ACCESS_FILTER_SECURITY_INFORMATION.cast_unsigned();
const STAGED_WRITE_ACCESS: u32 = GENERIC_WRITE | READ_CONTROL | FILE_READ_ATTRIBUTES;
const INSPECT_ACCESS: u32 = READ_CONTROL | FILE_READ_ATTRIBUTES;
const INSPECT_FLAGS: u32 = FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS;

fn validate_self_relative_descriptor(len: u32, control: u16) -> io::Result<usize> {
    if len == 0 || control & SE_SELF_RELATIVE == 0 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    usize::try_from(len).map_err(io::Error::other)
}

fn matches_private_descriptor(staged: &Access, descriptor: PSECURITY_DESCRIPTOR) -> Result<bool> {
    Ok(staged.protected()? && same_dacl(descriptor, staged.as_ptr())?)
}

fn check_dacl_present(present: i32, is_null: bool) -> io::Result<()> {
    if present == 0 || is_null {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(())
}

impl Access {
    fn read(file: &File) -> io::Result<Self> {
        let descriptor = LocalDescriptor::from_file(file, ACCESS_INFO_MASK)?;
        // SAFETY: the OS allocated and initialized this descriptor.
        let len = unsafe { GetSecurityDescriptorLength(descriptor.0) };
        let len = validate_self_relative_descriptor(len, descriptor.control()?)?;
        Ok(Self { descriptor, len })
    }

    const fn bytes(&self) -> &[u8] {
        // SAFETY: self-relative descriptor is one live buffer of the reported length.
        unsafe { std::slice::from_raw_parts(self.as_ptr().cast(), self.len) }
    }

    const fn as_ptr(&self) -> PSECURITY_DESCRIPTOR {
        self.descriptor.0
    }

    fn protected(&self) -> io::Result<bool> {
        Ok(self.descriptor.control()? & SE_DACL_PROTECTED != 0)
    }
}

#[derive(Debug)]
struct LocalDescriptor(PSECURITY_DESCRIPTOR);

impl LocalDescriptor {
    fn from_file(file: &File, info: u32) -> io::Result<Self> {
        let mut descriptor = ptr::null_mut();
        // SAFETY: request only the owned descriptor for a live file handle.
        let status = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                info,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &raw mut descriptor,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status.cast_signed()));
        }
        if descriptor.is_null() {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(Self(descriptor))
    }

    fn control(&self) -> io::Result<u16> {
        let mut control = 0;
        let mut revision = 0;
        // SAFETY: initialized descriptor returned by Windows, valid output pointers.
        if unsafe { GetSecurityDescriptorControl(self.0, &raw mut control, &raw mut revision) } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(control)
    }
    fn parse(sddl: &CStr) -> io::Result<Self> {
        let mut descriptor = ptr::null_mut();
        // SAFETY: NUL-terminated SDDL and valid out pointer. On success we own the allocation.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorA(
                sddl.as_ptr().cast(),
                SDDL_REVISION_1,
                &raw mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(descriptor))
    }
}

impl Drop for LocalDescriptor {
    fn drop(&mut self) {
        // SAFETY: both GetSecurityInfo and SDDL conversion transfer a LocalFree allocation.
        unsafe { LocalFree(self.0) };
    }
}

// Both descriptors come from Windows (a successful query or SDDL conversion).
fn same_dacl(left: PSECURITY_DESCRIPTOR, right: PSECURITY_DESCRIPTOR) -> io::Result<bool> {
    fn acl_bytes(sd: PSECURITY_DESCRIPTOR) -> io::Result<Vec<u8>> {
        let mut present = 0;
        let mut defaulted = 0;
        let mut acl = ptr::null_mut();
        // SAFETY: caller holds the valid descriptor; all out pointers are writable.
        if unsafe {
            GetSecurityDescriptorDacl(sd, &raw mut present, &raw mut acl, &raw mut defaulted)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        check_dacl_present(present, acl.is_null())?;
        // SAFETY: the returned ACL lies in the live, OS-produced descriptor.
        Ok(unsafe { std::slice::from_raw_parts(acl.cast(), usize::from((*acl).AclSize)) }.to_vec())
    }
    Ok(acl_bytes(left)? == acl_bytes(right)?)
}

fn create_new(path: &Path, descriptor: PSECURITY_DESCRIPTOR) -> io::Result<File> {
    let path = wide_path(path)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).map_err(io::Error::other)?,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    // SAFETY: path is NUL-terminated. Any supplied descriptor remains live during creation.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            STAGED_WRITE_ACCESS,
            FILE_SHARE_DELETE,
            &raw const attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CREATE_NEW returned a new, owned handle; transfer it exactly once.
    Ok(unsafe { File::from_raw_handle(handle) })
}

fn require_persistent_acls(file: &File) -> Result<()> {
    let mut flags = 0;
    // SAFETY: live handle, valid flags out pointer; all optional outputs omitted.
    if unsafe {
        GetVolumeInformationByHandleW(
            file.as_raw_handle(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            &raw mut flags,
            ptr::null_mut(),
            0,
        )
    } == 0
    {
        return Err(io::Error::last_os_error().into());
    }
    anyhow::ensure!(
        flags & FILE_PERSISTENT_ACLS != 0,
        "volume cannot enforce file ACLs"
    );
    Ok(())
}

fn regular_access(file: &File, path: &Path) -> Result<Access> {
    let metadata = file.metadata()?;
    if metadata.is_dir() {
        return Err(io::Error::from(io::ErrorKind::IsADirectory).into());
    }
    anyhow::ensure!(
        metadata.is_file()
            && metadata.file_attributes()
                & (FILE_ATTRIBUTE_REPARSE_POINT
                    | FILE_ATTRIBUTE_ENCRYPTED
                    | FILE_ATTRIBUTE_READONLY)
                == 0,
        "cannot retain access for non-regular, encrypted or read-only target {}",
        path.display()
    );
    Ok(Access::read(file)?)
}

fn open_target(path: &Path) -> Result<Option<File>> {
    match File::options()
        .access_mode(INSPECT_ACCESS)
        .custom_flags(INSPECT_FLAGS)
        .open(path)
    {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(in crate::infra::fs) fn verify_copy_access(source: &File, staged: &File) -> Result<()> {
    require_query_support()?;
    require_persistent_acls(staged)?;
    anyhow::ensure!(
        source.metadata()?.file_attributes() & FILE_ATTRIBUTE_ENCRYPTED == 0,
        "cannot materialize an encrypted source"
    );
    anyhow::ensure!(
        Access::read(source)? == Access::read(staged)?,
        "cannot materialize this entry without changing its Windows access policy"
    );
    Ok(())
}

pub(in crate::infra::fs) fn copy_destination_options() -> std::fs::OpenOptions {
    let mut options = File::options();
    // GetVolumeInformationByHandleW also needs FILE_READ_ATTRIBUTES.
    options.write(true).access_mode(STAGED_WRITE_ACCESS);
    options
}

pub(in crate::infra::fs) fn verify_copy_entry_access(source: &Path, staged: &Path) -> Result<()> {
    // Inspect the entry itself, including a directory junction or a dangling link.
    let source = open_target(source)?.context("copy source disappeared")?;
    let staged = open_target(staged)?.context("staged copy disappeared")?;
    verify_copy_access(&source, &staged)
}

fn target_access(path: &Path) -> Result<Option<Access>> {
    open_target(path)?
        .as_ref()
        .map(|file| regular_access(file, path))
        .transpose()
}

pub(super) fn write_checked(
    path: &Path,
    content: impl AsRef<[u8]>,
    check: impl FnOnce() -> Result<()>,
) -> Result<()> {
    require_query_support()?;
    let source =
        open_target(path).with_context(|| format!("inspect access for {}", path.display()))?;
    let before = source
        .as_ref()
        .map(|file| regular_access(file, path))
        .transpose()
        .with_context(|| format!("inspect access for {}", path.display()))?;
    let creation = match source.as_ref().zip(before.as_ref()) {
        // Do not replay another owner's SID or a projected SACL as creation
        // instructions. Let Windows assign these normally, then verify all of them.
        Some((file, access)) if access.protected()? => {
            Some(LocalDescriptor::from_file(file, DACL_SECURITY_INFORMATION)?)
        }
        Some(_) => None,
        None => Some(LocalDescriptor::parse(c"D:P(A;;FA;;;OW)")?),
    };
    drop(source);
    let descriptor = creation.as_ref().map_or(ptr::null_mut(), |sd| sd.0);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let (guard, mut file) = TempGuard::reserve_file_with(parent, ".dotfiles-write", "tmp", |p| {
        create_new(p, descriptor)
    })
    .with_context(|| format!("create protected staging file beside {}", path.display()))?;
    // Remove the staging name on error before releasing its deny-read/write handle.
    let mut guard = guard;
    require_persistent_acls(&file)?;
    let staged = regular_access(&file, guard.path())?;
    let retained = match &before {
        Some(access) => access == &staged,
        None => matches_private_descriptor(&staged, descriptor)?,
    };
    anyhow::ensure!(
        retained,
        "cannot retain the access descriptor for {}",
        path.display()
    );
    file.write_all(content.as_ref())
        .with_context(|| format!("write temporary file beside {}", path.display()))?;
    check()?;
    anyhow::ensure!(
        target_access(path)? == before && regular_access(&file, guard.path())? == staged,
        "access changed while preparing {}",
        path.display()
    );
    if before.is_some() {
        super::rename_into_place(guard.path(), path)?;
    } else {
        super::rename_noreplace(guard.path(), path)
            .with_context(|| format!("place new file at {}", path.display()))?;
    }
    guard.persist();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_files_are_owner_only_and_never_replace_a_newly_arrived_target() {
        let root = tempfile::tempdir_in(".").unwrap();
        let target = root.path().join("document");
        let private = LocalDescriptor::parse(c"D:P(A;;FA;;;OW)").unwrap();
        let verify_private = |path: &Path| {
            let access = target_access(path).unwrap().unwrap();
            assert!(access.protected().unwrap());
            assert!(same_dacl(access.as_ptr(), private.0).unwrap());
        };
        write_checked(&target, b"fixture", || {
            let staged = std::fs::read_dir(root.path())
                .unwrap()
                .next()
                .unwrap()?
                .path();
            verify_private(&staged);
            Ok(())
        })
        .unwrap();
        verify_private(&target);

        let missing = root.path().join("initially absent");
        assert!(
            write_checked(&missing, b"replacement", || {
                std::fs::write(&missing, b"arrived during staging")?;
                Ok(())
            })
            .is_err()
        );
        assert_eq!(std::fs::read(&missing).unwrap(), b"arrived during staging");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    }

    #[test]
    fn inherits_or_retains_protected_access_before_payload_and_at_publication() {
        struct Inspect<'a> {
            directory: &'a Path,
            expected: &'a Access,
        }
        impl AsRef<[u8]> for Inspect<'_> {
            fn as_ref(&self) -> &[u8] {
                let entry = std::fs::read_dir(self.directory)
                    .unwrap()
                    .map(std::result::Result::unwrap)
                    .find(|e| {
                        e.file_name()
                            .to_string_lossy()
                            .starts_with(".dotfiles-write-")
                    })
                    .unwrap();
                assert_eq!(
                    target_access(&entry.path()).unwrap().as_ref(),
                    Some(self.expected)
                );
                assert_eq!(
                    File::open(entry.path()).unwrap_err().raw_os_error(),
                    Some(windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION.cast_signed())
                );
                b"synthetic replacement"
            }
        }
        let root = tempfile::tempdir_in(".").unwrap();
        for protected in [false, true] {
            let target = root.path().join(format!("document-{protected}"));
            if protected {
                // Owner Rights is a well-known SID; no account lookup or ACL edits.
                let private = LocalDescriptor::parse(c"D:P(A;;FA;;;OW)").unwrap();
                create_new(&target, private.0)
                    .unwrap()
                    .write_all(b"fixture")
                    .unwrap();
            } else {
                std::fs::write(&target, b"fixture").unwrap();
            }
            let before = target_access(&target).unwrap().unwrap();
            if protected {
                assert!(before.protected().unwrap());
            }
            write_checked(
                &target,
                Inspect {
                    directory: root.path(),
                    expected: &before,
                },
                || Ok(()),
            )
            .unwrap();
            assert_eq!(target_access(&target).unwrap(), Some(before));
            assert_eq!(std::fs::read(target).unwrap(), b"synthetic replacement");
        }
    }

    #[test]
    fn incompatible_explicit_access_fails_without_staging_payload_or_changing_original() {
        use windows_sys::Win32::{
            Security::{Authorization::SetSecurityInfo, UNPROTECTED_DACL_SECURITY_INFORMATION},
            Storage::FileSystem::WRITE_DAC,
        };
        struct UnusedPayload;
        impl AsRef<[u8]> for UnusedPayload {
            fn as_ref(&self) -> &[u8] {
                panic!("must verify access before requesting payload bytes")
            }
        }
        let root = tempfile::tempdir_in(".").unwrap();
        let directory = crate::infra::fs::PrivateTempDir::create(root.path(), "negative").unwrap();
        let target = directory.path().join("document");
        let mut fixture = File::options()
            .write(true)
            .create_new(true)
            .access_mode(GENERIC_WRITE | WRITE_DAC)
            .open(&target)
            .unwrap();
        fixture.write_all(b"original fixture").unwrap();
        // Deny FILE_APPEND_DATA to the owner on this file only. Explicitly enable
        // inheritance: absence of P in creation SDDL is not an observed postcondition.
        let explicit = LocalDescriptor::parse(c"D:(D;;0x00000004;;;OW)(A;;FA;;;OW)").unwrap();
        let mut present = 0;
        let mut defaulted = 0;
        let mut dacl = ptr::null_mut();
        // SAFETY: parsed descriptor is live; the three out pointers are writable.
        let ok = unsafe {
            GetSecurityDescriptorDacl(
                explicit.0,
                &raw mut present,
                &raw mut dacl,
                &raw mut defaulted,
            )
        };
        assert!(
            ok != 0 && present != 0 && !dacl.is_null(),
            "expected an explicit fixture DACL"
        );
        // SAFETY: only the newly created fixture's DACL is set. The non-null ACL
        // remains owned by `explicit`; no owner, group, SACL or privilege is changed.
        let status = unsafe {
            SetSecurityInfo(
                fixture.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | UNPROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                dacl,
                ptr::null(),
            )
        };
        assert_eq!(
            status, ERROR_SUCCESS,
            "setting the fixture's unprotected DACL"
        );
        drop(fixture);

        let before = target_access(&target).unwrap().unwrap();
        assert!(
            !before.protected().unwrap(),
            "protected targets take the descriptor-copy path"
        );
        let ordinary = directory.path().join("ordinary");
        let sibling = create_new(&ordinary, ptr::null_mut()).unwrap();
        let inherited = regular_access(&sibling, &ordinary).unwrap();
        assert!(
            !same_dacl(before.as_ptr(), inherited.as_ptr()).unwrap(),
            "ordinary sibling must not inherit the target's explicit deny ACE"
        );
        assert_ne!(
            before, inherited,
            "the compared access projections must differ"
        );
        drop(sibling);
        std::fs::remove_file(ordinary).unwrap();

        let error = write_checked(&target, UnusedPayload, || Ok(())).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("cannot retain the access descriptor"),
            "{error:#}"
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"original fixture");
        assert_eq!(target_access(&target).unwrap(), Some(before));
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn descriptor_guards_copy_entry_access_and_mid_check_target_acl_change() {
        assert!(validate_self_relative_descriptor(0, SE_SELF_RELATIVE).is_err());
        assert!(validate_self_relative_descriptor(16, 0).is_err());
        assert_eq!(
            validate_self_relative_descriptor(16, SE_SELF_RELATIVE).unwrap(),
            16
        );
        assert!(check_dacl_present(0, false).is_err());
        assert!(check_dacl_present(1, true).is_err());
        check_dacl_present(1, false).unwrap();

        let root = tempfile::tempdir_in(".").unwrap();
        assert!(
            open_target(&root.path().join("bad?name")).is_err(),
            "non-NotFound open error must propagate"
        );

        let normal = root.path().join("normal");
        let private = root.path().join("private");
        std::fs::write(&normal, b"a").unwrap();
        let private_sd = LocalDescriptor::parse(c"D:P(A;;FA;;;OW)").unwrap();
        create_new(&private, private_sd.0)
            .unwrap()
            .write_all(b"b")
            .unwrap();
        verify_copy_entry_access(&normal, &normal).unwrap();
        assert!(
            verify_copy_entry_access(&normal, &private).is_err(),
            "mismatched ACLs must fail verify_copy_entry_access"
        );

        // Mutating the target's ACL during the check hook must abort publication
        // even though the staged file itself is unchanged.
        let racing = root.path().join("racing");
        std::fs::write(&racing, b"before").unwrap();
        let err = write_checked(&racing, b"after", || {
            std::fs::remove_file(&racing)?;
            create_new(&racing, private_sd.0)?.write_all(b"concurrent")?;
            Ok(())
        })
        .unwrap_err();
        assert!(
            err.to_string().contains("access changed while preparing"),
            "{err:#}"
        );
        assert_eq!(std::fs::read(&racing).unwrap(), b"concurrent");

        // matches_private_descriptor requires BOTH protected status and a matching DACL.
        let different_private = root.path().join("different-private");
        let different_sd = LocalDescriptor::parse(c"D:P(A;;FA;;;OW)(A;;FR;;;BA)").unwrap();
        create_new(&different_private, different_sd.0)
            .unwrap()
            .write_all(b"c")
            .unwrap();
        let private_access = target_access(&private).unwrap().unwrap();
        let different_access = target_access(&different_private).unwrap().unwrap();
        let normal_access = target_access(&normal).unwrap().unwrap();
        assert!(matches_private_descriptor(&private_access, private_sd.0).unwrap());
        assert!(different_access.protected().unwrap());
        assert!(!matches_private_descriptor(&different_access, private_sd.0).unwrap());
        assert!(!matches_private_descriptor(&normal_access, private_sd.0).unwrap());
    }

    #[test]
    fn windows_access_constants_include_required_bits() {
        for flag in [
            OWNER_SECURITY_INFORMATION,
            GROUP_SECURITY_INFORMATION,
            DACL_SECURITY_INFORMATION,
            LABEL_SECURITY_INFORMATION,
            ATTRIBUTE_SECURITY_INFORMATION,
            SCOPE_SECURITY_INFORMATION,
            PROCESS_TRUST_LABEL_SECURITY_INFORMATION.cast_unsigned(),
            ACCESS_FILTER_SECURITY_INFORMATION.cast_unsigned(),
        ] {
            assert_ne!(ACCESS_INFO_MASK & flag, 0, "missing mask bit {flag:#x}");
        }
        for flag in [GENERIC_WRITE, READ_CONTROL, FILE_READ_ATTRIBUTES] {
            assert_ne!(STAGED_WRITE_ACCESS & flag, 0);
        }
        for flag in [READ_CONTROL, FILE_READ_ATTRIBUTES] {
            assert_ne!(INSPECT_ACCESS & flag, 0);
        }
        for flag in [FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS] {
            assert_ne!(INSPECT_FLAGS & flag, 0);
        }
    }
}
