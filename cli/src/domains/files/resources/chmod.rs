//! File permission resource (chmod).
#[cfg(unix)]
use anyhow::Context as _;
use anyhow::Result;
use std::path::PathBuf;

use crate::domains::files::config::chmod::{OctalMode, target_path, validate_path};
use crate::engine::resource::ResourceError;
use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};

/// Unix file permission mask (all permission bits).
#[cfg(unix)]
pub(super) const MODE_BITS_MASK: u32 = 0o7777;

/// Resolved destinations whose declared permission modes were validated as conflict-free.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ValidatedDeclaredModes {
    resolved: Vec<Option<PathBuf>>,
}

/// A file permission resource that can be checked and applied (Unix only).
#[derive(Debug, Clone)]
pub struct ChmodResource {
    /// Target file path (absolute).
    pub target: PathBuf,
    pub(super) mode: Result<OctalMode, String>,
    home: Option<PathBuf>,
    validation_error: Option<String>,
    excluded_targets: Vec<PathBuf>,
    declared_modes: Vec<(PathBuf, u32)>,
    modes_validated: Option<std::sync::Arc<std::sync::Mutex<ValidatedDeclaredModes>>>,
    source_roots: Vec<PathBuf>,
    allow_external_target: bool,
}

impl ChmodResource {
    /// Create a new chmod resource.
    #[must_use]
    #[cfg(test)]
    pub const fn new(target: PathBuf, mode: OctalMode) -> Self {
        Self {
            target,
            mode: Ok(mode),
            home: None,
            validation_error: None,
            excluded_targets: Vec::new(),
            declared_modes: Vec::new(),
            modes_validated: None,
            source_roots: Vec::new(),
            allow_external_target: false,
        }
    }

    /// Create from a config entry and home directory.
    ///
    pub fn from_entry(
        entry: &crate::domains::files::config::chmod::ChmodEntry,
        home: &std::path::Path,
    ) -> Self {
        let target = home.join(target_path(&entry.path));
        Self {
            target,
            mode: entry.parsed_mode().clone(),
            home: Some(home.to_path_buf()),
            validation_error: validate_path(&entry.path).err(),
            excluded_targets: Vec::new(),
            declared_modes: Vec::new(),
            modes_validated: None,
            source_roots: Vec::new(),
            allow_external_target: entry.allow_external_target,
        }
    }

    /// Permit links into the public and overlay managed source trees.
    #[must_use]
    pub fn with_source_roots(mut self, roots: &[PathBuf]) -> Self {
        self.source_roots = roots.to_vec();
        self
    }

    /// Leave explicitly configured descendants to their own permission resources.
    #[must_use]
    pub fn excluding(mut self, targets: &[PathBuf]) -> Self {
        // Traversal checks only child entries. The root itself and unrelated
        // configured destinations cannot match those entries.
        self.excluded_targets = targets.to_vec();
        self
    }

    #[must_use]
    pub(crate) fn with_declared_modes(mut self, modes: &[(PathBuf, u32)]) -> Self {
        self.declared_modes = modes.to_vec();
        self
    }

    #[must_use]
    pub(crate) fn with_modes_validated(
        mut self,
        validated: std::sync::Arc<std::sync::Mutex<ValidatedDeclaredModes>>,
    ) -> Self {
        self.modes_validated = Some(validated);
        self
    }

    fn ensure_declared_modes_valid(&self) -> Result<bool> {
        if self.declared_modes.is_empty() {
            return Ok(false);
        }
        let current = resolve_declared_targets(&self.declared_modes)?;
        if let Some(validated) = &self.modes_validated {
            let mut guard = validated
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if guard.resolved == current {
                return Ok(false);
            }
            check_resolved_mode_conflicts(&self.declared_modes, &current)?;
            guard.resolved = current;
            return Ok(true);
        }
        check_resolved_mode_conflicts(&self.declared_modes, &current)?;
        Ok(true)
    }

