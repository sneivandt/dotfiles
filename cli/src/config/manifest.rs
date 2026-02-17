use anyhow::Result;
use std::path::Path;

use super::ini;

/// Sparse checkout manifest — files to exclude by category.
#[derive(Debug, Clone)]
pub struct Manifest {
    /// Files that should be present (not excluded).
    #[allow(dead_code)]
    pub included_files: Vec<String>,
    /// Files that should be excluded (in excluded categories).
    pub excluded_files: Vec<String>,
}

/// Load manifest from manifest.ini using OR exclusion logic.
///
/// A file section is excluded if ANY of its category tags match the excluded set.
/// This is the opposite of other config files which use AND inclusion logic.
pub fn load(path: &Path, excluded_categories: &[String]) -> Result<Manifest> {
    let sections = ini::parse_sections(path)?;

    let mut included_files = Vec::new();
    let mut excluded_files = Vec::new();

    for section in &sections {
        // OR logic: exclude if ANY category matches excluded set
        let should_exclude = section
            .categories
            .iter()
            .any(|cat| excluded_categories.contains(cat));

        for item in &section.items {
            if should_exclude {
                excluded_files.push(item.clone());
            } else {
                included_files.push(item.clone());
            }
        }
    }

    Ok(Manifest {
        included_files,
        excluded_files,
    })
}

#[cfg(test)]
mod tests {
    use crate::config::ini::parse_sections_from_str;

    #[test]
    fn or_exclusion_logic() {
        let content =
            "[base]\nfile1\n\n[arch]\nfile2\n\n[windows]\nfile3\n\n[arch,desktop]\nfile4\n";
        let sections = parse_sections_from_str(content).unwrap();

        // Excluding 'windows' should exclude file3
        let excluded = ["windows".to_string()];
        let mut included = Vec::new();
        let mut excluded_files = Vec::new();

        for section in &sections {
            let should_exclude = section.categories.iter().any(|cat| excluded.contains(cat));
            for item in &section.items {
                if should_exclude {
                    excluded_files.push(item.clone());
                } else {
                    included.push(item.clone());
                }
            }
        }

        assert_eq!(included, vec!["file1", "file2", "file4"]);
        assert_eq!(excluded_files, vec!["file3"]);
    }

    #[test]
    fn or_logic_multi_category() {
        let content = "[arch,desktop]\nfile1\n";
        let sections = parse_sections_from_str(content).unwrap();

        // Excluding just 'arch' should still exclude the section (OR logic)
        let excluded = ["arch".to_string()];
        let should_exclude = sections[0]
            .categories
            .iter()
            .any(|cat| excluded.contains(cat));
        assert!(should_exclude);
    }
}
