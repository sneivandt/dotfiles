//! Declarative configuration for privileged files below `/etc`.

use std::path::{Component, Path, PathBuf};

use anyhow::{Result, bail};
use serde::{Deserialize, Deserializer};

use crate::infra::config::config_section;

/// How a tracked source fragment is combined with an existing target.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MergeStrategy {
    /// Recursively merge a TOML table, preserving unmanaged keys.
    Toml,
    /// Merge INI keys by section, preserving unmanaged keys and sections.
    Ini,
    /// Insert PAM module rules at the end of their matching facility stacks.
    Pam,
}

/// One tracked source fragment and its privileged target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemFile {
    /// Absolute destination below `/etc`, inferred from `source` when omitted.
    pub target: PathBuf,
    /// Relative path below the declaring repository's `system/` directory.
    pub source: PathBuf,
    /// Content-aware merge behavior.
    pub merge: MergeStrategy,
    /// Repository root that declared this entry.
    pub(crate) origin: Option<PathBuf>,
}

impl<'de> Deserialize<'de> for SystemFile {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Entry {
            target: Option<PathBuf>,
            source: PathBuf,
            merge: MergeStrategy,
        }

        let entry = Entry::deserialize(deserializer)?;
        let target = entry
            .target
            .unwrap_or_else(|| Path::new("/etc").join(&entry.source));
        Ok(Self {
            target,
            source: entry.source,
            merge: entry.merge,
            origin: None,
        })
    }
}

impl SystemFile {
    /// Resolve the tracked fragment path.
    #[must_use]
    pub fn source_path(&self) -> PathBuf {
        self.origin
            .as_deref()
            .unwrap_or_else(|| Path::new("."))
            .join("system")
            .join(&self.source)
    }
}

config_section! {
    field: "files",
    ty: SystemFile,
}

/// Load every configured entry without applying category selectors.
pub(crate) fn load_all(path: &Path) -> Result<Vec<SystemFile>> {
    crate::infra::config::toml_loader::load_section_unfiltered::<Section>(path)
}

/// TOML filename that backs this config section.
pub(crate) const SYSTEM_FILES_TOML: &str = "system-files.toml";

/// Attach the repository root that owns a batch of entries.
pub(crate) fn set_origin(files: &mut [SystemFile], root: &Path) {
    for file in files {
        file.origin = Some(root.to_path_buf());
    }
}

/// Reject unsafe paths and duplicate targets before tasks are constructed.
pub(crate) fn validate_entries(files: &[SystemFile]) -> Result<()> {
    let mut targets = std::collections::BTreeSet::new();
    for file in files {
        if !file.target.is_absolute()
            || !file.target.starts_with("/etc")
            || file.target == Path::new("/etc")
            || file
                .target
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            bail!(
                "system file target {} must be an absolute path below /etc without '..' components",
                file.target.display()
            );
        }
        if file.source.as_os_str().is_empty()
            || file.source.is_absolute()
            || file
                .source
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            bail!(
                "system file source {} must stay below the system directory",
                file.source.display()
            );
        }
        if !targets.insert(file.target.clone()) {
            bail!(
                "system file target {} is configured more than once",
                file.target.display()
            );
        }
    }
    Ok(())
}

/// Validate tracked fragment paths for the repository check command.
#[must_use]
pub fn validate(files: &[SystemFile]) -> Vec<crate::infra::config::Diagnostic> {
    use crate::infra::config::{Diagnostic, DiagnosticCode};

    const MISSING_SOURCE: DiagnosticCode = DiagnosticCode::new("system-file", "missing-source");
    files
        .iter()
        .filter_map(|file| {
            let source = file.source_path();
            (!source.is_file()).then(|| {
                Diagnostic::error(
                    SYSTEM_FILES_TOML,
                    file.target.display().to_string(),
                    MISSING_SOURCE,
                    format!("tracked fragment {} does not exist", source.display()),
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::config::category_matcher::Category;
    use crate::infra::config::test_helpers::{assert_load_rejects, write_temp_toml};

    #[test]
    fn filters_profile_and_environment_sections() {
        let (_dir, path) = write_temp_toml(
            "[linux]\nfiles = [{ target = '/etc/base', source = 'base', merge = 'toml' }]\n\
             [arch-desktop]\nfiles = [{ target = '/etc/desktop', source = 'desktop', merge = 'pam' }]\n\
             [wsl]\nfiles = [{ target = '/etc/wsl.conf', source = 'wsl.conf', merge = 'ini' }]\n",
        );
        let files = load(&path, &[Category::Linux, Category::Arch, Category::Desktop]).unwrap();
        assert_eq!(files.len(), 2);
        assert!(
            files
                .iter()
                .any(|file| file.target == Path::new("/etc/base"))
        );
        assert!(
            files
                .iter()
                .any(|file| file.target == Path::new("/etc/desktop"))
        );
    }

    #[test]
    fn rejects_unknown_entry_keys() {
        assert_load_rejects(
            load,
            "[base]\nfiles = [{ target = '/etc/x', source = 'x', merge = 'toml', typo = true }]\n",
            "typo",
        );
    }

    #[test]
    fn infers_target_from_source_when_omitted() {
        let (_dir, path) = write_temp_toml(
            "[base]\nfiles = [\
             { source = 'codex/requirements.toml', merge = 'toml' }, \
             { target = '/etc/custom.toml', source = 'source.toml', merge = 'toml' }\
             ]\n",
        );

        let files = load_all(&path).unwrap();
        assert_eq!(
            files[0].target,
            PathBuf::from("/etc/codex/requirements.toml")
        );
        assert_eq!(files[1].target, PathBuf::from("/etc/custom.toml"));
    }

    #[cfg(unix)]
    #[test]
    fn validation_rejects_unsafe_and_duplicate_paths() {
        let file = |target: &str, source: &str| SystemFile {
            target: PathBuf::from(target),
            source: PathBuf::from(source),
            merge: MergeStrategy::Toml,
            origin: None,
        };
        for target in [
            "relative",
            "/etc",
            "/etc/.",
            "/etc-other/x",
            "/home/user/x",
            "/etc/../home/user/x",
            "/etc/./../home/user/x",
            "/etc/sub/../../home/user/x",
            "/etc/sub/../x",
        ] {
            let error = validate_entries(&[file(target, "x")]).unwrap_err();
            assert!(error.to_string().contains("target"), "{target}: {error}");
            assert!(error.to_string().contains("below /etc"), "{error}");
        }
        for source in ["", "/absolute", "../x", "./../x", "sub/../../x"] {
            let error = validate_entries(&[file("/etc/x", source)]).unwrap_err();
            assert!(error.to_string().contains("source"), "{source}: {error}");
        }
        for target in ["/etc/x", "/etc/sub/x", "/etc/./sub/x"] {
            validate_entries(&[file(target, "sub/x")]).unwrap();
        }
        assert!(validate_entries(&[file("/etc/x", "x"), file("/etc/x", "y")]).is_err());
        assert!(validate_entries(&[file("/etc/sub/x", "x"), file("/etc/./sub/x", "y")]).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn validation_does_not_treat_windows_targets_as_linux_etc_paths() {
        for target in ["/etc/x", r"C:\etc\x", r"C:\etc\..\home\x", r"\\host\etc\x"] {
            let file = SystemFile {
                target: PathBuf::from(target),
                source: PathBuf::from("x"),
                merge: MergeStrategy::Toml,
                origin: None,
            };
            assert!(validate_entries(&[file]).is_err(), "{target}");
        }
    }
}
