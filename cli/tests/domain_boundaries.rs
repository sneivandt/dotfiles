#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "panicking allowed at this architecture test boundary"
)]
//! Architecture tests for domain import boundaries.

use std::path::{Path, PathBuf};

fn rust_files(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();

    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).expect("read domain directory") {
            let path = entry.expect("read domain entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }

    files
}

#[test]
fn domain_subdirectories_are_shared_layers_or_feature_support() {
    let domains_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/domains");
    let mut violations = Vec::new();

    for domain_entry in std::fs::read_dir(&domains_root).expect("read domains directory") {
        let domain_path = domain_entry.expect("read domain entry").path();
        if !domain_path.is_dir() {
            continue;
        }

        for entry in std::fs::read_dir(&domain_path).expect("read domain directory") {
            let path = entry.expect("read domain entry").path();
            if !path.is_dir() {
                continue;
            }

            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .expect("domain subdirectory should be valid UTF-8");
            if name == "tasks" {
                violations.push(format!(
                    "{} is a forbidden generic task directory",
                    path.display()
                ));
                continue;
            }
            if matches!(name, "config" | "resources" | "tests") {
                continue;
            }

            let entry_point = domain_path.join(format!("{name}.rs"));
            if !entry_point.is_file() {
                violations.push(format!(
                    "{} has no root task entry point {}",
                    path.display(),
                    entry_point.display()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "domain layout violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn domains_do_not_import_the_app_or_sibling_domains() {
    let domains_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/domains");
    let mut violations = Vec::new();

    for entry in std::fs::read_dir(&domains_root).expect("read domains directory") {
        let path = entry.expect("read domain entry").path();
        if !path.is_dir() {
            continue;
        }
        let domain = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("domain directory should be valid UTF-8");

        for file in rust_files(&path) {
            let source = std::fs::read_to_string(&file).expect("read Rust source");
            for (line_index, line) in source.lines().enumerate() {
                let code = line.split("//").next().unwrap_or_default();
                if code.contains("crate::app") {
                    violations.push(format!(
                        "{}:{} imports crate::app",
                        file.display(),
                        line_index + 1
                    ));
                }

                for (offset, _) in code.match_indices("crate::domains::") {
                    let reference = &code[offset + "crate::domains::".len()..];
                    let referenced_domain = reference
                        .split(|character: char| {
                            !character.is_ascii_alphanumeric() && character != '_'
                        })
                        .next()
                        .unwrap_or_default();
                    if !referenced_domain.is_empty() && referenced_domain != domain {
                        violations.push(format!(
                            "{}:{} imports sibling domain '{referenced_domain}' from '{domain}'",
                            file.display(),
                            line_index + 1
                        ));
                    }
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "domain boundary violations:\n{}",
        violations.join("\n")
    );
}
