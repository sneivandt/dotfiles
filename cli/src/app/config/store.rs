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
use crate::infra::ConfigHandle;

macro_rules! define_config_store {
    ($($field:ident: $ty:ty => $count:expr;)+) => {
        /// Shared, atomically-swappable configuration split into per-domain handles.
        ///
        /// Cloning is cheap (each field is an `Arc`-backed [`ConfigHandle`]) and
        /// all clones observe the same slots.
        #[derive(Debug, Clone)]
        pub struct ConfigStore {
            /// Whole configuration, for app-owned validation tasks.
            pub aggregate: ConfigHandle<Config>,
            $(
                #[doc = concat!("Configuration handle for `", stringify!($field), "`.")]
                pub $field: ConfigHandle<$ty>,
            )+
        }

        impl ConfigStore {
            /// Split an aggregate [`Config`] into per-domain handles.
            #[must_use]
            pub fn from_config(config: Config) -> Self {
                Self {
                    $($field: ConfigHandle::new(config.$field.clone()),)+
                    aggregate: ConfigHandle::new(config),
                }
            }

            /// Replace reloadable handles from a freshly-loaded [`Config`].
            ///
            /// Each individual handle swap is atomic, but the complete store
            /// update is not one aggregate transaction. The command runner
            /// completes the reload dependency boundary before rebuilding tasks
            /// that consume these handles.
            pub fn reload(&self, config: Config) {
                $(self.$field.swap(config.$field.clone());)+
                self.aggregate.swap(config);
            }
        }
    };
}

config_section_inventory!(define_config_store);

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    reason = "test code uses direct indexing for focused assertions"
)]
mod tests {
    use super::*;
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