    pub(crate) fn validate_declared_modes(
        modes: &[(PathBuf, u32)],
    ) -> Result<ValidatedDeclaredModes> {
        let resolved = resolve_declared_targets(modes)?;
        check_resolved_mode_conflicts(modes, &resolved)?;
        Ok(ValidatedDeclaredModes { resolved })
    }

    pub(crate) fn ordering_target(&self) -> Result<PathBuf> {
        exclusion_target(&self.target)
    }

    #[cfg(unix)]
    fn resolved_exclusions(&self) -> ResourceResult<std::collections::HashSet<PathBuf>> {
        let mut excluded = std::collections::HashSet::with_capacity(self.excluded_targets.len());
        for target in &self.excluded_targets {
            excluded.insert(exclusion_target(target)?);
        }
        Ok(excluded)
    }

    fn invalid_path_reason(&self) -> ResourceResult<Option<String>> {
        if let Some(reason) = &self.validation_error {
            return Ok(Some(reason.clone()));
        }
        if let Some(home) = &self.home
            && self.target.try_exists()?
        {
            let target = crate::infra::fs::canonicalize(&self.target)?;
            let home = crate::infra::fs::canonicalize(home)?;
            if target == home {
                return Ok(Some("chmod target resolves to $HOME itself".into()));
            }
            let managed = self
                .source_roots
                .iter()
                .map(|root| {
                    crate::infra::fs::resolve_directory(root).map(|root| target.starts_with(root))
                })
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .any(|allowed| allowed);
            if !target.starts_with(home) && !managed && !self.allow_external_target {
                return Ok(Some(format!(
                    "chmod target resolves outside home and managed sources to {}; set allow_external_target = true to permit it",
                    target.display()
                )));
            }
        }
        Ok(None)
    }
}

impl Resource for ChmodResource {
    fn description(&self) -> String {
        match &self.mode {
            Ok(mode) => {
                let target = self.target.display();
                match std::fs::canonicalize(&self.target) {
                    Ok(resolved) if resolved != self.target && self.home.is_some() => {
                        format!("{mode} {target} (resolved {})", resolved.display())
                    }
                    _ => format!("{mode} {target}"),
                }
            }
            Err(reason) => format!(
                "invalid chmod mode ({reason}) for {}",
                self.target.display()
            ),
        }
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        if let Some(reason) = self.invalid_path_reason()? {
            return Err(ResourceError::conflicting_state(
                self.target.display().to_string(),
                "a file or directory beneath $HOME",
                reason,
            ));
        }
        self.ensure_declared_modes_valid()?;
        #[cfg(unix)]
        {
            let mode = self.mode.as_ref().map_err(|reason| {
                ResourceError::conflicting_state(
                    self.target.display().to_string(),
                    "valid octal mode",
                    reason,
                )
            })?;
            let mode = mode.as_u32();
            let target = crate::infra::fs::canonicalize(&self.target)?;
            let dir_mode = ensure_dir_execute_bits(mode);
            let mut recovered = Vec::new();
            let excluded = std::cell::RefCell::new(self.resolved_exclusions()?);
            let result = (|| {
                let reason = permission_tree_reason(
                    &target,
                    &|path| Ok(excluded.borrow().contains(path)),
                    &mut |path, metadata| {
                        use std::os::unix::fs::PermissionsExt as _;
                        let current = metadata.permissions().mode() & MODE_BITS_MASK;
                        if needs_directory_access(current) {
                            recovered.push(RecoveredDirectory::acquire(path, current)?);
                            // Refresh exclusions only after recovering an inaccessible directory.
                            *excluded.borrow_mut() = self.resolved_exclusions()?;
                        }
                        Ok(true)
                    },
                )?;
                if let Some(reason) = reason {
                    return Err(ResourceError::conflicting_state(
                        target.display().to_string(),
                        "a permission tree without hard-linked files",
                        reason,
                    ));
                }
                let modes_changed = self.ensure_declared_modes_valid()?;
                let current_target = crate::infra::fs::canonicalize(&self.target)?;
                if current_target != target {
                    return Err(ResourceError::conflicting_state(
                        self.target.display().to_string(),
                        target.display().to_string(),
                        format!(
                            "chmod target changed during apply to {}",
                            current_target.display()
                        ),
                    ));
                }
                if modes_changed {
                    *excluded.borrow_mut() = self.resolved_exclusions()?;
                }
                if target.is_dir() {
                    let excluded_ref = excluded.borrow();
                    let changed = apply_recursive(
                        &target,
                        dir_mode,
                        strip_file_execute_bits(mode),
                        &|path| Ok(excluded_ref.contains(path)),
                        &mut recovered,
                    )?;
                    if !changed {
                        return Ok(ResourceChange::AlreadyCorrect);
                    }
                } else {
                    set_permissions(&target, mode)?;
                }
                Ok(ResourceChange::Applied)
            })();

            match result {
                Err(error) => match restore_directory_access(recovered.iter()) {
                    Ok(()) => Err(error),
                    Err(restore_error) => {
                        Err(anyhow::Error::from(error).context(restore_error).into())
                    }
                },
                Ok(change) => {
                    restore_directory_access(
                        recovered.iter().filter(|recovery| !recovery.finalized),
                    )?;
                    Ok(change)
                }
            }
        }

        #[cfg(not(unix))]
        {
            Ok(ResourceChange::skipped(
                "chmod not supported on this platform",
            ))
        }
    }
}

