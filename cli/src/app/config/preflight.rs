//! Structural validation shared by all main configuration files.

use anyhow::{Result, bail};

use crate::infra::config::category_matcher::Category;
use crate::infra::config::toml_loader::ConfigDocument;

pub(super) fn validate_category_sections(
    document: &ConfigDocument<'_>,
    known_categories: &[Category],
) -> Result<()> {
    let path = &document.path;
    for section in document.section_names() {
        let mut seen = Vec::new();
        for tag in section.split('-') {
            let tag = tag.trim();
            if tag.is_empty() {
                bail!(
                    "{} section [{section}] contains an empty category tag",
                    path.display()
                );
            }
            let category = Category::from_tag(tag);
            if !known_categories.contains(&category) {
                bail!(
                    "{} section [{section}] uses unknown category '{tag}'; expected one of base, desktop, linux, windows, arch, or wsl",
                    path.display()
                );
            }
            if seen.contains(&category) {
                bail!(
                    "{} section [{section}] repeats category '{tag}'",
                    path.display()
                );
            }
            seen.push(category);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn rejects_invalid_category_tags() {
        let path = Path::new("packages.toml");
        for (section, expected) in [
            ("windwos", "uses unknown category 'windwos'"),
            ("base-", "contains an empty category tag"),
            ("base-base", "repeats category 'base'"),
            ("work", "uses unknown category 'work'"),
        ] {
            let content = format!("[{section}]\npackages = []\n");
            let document = ConfigDocument::parse(path, &content).unwrap();
            let error =
                validate_category_sections(&document, super::super::profiles::KNOWN_CATEGORIES)
                    .expect_err("invalid category should fail");
            let message = format!("{error:#}");
            assert!(message.contains(expected), "{message}");
            assert!(message.contains(&path.display().to_string()), "{message}");
        }
    }
}
