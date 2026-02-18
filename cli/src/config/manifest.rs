use anyhow::Result;
use std::path::Path;

use super::ini;

/// Sparse checkout manifest — files to exclude by category.
#[derive(Debug, Clone)]
pub struct Manifest {
    /// Files that should be excluded (in excluded categories).
    pub excluded_files: Vec<String>,
}

/// Load manifest from manifest.ini using OR exclusion logic.
///
/// A file section is excluded if ANY of its category tags match the excluded set.
/// This is the opposite of other config files which use AND inclusion logic.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be parsed.
pub fn load(path: &Path, excluded_categories: &[String]) -> Result<Manifest> {
    let sections = ini::parse_sections(path)?;

    let mut excluded_files = Vec::new();

    for section in &sections {
        // OR logic: exclude if ANY category matches excluded set
        let should_exclude = section
            .categories
            .iter()
            .any(|cat| excluded_categories.contains(cat));

        if should_exclude {
            excluded_files.extend(section.items.iter().cloned());
        }
    }

    Ok(Manifest { excluded_files })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::test_helpers::write_temp_ini;

    #[test]
    fn or_exclusion_logic() {
        let (_dir, path) = write_temp_ini(
            "[base]\nfile1\n\n[arch]\nfile2\n\n[windows]\nfile3\n\n[arch,desktop]\nfile4\n",
        );
        // Excluding 'windows' should exclude only file3
        let manifest = load(&path, &["windows".to_string()]).unwrap();
        assert_eq!(manifest.excluded_files, vec!["file3"]);
    }

    #[test]
    fn or_logic_multi_category() {
        let (_dir, path) = write_temp_ini("[arch,desktop]\nfile1\n");
        // Excluding just 'arch' should still exclude the section (OR logic)
        let manifest = load(&path, &["arch".to_string()]).unwrap();
        assert_eq!(manifest.excluded_files, vec!["file1"]);
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = load(&dir.path().join("nope.ini"), &["windows".to_string()]).unwrap();
        assert!(
            manifest.excluded_files.is_empty(),
            "missing file should produce empty manifest"
        );
    }

    #[test]
    fn excludes_nothing_when_no_categories_match() {
        let (_dir, path) = write_temp_ini("[base]\nfile1\n\n[arch]\nfile2\n");
        let manifest = load(&path, &["windows".to_string()]).unwrap();
        assert!(
            manifest.excluded_files.is_empty(),
            "no categories matched — nothing should be excluded"
        );
    }
}
