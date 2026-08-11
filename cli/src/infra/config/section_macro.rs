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
    (field: $field:literal, ty: $ty:ty $(,)?) => {
        $crate::infra::config::config_section! {
            @define
            field: $field,
            entry: $ty,
            item: $ty,
            map: |entry| entry,
        }
    };

    (
        field: $field:literal,
        entry: $entry:ty,
        item: $item:ty,
        map: |$param:ident| $map_expr:expr $(,)?
    ) => {
        $crate::infra::config::config_section! {
            @define
            field: $field,
            entry: $entry,
            item: $item,
            map: |$param| $map_expr,
        }
    };

    (
        @define
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

        impl $crate::infra::config::toml_loader::ConfigSection for Section {
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
            active_categories: &[$crate::infra::config::category_matcher::Category],
        ) -> ::anyhow::Result<Vec<$item>> {
            $crate::infra::config::toml_loader::load_section::<Section>(path, active_categories)
        }
    };
}

pub(crate) use config_section;