impl IntrinsicState for ChmodResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        let invalid_reason = match self.invalid_path_reason() {
            Err(error) if error_is_permission_denied(&error) => {
                return Ok(ResourceState::Unknown {
                    reason: format!(
                        "cannot validate {} until ancestor permissions allow access",
                        self.target.display()
                    ),
                });
            }
            result => result?,
        };
        if let Some(reason) = invalid_reason {
            return Ok(ResourceState::Invalid { reason });
        }
        self.ensure_declared_modes_valid()?;
        if let Err(reason) = &self.mode {
            return Ok(ResourceState::Invalid {
                reason: reason.clone(),
            });
        }
        if !self.target.try_exists()? {
            return Ok(ResourceState::Missing);
        }

        // Get current mode (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = self
                .mode
                .as_ref()
                .map_err(|reason| anyhow::anyhow!("{reason}"))?;
            let desired_mode = mode.as_u32();
            let target = crate::infra::fs::canonicalize(&self.target)?;
            let excluded = self.resolved_exclusions()?;
            let is_excluded = |path: &std::path::Path| Ok(excluded.contains(path));
            if let Some(reason) =
                permission_tree_reason(&target, &is_excluded, &mut |_, metadata| {
                    // Inaccessible, incorrect directories are inspected during apply,
                    // after temporarily restoring owner access, never during discovery.
                    Ok(!needs_directory_access(
                        metadata.permissions().mode() & MODE_BITS_MASK,
                    ))
                })?
            {
                return Ok(ResourceState::Invalid { reason });
            }
            if self.target.is_dir() {
                check_dir_recursive(&target, desired_mode, &is_excluded)
            } else {
                let current_mode =
                    std::fs::metadata(&self.target)?.permissions().mode() & MODE_BITS_MASK;
                if current_mode == desired_mode {
                    Ok(ResourceState::Correct)
                } else {
                    Ok(ResourceState::Incorrect {
                        current: format!("{current_mode:o}"),
                    })
                }
            }
        }
        #[cfg(not(unix))]
        {
            Ok(ResourceState::Invalid {
                reason: "chmod not supported on this platform".to_string(),
            })
        }
    }
}

