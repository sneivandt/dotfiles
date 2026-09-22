//! TOML configuration file parsing with category filtering.
use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::path::Path;

use super::category_matcher::{Category, matches, parse_section_key};

/// One filesystem snapshot and TOML parse, shared by structural and typed loading.
#[derive(Debug)]
pub(crate) struct ConfigDocument<'a> {
    pub(crate) path: &'a Path,
    raw: &'a str,
    table: toml::Spanned<toml::de::DeTable<'a>>,
}

impl<'a> ConfigDocument<'a> {
    pub(crate) fn parse(path: &'a Path, raw: &'a str) -> Result<Self> {
        Ok(Self {
            path,
            raw,
            table: toml::de::DeTable::parse(raw)
                .with_context(|| format!("Failed to parse TOML config: {}", path.display()))?,
        })
    }

    pub(crate) fn section_names(&self) -> impl Iterator<Item = &str> {
        self.table
            .get_ref()
            .keys()
            .map(|name| name.get_ref().as_ref())
    }

    pub(crate) fn deserialize<T: DeserializeOwned>(&self) -> Result<T> {
        T::deserialize(toml::de::Deserializer::from(self.table.clone()))
            .map_err(|mut error| {
                error.set_input(Some(self.raw));
                error
            })
            .with_context(|| format!("Failed to parse TOML config: {}", self.path.display()))
    }

    pub(crate) fn section_items<S: DeserializeOwned, T>(
        &self,
        extract: impl Fn(S) -> Vec<T>,
    ) -> Result<Vec<(String, Vec<T>)>> {
        let config: BTreeMap<String, S> = self.deserialize()?;
        Ok(config
            .into_iter()
            .map(|(name, section)| (name, extract(section)))
            .collect())
    }
}

/// Read source text with the caller's required/optional absence policy.
pub(crate) fn read_config(path: &Path, required: bool) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(error) if !required && error.kind() == std::io::ErrorKind::NotFound => {
            Ok(String::new())
        }
        Err(error) => {
            Err(error).with_context(|| format!("Failed to read config file: {}", path.display()))
        }
    }
}

#[cfg(test)]
pub(crate) fn with_optional_document<T>(
    path: &Path,
    decode: impl FnOnce(&ConfigDocument<'_>) -> Result<T>,
) -> Result<T> {
    let content = read_config(path, false)?;
    decode(&ConfigDocument::parse(path, &content)?)
}

/// Filter items from a TOML table by category matching.
///
/// A section is included only when all of its category tags are present in
/// `active_categories` (AND logic).
#[must_use]
pub(crate) fn filter_by_categories<T>(
    items: Vec<(String, Vec<T>)>,
    active_categories: &[Category],
) -> Vec<T> {
    items
        .into_iter()
        .filter(|(section_name, _)| matches(&parse_section_key(section_name), active_categories))
        .flat_map(|(_, items)| items)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::config::test_helpers::write_temp_toml;
    use serde::Deserialize;

    #[test]
    fn missing_files_respect_required_and_optional_boundaries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nonexistent.toml");
        let content = read_config(&path, false).unwrap();
        let document = ConfigDocument::parse(&path, &content).unwrap();
        let result: BTreeMap<String, String> = document.deserialize().unwrap();
        assert!(result.is_empty());
        assert!(
            document
                .section_items(|s: Section| s.items)
                .unwrap()
                .is_empty()
        );
        let required_error = read_config(&path, true).unwrap_err();
        assert_eq!(
            required_error.to_string(),
            format!("Failed to read config file: {}", path.display())
        );
        let optional_error = document.deserialize::<Section>().err().unwrap();
        assert_eq!(
            optional_error.to_string(),
            format!("Failed to parse TOML config: {}", path.display())
        );
    }

    #[test]
    fn load_config_valid_toml() {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Root {
            key: String,
        }
        let (_dir, path) = write_temp_toml("key = \"value\"\n");
        let content = read_config(&path, true).unwrap();
        let root: Root = ConfigDocument::parse(&path, &content)
            .unwrap()
            .deserialize()
            .unwrap();
        assert_eq!(root.key, "value");
    }

    #[test]
    fn loaders_preserve_parse_and_read_errors() {
        for required in [false, true] {
            for content in [
                "{{invalid toml",
                "[base]\nitems = 42",
                "[base]\nitems = \"not-an-array\"",
                "[base]\nitems = []\nunknown = true",
            ] {
                let (_dir, path) = write_temp_toml(content);
                let raw = read_config(&path, required).unwrap();
                let error = ConfigDocument::parse(&path, &raw)
                    .and_then(|document| document.deserialize::<BTreeMap<String, Section>>())
                    .err()
                    .expect("invalid TOML must fail");
                assert_eq!(
                    error.to_string(),
                    format!("Failed to parse TOML config: {}", path.display())
                );
                assert!(error.chain().count() > 1, "preserve the parser error");
                assert!(
                    with_optional_document(&path, |document| document
                        .section_items(|s: Section| s.items))
                    .is_err()
                );
            }
            let dir = tempfile::tempdir().unwrap();
            let error =
                read_config(dir.path(), required).expect_err("reading a directory must fail");
            assert_eq!(
                error.to_string(),
                format!("Failed to read config file: {}", dir.path().display())
            );
        }
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Section {
        items: Vec<String>,
    }

    #[test]
    fn parsed_document_preserves_typed_error_locations() {
        for content in [
            "[base]\nitems = 42",
            "[base]\nitems = []\nunknown = true",
            "[base]\nitems = ['valid']\n[desktop]\nitems = [false]",
        ] {
            let (_dir, path) = write_temp_toml(content);
            let document = ConfigDocument::parse(&path, content).unwrap();
            let error = document
                .deserialize::<BTreeMap<String, Section>>()
                .err()
                .unwrap();
            let direct = toml::from_str::<BTreeMap<String, Section>>(content)
                .err()
                .unwrap();
            assert_eq!(error.root_cause().to_string(), direct.to_string());
            assert!(error.root_cause().to_string().contains("line "));
        }
    }

    #[test]
    fn load_section_items_extracts_sections() {
        let toml = "\
[base]
items = [\"a\", \"b\"]

[desktop]
items = [\"c\"]
";
        let (_dir, path) = write_temp_toml(toml);
        let sections = with_optional_document(&path, |document| {
            document.section_items(|s: Section| s.items)
        })
        .unwrap();
        assert_eq!(
            sections,
            vec![
                ("base".to_string(), vec!["a".to_string(), "b".to_string()]),
                ("desktop".to_string(), vec!["c".to_string()])
            ]
        );
    }

    #[test]
    fn category_filter_preserves_order_and_requires_all_tags() {
        let items = vec![
            ("base".to_string(), vec!["a", "b"]),
            ("arch-desktop".to_string(), vec!["c"]),
            ("arch".to_string(), vec!["d"]),
        ];
        for (active, expected) in [
            (vec![Category::Base], vec!["a", "b"]),
            (vec![Category::Arch], vec!["d"]),
            (vec![Category::Arch, Category::Desktop], vec!["c", "d"]),
            (vec![Category::Linux], vec![]),
            (vec![], vec![]),
        ] {
            assert_eq!(
                filter_by_categories(items.clone(), &active),
                expected,
                "{active:?}"
            );
        }
        assert!(filter_by_categories::<String>(vec![], &[Category::Base]).is_empty());
    }
}
