//! The [`config_section!`] macro for declaring configuration sections.

/// Define a typed decoder for category-keyed configuration lists.
///
/// Generates an internal section struct, a decoder for parsed documents,
/// and a test-only path loader that filters by active categories.
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

        /// Load items from the TOML config file, filtered by active categories.
        ///
        /// # Errors
        ///
        /// Returns an error if the file exists but cannot be parsed.
        #[cfg(test)]
        fn load(
            path: &::std::path::Path,
            active_categories: &[$crate::infra::config::category_matcher::Category],
        ) -> ::anyhow::Result<Vec<$item>> {
            Ok($crate::infra::config::toml_loader::filter_by_categories(
                $crate::infra::config::toml_loader::with_optional_document(path, decode)?,
                active_categories,
            ))
        }

        /// Decode all category sections from the already parsed document.
        pub(crate) fn decode(
            document: &$crate::infra::config::toml_loader::ConfigDocument<'_>,
        ) -> ::anyhow::Result<Vec<(String, Vec<$item>)>> {
            document.section_items(|section: Section| {
                section
                    .entries
                    .into_iter()
                    .map(|$param| $map_expr)
                    .collect()
            })
        }
    };
}

pub(crate) use config_section;