fn error_is_permission_denied(error: &(dyn std::error::Error + 'static)) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::PermissionDenied)
        || error.source().is_some_and(error_is_permission_denied)
}

fn resolve_declared_targets(modes: &[(PathBuf, u32)]) -> Result<Vec<Option<PathBuf>>> {
    modes
        .iter()
        .map(|(path, _)| resolve_declared_target(path))
        .collect()
}

fn check_resolved_mode_conflicts(
    modes: &[(PathBuf, u32)],
    resolved: &[Option<PathBuf>],
) -> Result<()> {
    let mut seen = std::collections::HashMap::new();
    for ((path, mode), target) in modes.iter().zip(resolved) {
        if let Some(target) = target
            && let Some((previous_mode, previous_path)) = seen.insert(target.clone(), (mode, path))
        {
            anyhow::ensure!(
                previous_mode == mode,
                "conflicting permission modes for {} and {} resolving to {}; keep one mode for this destination",
                previous_path.display(),
                path.display(),
                target.display()
            );
        }
    }
    Ok(())
}

fn resolve_declared_target(path: &std::path::Path) -> Result<Option<PathBuf>> {
    let resolved = (|| {
        if path.try_exists()? {
            crate::infra::fs::canonicalize(path)
        } else {
            crate::infra::fs::resolve_target_entry(path)
        }
    })();
    match resolved {
        Err(error) if error_is_permission_denied(error.as_ref()) => Ok(None),
        result => result.map(Some),
    }
}

// An unresolved suffix is only an exclusion/order hint, never permission to
// mutate a destination. Each resource still validates its fully resolved target.
fn exclusion_target(path: &std::path::Path) -> Result<PathBuf> {
    exclusion_target_with(path, crate::infra::fs::canonicalize)
}

fn exclusion_target_with(
    path: &std::path::Path,
    canonicalize: impl Fn(&std::path::Path) -> Result<PathBuf>,
) -> Result<PathBuf> {
    if let Some(target) = resolve_declared_target(path)? {
        return Ok(target);
    }
    for ancestor in path.ancestors().skip(1) {
        match canonicalize(ancestor) {
            Ok(resolved) => return Ok(resolved.join(path.strip_prefix(ancestor)?)),
            Err(error) if error_is_permission_denied(error.as_ref()) => {}
            Err(error) => return Err(error),
        }
    }
    anyhow::bail!("cannot resolve chmod exclusion {}", path.display())
}

#[cfg(unix)]
struct RecoveredDirectory {
    path: PathBuf,
    original: u32,
    directory: std::fs::File,
    finalized: bool,
}

#[cfg(unix)]
impl RecoveredDirectory {
    fn acquire(path: &std::path::Path, original: u32) -> ResourceResult<Self> {
        use rustix::fs::{Mode, OFlags};

        set_permissions(path, original | 0o500)?;
        let flags = OFlags::RDONLY
            .union(OFlags::CLOEXEC)
            .union(OFlags::DIRECTORY)
            .union(OFlags::NOFOLLOW);
        let opened = rustix::fs::open(path, flags, Mode::empty())
            .map(std::fs::File::from)
            .map_err(std::io::Error::from);
        match opened {
            Ok(directory) => Ok(Self {
                path: path.to_path_buf(),
                original,
                directory,
                finalized: false,
            }),
            Err(error) => {
                let error = anyhow::Error::new(error)
                    .context(format!("open recovered directory {}", path.display()));
                match set_permissions(path, original) {
                    Ok(()) => Err(error.into()),
                    Err(restore_error) => Err(error
                        .context(format!(
                            "could not restore temporary directory access on {}: {restore_error}",
                            path.display()
                        ))
                        .into()),
                }
            }
        }
    }
}

#[cfg(unix)]
fn restore_directory_access<'a>(
    recovered: impl DoubleEndedIterator<Item = &'a RecoveredDirectory>,
) -> ResourceResult<()> {
    use std::os::unix::fs::PermissionsExt as _;

    let mut failures = Vec::new();
    for recovery in recovered.rev() {
        // A completed restrictive ancestor may no longer be traversable.
        // Restore the original directory itself, without resolving its path again.
        if let Err(error) = recovery
            .directory
            .set_permissions(std::fs::Permissions::from_mode(recovery.original))
        {
            failures.push(format!("{}: {error}", recovery.path.display()));
        }
    }
    if !failures.is_empty() {
        return Err(anyhow::anyhow!(
            "could not restore temporary directory access: {}",
            failures.join("; ")
        )
        .into());
    }
    Ok(())
}

