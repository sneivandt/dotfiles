#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "panicking allowed at this architecture test boundary"
)]
//! Architecture tests for domain import boundaries.

use std::collections::BTreeMap;
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

fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn relative_source_path(path: &Path) -> String {
    path.strip_prefix(source_root())
        .expect("source file should be beneath src")
        .to_string_lossy()
        .replace('\\', "/")
}

fn is_test_source(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == "tests")
        || path.file_stem().is_some_and(|stem| stem == "tests")
}

fn production_source(source: &str) -> &str {
    source
        .split_once("#[cfg(test)]")
        .map_or(source, |(production, _)| production)
}

fn uncommented(line: &str) -> &str {
    line.split("//").next().unwrap_or_default()
}

fn identifier_occurrences(source: &str, identifier: &str) -> usize {
    source
        .match_indices(identifier)
        .filter(|(offset, _)| {
            let before = source[..*offset].chars().next_back();
            let end = offset
                .checked_add(identifier.len())
                .expect("matched identifier offset should remain in bounds");
            let after = source[end..].chars().next();
            before.is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
                && after
                    .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
        })
        .count()
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
fn subprocess_construction_is_owned_by_infrastructure() {
    let src_root = source_root();
    let mut violations = Vec::new();

    for file in rust_files(&src_root) {
        let relative = relative_source_path(&file);
        if relative.starts_with("infra/")
            || relative == "app/commands/reexec.rs"
            || is_test_source(&file)
        {
            continue;
        }

        let source = std::fs::read_to_string(&file).expect("read Rust source");
        for (line_index, line) in production_source(&source).lines().enumerate() {
            let code = uncommented(line);
            for (offset, _) in code.match_indices("Command::new(") {
                let preceding = code[..offset].chars().next_back();
                if preceding
                    .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
                {
                    continue;
                }
                violations.push(format!(
                    "{}:{} constructs a process directly",
                    file.display(),
                    line_index + 1
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "direct process construction must use crate::infra::exec; \
                 app/commands/reexec.rs is the documented lifecycle exception:\n{}",
        violations.join("\n")
    );
}

#[test]
fn runtime_platform_detection_uses_platform_capabilities() {
    let src_root = source_root();
    let allowed = [
        // Release artifact and installed binary names are compile-target metadata.
        "domains/dotfiles/self_update/paths.rs",
        // Interpreter fallback must distinguish Windows PowerShell from pwsh.
        "domains/overlay/resources/script.rs",
    ];
    let mut violations = Vec::new();

    for file in rust_files(&src_root) {
        let relative = relative_source_path(&file);
        if relative == "infra/platform.rs"
            || allowed.contains(&relative.as_str())
            || is_test_source(&file)
        {
            continue;
        }

        let source = std::fs::read_to_string(&file).expect("read Rust source");
        for (line_index, line) in production_source(&source).lines().enumerate() {
            let compact = uncommented(line)
                .chars()
                .filter(|character| !character.is_ascii_whitespace())
                .collect::<String>();
            let cfg_probe = compact.contains("cfg!(")
                && [
                    "windows",
                    "unix",
                    "target_os",
                    "target_family",
                    "target_arch",
                ]
                .iter()
                .any(|target| compact.contains(target));
            if cfg_probe || compact.contains("std::env::consts::OS") {
                violations.push(format!(
                    "{}:{} performs runtime platform detection",
                    file.display(),
                    line_index + 1
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "runtime platform checks must use Platform/System capabilities; \
                 use #[cfg(...)] only for compile-time implementation gating:\n{}",
        violations.join("\n")
    );
}

#[test]
fn domain_tasks_are_registered_by_the_application() {
    let src_root = source_root();
    let domains_root = src_root.join("domains");
    let mut task_types = BTreeMap::new();

    for file in rust_files(&domains_root) {
        if is_test_source(&file) {
            continue;
        }
        let source = std::fs::read_to_string(&file).expect("read Rust source");
        let mut task_macro = false;
        for line in production_source(&source).lines() {
            let code = uncommented(line).trim();
            if code.contains("resource_task! {") {
                task_macro = true;
                continue;
            }
            if task_macro && let Some(declaration) = code.strip_prefix("pub ") {
                let name = declaration
                    .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                    .next()
                    .unwrap_or_default();
                if !name.is_empty() {
                    task_types.insert(name.to_owned(), file.clone());
                }
                task_macro = false;
            }
            if let Some(offset) = code.find("Task for ") {
                let preceding = code[..offset].chars().next_back();
                if preceding
                    .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
                {
                    continue;
                }
                let implementation_start = offset
                    .checked_add("Task for ".len())
                    .expect("matched Task implementation offset should remain in bounds");
                let implementation = &code[implementation_start..];
                let name = implementation
                    .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                    .next()
                    .unwrap_or_default();
                if !name.is_empty() {
                    task_types.insert(name.to_owned(), file.clone());
                }
            }
        }
    }

    let mut registration_source =
        std::fs::read_to_string(src_root.join("app/catalog.rs")).expect("read task catalog");
    for file in rust_files(&src_root.join("app/commands")) {
        if is_test_source(&file) {
            continue;
        }
        registration_source.push_str(production_source(
            &std::fs::read_to_string(file).expect("read command source"),
        ));
    }

    let dynamic_tasks = [
        // One instance per private-overlay script is injected after config reload.
        "OverlayScriptTask",
    ];
    let mut violations = Vec::new();
    for (task_type, file) in task_types {
        if dynamic_tasks.contains(&task_type.as_str()) {
            continue;
        }
        if identifier_occurrences(&registration_source, &task_type) < 2 {
            violations.push(format!(
                "{} ({}) is not imported and constructed by app/catalog.rs or app/commands",
                task_type,
                file.display()
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "domain Task implementations must be registered by an application command; \
                 document convention-based dynamic injection explicitly:\n{}",
        violations.join("\n")
    );
}

#[test]
fn wrappers_only_bootstrap_and_forward() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cli should be inside repository root");
    let wrappers = [
        ("dotfiles.sh", "exec \"$BINARY\" \"$@\""),
        ("dotfiles.ps1", "& $Binary @CliArgs"),
    ];
    let forbidden = [
        "pacman ",
        "paru ",
        "winget ",
        "systemctl ",
        "git config ",
        "code --install-extension",
        "ln -s ",
        "new-item -itemtype symboliclink",
        "set-itemproperty ",
        "new-itemproperty ",
        "reg.exe ",
    ];
    let mut violations = Vec::new();

    for (wrapper, forwarding) in wrappers {
        let path = repo_root.join(wrapper);
        let source = std::fs::read_to_string(&path).expect("read wrapper");
        if !source.lines().any(|line| {
            !line.trim_start().starts_with('#') && uncommented(line).contains(forwarding)
        }) {
            violations.push(format!(
                "{} does not preserve the expected argument-forwarding boundary",
                path.display()
            ));
        }

        for (line_index, line) in source.lines().enumerate() {
            let code = line.trim();
            if code.starts_with('#') {
                continue;
            }
            let lowercase = code.to_ascii_lowercase();
            for pattern in forbidden {
                if lowercase.contains(pattern) {
                    violations.push(format!(
                        "{}:{} contains domain orchestration pattern '{pattern}'",
                        path.display(),
                        line_index + 1
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "wrappers may bootstrap/update the Rust binary and forward arguments, \
                 but domain convergence belongs in cli/src:\n{}",
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
