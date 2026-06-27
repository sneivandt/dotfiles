//! The [`config_section!`] macro for declaring configuration sections.

/// Define a [`ConfigSection`](helpers::toml_loader::ConfigSection) implementation
/// and `load()` function with minimal boilerplate.
///
/// Generates an internal section struct, the `ConfigSection` trait impl,
/// and a public `load()` function that filters by active categories.
///
/// Supports identity mapping (`ty`) and explicit entry-to-item mapping
/// (`entry`, `item`, `map`) variants.
macro_rules! config_section {
    // Identity mapping (Entry == Item).
    (field: $field:literal, ty: $ty:ty $(,)?) => {
        #[derive(Debug, ::serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Section {
            #[serde(rename = $field)]
            entries: Vec<$ty>,
        }

        impl $crate::config::helpers::toml_loader::ConfigSection for Section {
            type Entry = $ty;
            type Item = $ty;

            fn extract(self) -> Vec<$ty> {
                self.entries
            }

            fn map(entry: $ty) -> $ty {
                entry
            }
        }

        /// Load items from the TOML config file, filtered by active categories.
        ///
        /// # Errors
        ///
        /// Returns an error if the file exists but cannot be parsed.
        pub fn load(
            path: &::std::path::Path,
            active_categories: &[$crate::config::helpers::category_matcher::Category],
        ) -> ::anyhow::Result<Vec<$ty>> {
            $crate::config::helpers::toml_loader::load_section::<Section>(path, active_categories)
        }
    };

    // With explicit entry-to-item mapping.
    (
        field: $field:literal,
        entry: $entry:ty,
        item: $item:ty,
        map: |$param:ident| $map_expr:expr $(,)?
    ) => {
        #[derive(Debug, ::serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Section {
            #[serde(rename = $field)]
            entries: Vec<$entry>,
        }

        impl $crate::config::helpers::toml_loader::ConfigSection for Section {
            type Entry = $entry;
            type Item = $item;

            fn extract(self) -> Vec<$entry> {
                self.entries
            }

            fn map($param: $entry) -> $item {
                $map_expr
            }
        }

        /// Load items from the TOML config file, filtered by active categories.
        ///
        /// # Errors
        ///
        /// Returns an error if the file exists but cannot be parsed.
        pub fn load(
            path: &::std::path::Path,
            active_categories: &[$crate::config::helpers::category_matcher::Category],
        ) -> ::anyhow::Result<Vec<$item>> {
            $crate::config::helpers::toml_loader::load_section::<Section>(path, active_categories)
        }
    };
}

pub(crate) use config_section;