/// Recursively check a directory and its contents against the desired mode.
///
/// Directories are compared with execute bits added (via [`ensure_dir_execute_bits`]),
/// files are compared with execute bits stripped (via [`strip_file_execute_bits`]),
/// matching the logic in [`apply_recursive`].
#[cfg(unix)]
fn check_dir_recursive(
    path: &std::path::Path,
    base_mode: u32,
    is_excluded: &impl Fn(&std::path::Path) -> ResourceResult<bool>,
) -> ResourceResult<ResourceState> {
    use std::os::unix::fs::PermissionsExt;

    let dir_mode = ensure_dir_execute_bits(base_mode);
    let file_mode = strip_file_execute_bits(base_mode);

    let current_mode = std::fs::metadata(path)?.permissions().mode() & MODE_BITS_MASK;
    if current_mode != dir_mode {
        return Ok(ResourceState::Incorrect {
            current: format!("directory {} has mode {current_mode:o}", path.display()),
        });
    }
    if needs_directory_access(current_mode) {
        return Ok(ResourceState::Incorrect {
            current: format!(
                "directory {} needs temporary owner access to inspect contents",
                path.display()
            ),
        });
    }

    let entries =
        std::fs::read_dir(path).with_context(|| format!("reading directory {}", path.display()))?;

    for entry in entries {
        let entry = entry.with_context(|| format!("reading entry in {}", path.display()))?;
        let entry_path = entry.path();

        let kind = entry.file_type()?;
        if !(kind.is_dir() || kind.is_file()) || is_excluded(&entry_path)? {
            continue;
        }

        if kind.is_dir() {
            if let recursive_state @ ResourceState::Incorrect { .. } =
                check_dir_recursive(&entry_path, base_mode, is_excluded)?
            {
                return Ok(recursive_state);
            }
        } else {
            let entry_mode = std::fs::metadata(&entry_path)?.permissions().mode() & MODE_BITS_MASK;
            if entry_mode != file_mode {
                return Ok(ResourceState::Incorrect {
                    current: format!("file {} has mode {entry_mode:o}", entry_path.display()),
                });
            }
        }
    }

    Ok(ResourceState::Correct)
}

#[cfg(unix)]
fn apply_recursive(
    path: &std::path::Path,
    dir_mode: u32,
    file_mode: u32,
    is_excluded: &impl Fn(&std::path::Path) -> ResourceResult<bool>,
    recovered: &mut [RecoveredDirectory],
) -> ResourceResult<bool> {
    use std::os::unix::fs::PermissionsExt as _;

    let mut changed = false;
    if path.is_dir() {
        for entry in std::fs::read_dir(path)
            .with_context(|| format!("reading directory {}", path.display()))?
        {
            let entry = entry.with_context(|| format!("reading entry in {}", path.display()))?;
            let entry_path = entry.path();
            let kind = entry.file_type()?;
            if !(kind.is_dir() || kind.is_file()) || is_excluded(&entry_path)? {
                continue;
            }
            if kind.is_dir() {
                changed |=
                    apply_recursive(&entry_path, dir_mode, file_mode, is_excluded, recovered)?;
            } else {
                let current_mode =
                    std::fs::metadata(&entry_path)?.permissions().mode() & MODE_BITS_MASK;
                if current_mode != file_mode {
                    changed = true;
                    set_permissions(&entry_path, file_mode)?;
                }
            }
        }
    }

    // Keep directories traversable until every descendant has been processed.
    let effective_mode = if path.is_dir() { dir_mode } else { file_mode };
    let recovery = recovered.iter_mut().find(|recovery| recovery.path == path);
    let (current, was_recovered) = match &recovery {
        Some(rec) => (rec.original, true),
        None => (
            std::fs::metadata(path)?.permissions().mode() & MODE_BITS_MASK,
            false,
        ),
    };
    if current != effective_mode {
        changed = true;
    }
    if current != effective_mode || was_recovered {
        set_permissions(path, effective_mode)?;
    }
    if let Some(rec) = recovery {
        rec.finalized = true;
    }
    Ok(changed)
}

