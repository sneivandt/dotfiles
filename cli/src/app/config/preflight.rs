//! Structural validation shared by all main configuration files.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context as _, Result, bail};

use crate::infra::config::category_matcher::Category;
use crate::infra::config::toml_loader;

use super::REQUIRED_CONFIG_FILES;

const CATEGORY_CONFIG_FILES: &[&str] = &[
    "chmod.toml",
    "copilot.toml",
    "git-config.toml",
    "manifest.toml",
    "packages.toml",
    "symlinks.toml",
    "systemd-units.toml",
    "vscode-extensions.toml",
];

const OVERLAY_CATEGORY_CONFIG_FILES: &[&str] = &[
    "chmod.toml",
    "copilot.toml",
    "git-config.toml",
    "packages.toml",
    "scripts.toml",
    "symlinks.toml",
    "systemd-units.toml",
    "vscode-extensions.toml",
];

pub(super) fn validate(
    root: &Path,
    overlay_root: Option<&Path>,
    configured_categories: &[Category],
) -> Result<()> {
    let conf = root.join("conf");
    for file in REQUIRED_CONFIG_FILES {
        let path = conf.join(file);
        drop(toml_loader::load_required_config::<toml::Value>(&path)?);
    }
    for file in CATEGORY_CONFIG_FILES {
        validate_category_sections(&conf.join(file), configured_categories)?;
    }

    if let Some(overlay_root) = overlay_root {
        let overlay_conf = overlay_root.join("conf");
        for file in OVERLAY_CATEGORY_CONFIG_FILES {
            let path = overlay_conf.join(file);
            if path.exists() {
                validate_category_sections(&path, configured_categories).with_context(|| {
                    format!("Invalid configuration in overlay {}", path.display())
                })?;
            }
        }
        let registry = overlay_conf.join("registry.toml");
        if registry.exists() {
            drop(
                toml_loader::load_required_config::<toml::Value>(&registry).with_context(|| {
                    format!("Invalid configuration in overlay {}", registry.display())
                })?,
            );
        }
    }

    Ok(())
}

fn validate_category_sections(path: &Path, configured_categories: &[Category]) -> Result<()> {
    let sections: BTreeMap<String, toml::Value> = toml_loader::load_required_config(path)?;
    for section in sections.keys() {
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
            if !configured_categories.contains(&category) {
                bail!(
                    "{} section [{section}] uses unknown category '{tag}'; declare custom categories in profiles.toml",
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

    fn complete_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        let conf = dir.path().join("conf");
        std::fs::create_dir_all(&conf).expect("conf dir");
        for file in REQUIRED_CONFIG_FILES {
            std::fs::write(conf.join(file), "").expect("write config");
        }
        dir
    }

    #[test]
    fn rejects_missing_main_config() {
        let dir = complete_repo();
        std::fs::remove_file(dir.path().join("conf").join("copilot.toml")).expect("remove config");

        let error = validate(dir.path(), None, &[Category::Base])
            .expect_err("missing main config should fail");

        assert!(error.to_string().contains("copilot.toml"));
    }

    #[test]
    fn rejects_unknown_category_tag() {
        let dir = complete_repo();
        std::fs::write(
            dir.path().join("conf").join("packages.toml"),
            "[windwos]\npackages = []\n",
        )
        .expect("write packages");

        let error = validate(dir.path(), None, &[Category::Base, Category::Windows])
            .expect_err("unknown category should fail");

        assert!(error.to_string().contains("windwos"));
    }

    #[test]
    fn accepts_profile_declared_custom_category() {
        let dir = complete_repo();
        std::fs::write(
            dir.path().join("conf").join("packages.toml"),
            "[work]\npackages = []\n",
        )
        .expect("write packages");

        validate(
            dir.path(),
            None,
            &[Category::Base, Category::Other("work".to_string())],
        )
        .expect("declared custom category should pass");
    }
}
