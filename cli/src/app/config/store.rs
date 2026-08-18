//! Composition of per-domain configuration handles.
//!
//! The application layer loads the aggregate [`Config`] and then splits it into
//! one typed [`ConfigHandle`] per domain slice.  Each concrete task holds a
//! clone of exactly the handle it needs, so no task depends on the aggregate
//! configuration type. During an app-owned reload the store swaps each
//! reloadable handle in place, and because tasks share those handles the update
//! is visible without rebuilding static tasks. Dynamic overlay script tasks are
//! rebuilt after the reload discovery boundary from their freshly swapped
//! handle.

use crate::app::config::Config;
use crate::domains::ai::apm::ApmFragmentSource;
use crate::domains::files::config::symlinks::{Symlink, resolve_symlinks_dir};
use crate::infra::ConfigHandle;
use crate::infra::config::ConfigSource;
use std::path::Path;

macro_rules! define_config_store {
    ($($field:ident: $ty:ty => $count:expr;)+) => {
        /// Shared, atomically-swappable configuration split into per-domain handles.
        ///
        /// Cloning is cheap (each field is an `Arc`-backed [`ConfigHandle`]) and
        /// all clones observe the same slots.
        #[derive(Debug, Clone)]
        pub struct ConfigStore {
            source: ConfigSource<PublishedConfig>,
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
                let source = ConfigSource::new(PublishedConfig::new(config));
                Self {
                    $($field: source.project(|snapshot| snapshot.config.$field.clone()),)+
                    apm_fragments: source.project(|snapshot| snapshot.apm_fragments.clone()),
                    aggregate: source.project(|snapshot| snapshot.config.clone()),
                    source,
                }
            }

            /// Replace reloadable handles from a freshly-loaded [`Config`].
            ///
            /// All projected handles switch to the new immutable generation in
            /// one publication step.
            pub fn reload(&self, config: Config) {
                self.source.swap(PublishedConfig::new(config));
            }
        }
    };
}

config_section_inventory!(define_config_store);

#[derive(Debug)]
struct PublishedConfig {
    config: Config,
    apm_fragments: Vec<ApmFragmentSource>,
}

impl PublishedConfig {
    fn new(config: Config) -> Self {
        let apm_fragments = apm_fragment_sources(&config);
        Self {
            config,
            apm_fragments,
        }
    }
}

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
        .unwrap_or_else(|| format!(".{}", symlink.source));
    let mut segments = target
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty());
    if segments.next() != Some(".apm") || segments.next() != Some("config") {
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
    use crate::domains::overlay::config::scripts::ScriptEntry;
    use crate::test_helpers::empty_config;
    use std::path::PathBuf;

    fn script(name: &str) -> ScriptEntry {
        ScriptEntry {
            name: name.to_string(),
            path: format!("scripts/{name}.sh"),
            description: None,
        }
    }

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
    fn reload_swaps_script_configuration_in_both_handles() {
        let mut initial = empty_config(PathBuf::from("/tmp"));
        initial.scripts = vec![script("initial")];
        let store = ConfigStore::from_config(initial);

        let mut reloaded = empty_config(PathBuf::from("/tmp"));
        reloaded.scripts = vec![script("reloaded")];
        store.reload(reloaded);

        assert_eq!(store.scripts.read()[0].name, "reloaded");
        assert_eq!(store.aggregate.read().scripts[0].name, "reloaded");
    }
}