#[cfg(unix)]
fn set_permissions(path: &std::path::Path, mode: u32) -> ResourceResult<()> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.is_symlink() {
        return Err(anyhow::anyhow!("chmod target became a symlink: {}", path.display()).into());
    }
    if !metadata.is_dir() {
        crate::infra::fs::require_single_link(&metadata)?;
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(|error| {
        if error_is_permission_denied(&error) {
            ResourceError::permission_denied(path.display().to_string())
        } else {
            anyhow::Error::new(error)
                .context(format!("set permissions on {}", path.display()))
                .into()
        }
    })
}

#[cfg(unix)]
fn permission_tree_reason(
    path: &std::path::Path,
    is_excluded: &impl Fn(&std::path::Path) -> ResourceResult<bool>,
    prepare_directory: &mut impl FnMut(&std::path::Path, &std::fs::Metadata) -> ResourceResult<bool>,
) -> ResourceResult<Option<String>> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir() && !metadata.is_symlink() {
        return Ok(crate::infra::fs::require_single_link(&metadata)
            .err()
            .map(|error| format!("{}: {error}", path.display())));
    }
    if metadata.is_dir() && prepare_directory(path, &metadata)? {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let child = entry.path();
            let kind = entry.file_type()?;
            if !(kind.is_dir() || kind.is_file()) || is_excluded(&child)? {
                continue;
            }
            if let Some(reason) = permission_tree_reason(&child, is_excluded, prepare_directory)? {
                return Ok(Some(reason));
            }
        }
    }
    Ok(None)
}

#[cfg(unix)]
const fn needs_directory_access(current: u32) -> bool {
    current & 0o500 != 0o500
}

/// Add execute bits to a mode for each permission triplet that has read access.
/// This mirrors the conventional behaviour of `chmod -R`: files get the
/// specified mode, while directories get execute bits so they remain
/// traversable (e.g., mode 600 → dir mode 700).
#[cfg(unix)]
pub(super) const fn ensure_dir_execute_bits(mode: u32) -> u32 {
    mode | ((mode & 0o444) >> 2)
}

/// Strip execute bits from a mode before applying it to regular files during
/// recursive directory chmod operations, preserving the remaining permission
/// bits unchanged.
#[cfg(unix)]
pub(super) const fn strip_file_execute_bits(mode: u32) -> u32 {
    mode & !0o111
}

#[cfg(all(test, unix))]
mod mutation_tests {
    use super::*;

