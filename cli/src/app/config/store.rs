//! Composition of per-domain configuration handles.
//!
//! The application layer loads the aggregate [`Config`] and then splits it into
//! one typed [`ConfigHandle`] per domain slice.  Each concrete task holds a
//! clone of exactly the handle it needs, so no task depends on the aggregate
//! configuration type. Handles remain immutable for the life of the process;
//! repository updates restart the command to publish a fresh snapshot.

use crate::app::config::Config;
use crate::domains::ai::apm::ApmFragmentSource;
use crate::domains::files::config::symlinks::{Symlink, default_target, resolve_symlinks_dir};
use crate::infra::ConfigHandle;
use std::path::Path;

macro_rules! define_config_store {
    ($($field:ident: $ty:ty => $count:expr;)+) => {
        /// Shared immutable configuration split into per-domain handles.
        ///
        /// Cloning is cheap (each field is an `Arc`-backed [`ConfigHandle`]) and
        /// all clones observe the same startup snapshots.
        #[derive(Debug, Clone)]
        pub struct ConfigStore {
            /// Whole configuration, for app-owned validation tasks.
            pub aggregate: ConfigHandle<Config>,
            /// Resolved APM fragment sources derived from managed symlinks.
            pub(crate) apm_fragments: ConfigHandle<Vec<ApmFragmentSource>>,
            $(
                #[doc = concat!("Configuration handle for `", stringify!($field), "`.")]
                pub $field: ConfigHandle<$ty>,
            )+
        }

        impl ConfigStore {
            /// Split an aggregate [`Config`] into per-domain handles.
            #[must_use]
            pub fn from_config(config: Config) -> Self {
                let apm_fragments = apm_fragment_sources(&config);
                Self {
                    $($field: ConfigHandle::new(config.$field.clone()),)+
                    apm_fragments: ConfigHandle::new(apm_fragments),
                    aggregate: ConfigHandle::new(config),
                }
            }
        }
    };
}

config_section_inventory!(define_config_store);

fn apm_fragment_sources(config: &Config) -> Vec<ApmFragmentSource> {
    config
        .symlinks
        .iter()
        .filter_map(|symlink| {
            let target_name = apm_fragment_target_name(symlink)?;
            let source = resolve_symlinks_dir(symlink, &config.root).join(&symlink.source);
            Some(ApmFragmentSource::new(source, target_name))
        })
        .collect()
}

fn apm_fragment_target_name(symlink: &Symlink) -> Option<std::ffi::OsString> {
    let target = symlink
        .target
        .clone()
        .unwrap_or_else(|| default_target(&symlink.source));
    let mut segments = Path::new(&target).iter().filter(|segment| *segment != ".");
    if segments.next()? != ".apm" || segments.next()? != "config" {
        return None;
    }
    let filename = segments.next()?;
    if segments.next().is_some() {
        return None;
    }
    let path = Path::new(filename);
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("yml") || extension.eq_ignore_ascii_case("yaml")
        })
        .then(|| path.as_os_str().to_os_string())
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    reason = "test code uses direct indexing for focused assertions"
)]
mod tests {
    use super::*;
    use crate::domains::files::config::symlinks::Symlink;
    use crate::test_helpers::empty_config;
    use std::path::PathBuf;

    fn symlink(root: &Path, source: &str) -> Symlink {
        Symlink {
            source: source.to_string(),
            target: None,
            origin: Some(root.to_path_buf()),
        }
    }

    #[test]
    fn derives_apm_fragments_from_managed_symlinks() {
        let root = PathBuf::from("/repo");
        let mut config = empty_config(root.clone());
        config.symlinks = vec![
            symlink(&root, "apm/config/base.yml"),
            symlink(&root, "apm/plugins/dot-agent"),
        ];

        let store = ConfigStore::from_config(config);

        assert_eq!(
            *store.apm_fragments.read(),
            vec![ApmFragmentSource::new(
                root.join("symlinks").join("apm/config/base.yml"),
                "base.yml".into(),
            )]
        );
    }

    #[test]
    fn fragment_sources_follow_normalized_managed_targets() {
        let root = PathBuf::from("fixture-root");
        for (source, target, expected) in [
            ("./apm/config/base.yml", None, Some("base.yml")),
            ("apm/./config/base.yaml", None, Some("base.yaml")),
            (
                "fragment.yml",
                Some("./.apm/config/base.yml"),
                Some("base.yml"),
            ),
            (
                "fragment.yml",
                Some(".apm/./config//base.yml"),
                Some("base.yml"),
            ),
            ("fragment.yml", Some(".apm/config/nested/base.yml"), None),
            ("fragment.yml", Some("../.apm/config/base.yml"), None),
            ("fragment.yml", Some(".apm/config/base.json"), None),
        ] {
            let mut config = empty_config(root.clone());
            config.symlinks = vec![Symlink {
                source: source.into(),
                target: target.map(str::to_owned),
                origin: Some(root.clone()),
            }];
            let store = ConfigStore::from_config(config);
            let expected = expected
                .map(|name| ApmFragmentSource::new(root.join("symlinks").join(source), name.into()))
                .into_iter()
                .collect::<Vec<_>>();
            assert_eq!(
                *store.apm_fragments.read(),
                expected,
                "{source:?} -> {target:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn backslashes_in_unix_targets_are_not_apm_directory_separators() {
        let mut config = empty_config("fixture-root".into());
        config.symlinks = vec![Symlink {
            source: "fragment.yml".into(),
            target: Some(r".apm\config\base.yml".into()),
            origin: None,
        }];

        assert!(
            ConfigStore::from_config(config)
                .apm_fragments
                .read()
                .is_empty()
        );
    }

    #[cfg(windows)]
    #[test]
    fn backslashes_in_windows_targets_are_apm_directory_separators() {
        for (source, target) in [
            (r".\apm\config\base.yml", None),
            ("fragment.yml", Some(r".\.apm\config\base.yml")),
        ] {
            let entry = Symlink {
                source: source.into(),
                target: target.map(str::to_owned),
                origin: None,
            };
            assert_eq!(
                apm_fragment_target_name(&entry),
                Some("base.yml".into()),
                "{source:?} -> {target:?}"
            );
        }
    }
}
