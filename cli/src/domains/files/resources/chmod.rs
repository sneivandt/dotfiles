//! File permission resource (chmod).

mod targets;
#[cfg(unix)]
mod tree;

use anyhow::Result;
use std::path::PathBuf;

use crate::domains::files::config::chmod::{OctalMode, target_path, validate_path};
use crate::engine::resource::ResourceError;
use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};

#[cfg(all(test, unix))]
pub(super) use targets::exclusion_target_with;
use targets::{
    check_resolved_mode_conflicts, error_is_permission_denied, exclusion_target,
    resolve_declared_targets,
};
#[cfg(unix)]
pub(super) use tree::{
    MODE_BITS_MASK, RecoveredDirectory, ensure_dir_execute_bits, permission_tree_reason,
    restore_directory_access, set_permissions, strip_file_execute_bits,
};
#[cfg(unix)]
use tree::{apply_recursive, check_dir_recursive, needs_directory_access};

/// Resolved destinations whose declared permission modes were validated as conflict-free.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ValidatedDeclaredModes {
    pub(super) resolved: Vec<Option<PathBuf>>,
}

/// A file permission resource that can be checked and applied (Unix only).
#[derive(Debug, Clone)]
pub(crate) struct ChmodResource {
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
    pub(crate) const fn new(target: PathBuf, mode: OctalMode) -> Self {
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
    pub(crate) fn from_entry(
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
    pub(crate) fn with_source_roots(mut self, roots: &[PathBuf]) -> Self {
        self.source_roots = roots.to_vec();
        self
    }

    /// Leave explicitly configured descendants to their own permission resources.
    #[must_use]
    pub(crate) fn excluding(mut self, targets: &[PathBuf]) -> Self {
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