    #[test]
    fn exclusion_fallback_preserves_non_permission_resolution_errors() {
        use std::os::unix::fs::PermissionsExt as _;

        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let locked = root.join("locked");
        let target = locked.join("config");
        std::fs::create_dir(&locked).unwrap();
        std::fs::write(&target, "fixture").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        for kind in [
            std::io::ErrorKind::NotFound,
            std::io::ErrorKind::NotADirectory,
            std::io::ErrorKind::Other,
        ] {
            let error = exclusion_target_with(&target, |ancestor| {
                if ancestor == locked {
                    Err(std::io::Error::new(kind, "ancestor changed during resolution").into())
                } else {
                    crate::infra::fs::canonicalize(ancestor)
                }
            })
            .unwrap_err();
            assert_eq!(error.downcast_ref::<std::io::Error>().unwrap().kind(), kind);
        }
        let hint = exclusion_target_with(&target, |ancestor| {
            if ancestor == locked {
                Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied).into())
            } else {
                crate::infra::fs::canonicalize(ancestor)
            }
        })
        .unwrap();
        assert_eq!(hint, target);
        assert_eq!(
            locked.metadata().unwrap().permissions().mode() & MODE_BITS_MASK,
            0
        );
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[test]
    fn recovery_rejects_replaced_non_directories_without_changing_permissions() {
        use std::os::unix::fs::PermissionsExt as _;

        let fixture = tempfile::tempdir_in(".").unwrap();
        let file = fixture.path().join("replaced-directory");
        std::fs::write(&file, "fixture").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(
            RecoveredDirectory::acquire(&file, 0o600).is_err(),
            "recovery must reject a directory replaced by a regular file"
        );
        assert_eq!(
            file.metadata().unwrap().permissions().mode() & MODE_BITS_MASK,
            0o600
        );
    }

    #[test]
    fn recovery_restores_original_directories_after_restrictive_ancestors_and_alias_replacement() {
        use std::os::unix::fs::PermissionsExt as _;

        for replace_path in [false, true] {
            let fixture = tempfile::tempdir_in(".").unwrap();
            let root = fixture.path().join("tree");
            let branch = root.join("branch");
            let nested = branch.join("nested");
            let preserved = branch.join("preserved");
            let outside = fixture.path().join("outside");
            std::fs::create_dir_all(&nested).unwrap();
            std::fs::set_permissions(&branch, std::fs::Permissions::from_mode(0o700)).unwrap();
            std::fs::write(&outside, "unmanaged").unwrap();
            std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o600)).unwrap();
            for path in [&nested, &root] {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
            }
            let recovered = [
                RecoveredDirectory::acquire(&root, 0).unwrap(),
                RecoveredDirectory::acquire(&nested, 0).unwrap(),
            ];
            if replace_path {
                std::fs::rename(&nested, &preserved).unwrap();
                std::os::unix::fs::symlink(&outside, &nested).unwrap();
            } else {
                set_permissions(&nested, 0).unwrap();
            }
            // Model a completed restrictive branch before a later apply failure.
            set_permissions(&branch, 0).unwrap();
            restore_directory_access(recovered.iter()).unwrap();

            assert_eq!(
                root.metadata().unwrap().permissions().mode() & MODE_BITS_MASK,
                0
            );
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(
                branch.metadata().unwrap().permissions().mode() & MODE_BITS_MASK,
                0
            );
            std::fs::set_permissions(&branch, std::fs::Permissions::from_mode(0o700)).unwrap();
            let restored = if replace_path { &preserved } else { &nested };
            assert_eq!(
                restored.metadata().unwrap().permissions().mode() & MODE_BITS_MASK,
                0
            );
            std::fs::set_permissions(restored, std::fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(
                outside.metadata().unwrap().permissions().mode() & MODE_BITS_MASK,
                0o600
            );
        }
    }

    #[test]
    fn changed_final_entries_are_rejected_at_the_permission_write() {
        use std::os::unix::fs::PermissionsExt as _;
        let fixture = tempfile::tempdir().unwrap();
        let file = fixture.path().join("private");
        let alias = fixture.path().join("alias");
        std::fs::write(&file, "private").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::hard_link(&file, &alias).unwrap();
        assert!(
            permission_tree_reason(fixture.path(), &|_| Ok(false), &mut |_, _| Ok(true))
                .unwrap()
                .is_some()
        );
        assert!(
            permission_tree_reason(
                fixture.path(),
                &|path| Ok(path == file || path == alias),
                &mut |_, _| Ok(true),
            )
            .unwrap()
            .is_none()
        );
        assert!(set_permissions(&file, 0o644).is_err());
        std::fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&file, &alias).unwrap();
        assert!(set_permissions(&alias, 0o644).is_err());
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        set_permissions(&file, 0o640).unwrap();
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o640);
    }

    #[test]
    fn apply_recursive_skips_unchanged_files_and_respects_preresolved_exclusions() {
        use std::os::unix::fs::PermissionsExt as _;

        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let ok_file = root.join("already-ok");
        let drift_file = root.join("drifted");
        let excluded_file = root.join("excluded");
        std::fs::write(&ok_file, "ok").unwrap();
        std::fs::write(&drift_file, "drift").unwrap();
        std::fs::write(&excluded_file, "excluded").unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&ok_file, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::set_permissions(&drift_file, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::set_permissions(&excluded_file, std::fs::Permissions::from_mode(0o644)).unwrap();

        let resource = ChmodResource::new(root, OctalMode::parse("600").unwrap())
            .excluding(std::slice::from_ref(&excluded_file));
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        assert_eq!(
            ok_file.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            drift_file.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            excluded_file.metadata().unwrap().permissions().mode() & 0o777,
            0o644,
            "excluded descendant must remain untouched"
        );
        assert_eq!(resource.apply().unwrap(), ResourceChange::AlreadyCorrect);
    }

    #[test]
    fn retargeted_symlink_invalidates_cached_mode_validation_before_ordinary_file_chmod() {
        use std::os::unix::fs::PermissionsExt as _;

        let fixture = tempfile::tempdir_in(".").unwrap();
        let home = fixture.path().canonicalize().unwrap();
        let other = home.join(".other");
        let file = home.join(".file");
        let link = home.join(".link");
        std::fs::write(&other, "other").unwrap();
        std::fs::write(&file, "secret").unwrap();
        std::fs::set_permissions(&other, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::os::unix::fs::symlink(&other, &link).unwrap();

        let modes = vec![(link.clone(), 0o600), (file.clone(), 0o644)];
        let validated = ChmodResource::validate_declared_modes(&modes).unwrap();
        let shared = std::sync::Arc::new(std::sync::Mutex::new(validated));
        let resource = ChmodResource::new(file.clone(), OctalMode::parse("644").unwrap())
            .with_declared_modes(&modes)
            .with_modes_validated(std::sync::Arc::clone(&shared));

        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Incorrect { .. }
        ));

        // Retarget the symlink declaration to alias the ordinary file after
        // initial validation succeeded.
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&file, &link).unwrap();

        let apply_err = resource.apply().unwrap_err();
        assert!(
            format!("{apply_err:#}").contains("conflicting permission modes"),
            "{apply_err:#}"
        );
        assert_eq!(
            file.metadata().unwrap().permissions().mode() & 0o777,
            0o600,
            "stricter file mode must be preserved when a retargeted symlink creates a conflict"
        );
        let state_err = resource.current_state().unwrap_err();
        assert!(
            format!("{state_err:#}").contains("conflicting permission modes"),
            "{state_err:#}"
        );

        // Restoring the non-conflicting symlink target lets subsequent apply proceed.
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&other, &link).unwrap();
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o644);
    }

    #[test]
    fn validate_declared_modes_records_resolved_targets_and_rejects_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("secret");
        std::fs::write(&file, b"x").unwrap();
        let validated = ChmodResource::validate_declared_modes(&[(file.clone(), 0o600)]).unwrap();
        assert_eq!(validated.resolved.len(), 1);
        assert!(validated.resolved[0].is_some());

        let link = dir.path().join("alias");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        let err =
            ChmodResource::validate_declared_modes(&[(file, 0o600), (link, 0o644)]).unwrap_err();
        assert!(
            format!("{err:#}").contains("conflicting permission modes"),
            "{err:#}"
        );
    }
}
