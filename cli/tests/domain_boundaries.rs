#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::wildcard_enum_match_arm,
    reason = "panicking and wildcard arms over syn's non-exhaustive enums are acceptable at this architecture test boundary"
)]
//! Architecture tests for domain import boundaries.
//!
//! Rust sources are parsed into a syntax tree with `syn` rather than scanned as
//! text.  Comments, doc comments, string literals, and `#[cfg(test)]` blocks are
//! therefore excluded structurally instead of by substring heuristics, so a rule
//! can neither be tripped by a mention inside a comment nor silently stop
//! matching because of how a line happens to be written.
//!
//! Macro arguments are walked at the token level for the same reason: they hold
//! real code (task guards, dependency lists) that `syn` cannot turn into typed
//! nodes, but tokens are already lexed, so comments and literals stay
//! distinguishable from code.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::visit::Visit;

/// `cfg!` predicates that describe the host platform rather than gate an
/// implementation at compile time.
const PLATFORM_PREDICATES: [&str; 5] = [
    "windows",
    "unix",
    "target_os",
    "target_family",
    "target_arch",
];

/// Facts extracted from one parsed source file.
#[derive(Default)]
struct Facts {
    /// Every `a::b::c` chain appearing in code, with the line it starts on.
    paths: Vec<(Vec<String>, usize)>,
    /// Every macro invocation: name, argument tokens, and line.
    macros: Vec<(String, TokenStream, usize)>,
    /// Types carrying an `impl Task for ...` block.
    task_impls: Vec<String>,
    /// Types declared by a `resource_task!` / `config_resource_task!` call.
    declared_tasks: Vec<String>,
    /// Every path introduced by a `use` declaration, with its line.
    imports: Vec<(Vec<String>, usize)>,
}

/// One parsed source file and the facts extracted from it.
struct Source {
    path: PathBuf,
    relative: String,
    facts: Facts,
}

/// Walks a syntax tree, recording the references each architecture rule checks.
struct Collector {
    facts: Facts,
    /// When false, items gated behind `#[cfg(test)]` are skipped entirely.
    include_test_code: bool,
}

impl Collector {
    fn skip(&self, attributes: &[syn::Attribute]) -> bool {
        !self.include_test_code && is_cfg_test(attributes)
    }
}

impl<'ast> Visit<'ast> for Collector {
    /// Attributes are compile-time metadata, never runtime code: `#[cfg(...)]`
    /// gating is explicitly allowed, and doc comments arrive here as
    /// `#[doc = "..."]` rather than as text that has to be stripped.
    fn visit_attribute(&mut self, _attribute: &'ast syn::Attribute) {}

    fn visit_item(&mut self, item: &'ast syn::Item) {
        if self.skip(item_attributes(item)) {
            return;
        }
        syn::visit::visit_item(self, item);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if self.skip(&item.attrs) {
            return;
        }
        syn::visit::visit_impl_item_fn(self, item);
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if let Some((None, trait_path, _)) = item.trait_.as_ref()
            && trait_path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "Task")
            && let syn::Type::Path(self_ty) = item.self_ty.as_ref()
            && let Some(name) = self_ty.path.segments.last()
        {
            self.facts.task_impls.push(name.ident.to_string());
        }
        syn::visit::visit_item_impl(self, item);
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        if self.skip(&item.attrs) {
            return;
        }
        collect_use_tree(&item.tree, &mut Vec::new(), &mut self.facts.imports);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        let line = path
            .segments
            .first()
            .map_or(0, |segment| segment.ident.span().start().line);
        let segments = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        self.facts.paths.push((segments, line));
        syn::visit::visit_path(self, path);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let Some(name) = mac.path.segments.last() else {
            return;
        };
        let name = name.ident.to_string();
        let line = name_line(&mac.path);
        if matches!(name.as_str(), "resource_task" | "config_resource_task")
            && let Some(declared) = declared_task_name(mac.tokens.clone())
        {
            self.facts.declared_tasks.push(declared);
        }
        self.facts.macros.push((name, mac.tokens.clone(), line));
        collect_tokens(mac.tokens.clone(), &mut self.facts);
        syn::visit::visit_macro(self, mac);
    }
}

fn name_line(path: &syn::Path) -> usize {
    path.segments
        .first()
        .map_or(0, |segment| segment.ident.span().start().line)
}

/// The attributes attached to any item, whatever its kind.
fn item_attributes(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Const(node) => &node.attrs,
        syn::Item::Enum(node) => &node.attrs,
        syn::Item::ExternCrate(node) => &node.attrs,
        syn::Item::Fn(node) => &node.attrs,
        syn::Item::ForeignMod(node) => &node.attrs,
        syn::Item::Impl(node) => &node.attrs,
        syn::Item::Macro(node) => &node.attrs,
        syn::Item::Mod(node) => &node.attrs,
        syn::Item::Static(node) => &node.attrs,
        syn::Item::Struct(node) => &node.attrs,
        syn::Item::Trait(node) => &node.attrs,
        syn::Item::TraitAlias(node) => &node.attrs,
        syn::Item::Type(node) => &node.attrs,
        syn::Item::Union(node) => &node.attrs,
        syn::Item::Use(node) => &node.attrs,
        _ => &[],
    }
}

/// Whether any attribute gates its item behind `cfg(test)`, including
/// composites such as `#[cfg(all(test, unix))]`.
fn is_cfg_test(attributes: &[syn::Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("cfg")
            && attribute
                .meta
                .require_list()
                .is_ok_and(|list| mentions_ident(&list.tokens, "test"))
    })
}

/// Whether a token stream contains `wanted` as an identifier at any depth.
fn mentions_ident(tokens: &TokenStream, wanted: &str) -> bool {
    tokens.clone().into_iter().any(|tree| match tree {
        TokenTree::Ident(ident) => ident == wanted,
        TokenTree::Group(group) => mentions_ident(&group.stream(), wanted),
        _ => false,
    })
}

/// Flatten a `use` declaration into one path per imported name.
fn collect_use_tree(
    tree: &syn::UseTree,
    prefix: &mut Vec<String>,
    out: &mut Vec<(Vec<String>, usize)>,
) {
    match tree {
        syn::UseTree::Path(node) => {
            prefix.push(node.ident.to_string());
            collect_use_tree(&node.tree, prefix, out);
            prefix.pop();
        }
        syn::UseTree::Name(node) => {
            let mut path = prefix.clone();
            path.push(node.ident.to_string());
            out.push((path, node.ident.span().start().line));
        }
        syn::UseTree::Rename(node) => {
            let mut path = prefix.clone();
            path.push(node.ident.to_string());
            out.push((path, node.ident.span().start().line));
        }
        syn::UseTree::Glob(node) => out.push((prefix.clone(), node.star_token.span.start().line)),
        syn::UseTree::Group(node) => {
            for item in &node.items {
                collect_use_tree(item, prefix, out);
            }
        }
    }
}

/// The type name declared by a `resource_task!`-family invocation, whose body
/// starts with optional doc attributes, a visibility, and then the type name.
fn declared_task_name(tokens: TokenStream) -> Option<String> {
    let mut trees = tokens.into_iter().peekable();
    let mut after_visibility = false;

    while let Some(tree) = trees.next() {
        match tree {
            TokenTree::Punct(punct) if punct.as_char() == '#' => {
                trees.next();
            }
            TokenTree::Ident(ident) if ident == "pub" => {
                after_visibility = true;
                if matches!(trees.peek(), Some(TokenTree::Group(group))
                    if group.delimiter() == Delimiter::Parenthesis)
                {
                    trees.next();
                }
            }
            TokenTree::Ident(ident) if after_visibility => return Some(ident.to_string()),
            _ => {}
        }
    }

    None
}

/// Record a completed path chain and clear the accumulator.
fn flush(segments: &mut Vec<String>, line: usize, facts: &mut Facts) {
    if !segments.is_empty() {
        facts.paths.push((std::mem::take(segments), line));
    }
}

/// Extract path chains and nested macro invocations from raw macro arguments.
fn collect_tokens(tokens: TokenStream, facts: &mut Facts) {
    let mut segments: Vec<String> = Vec::new();
    let mut line = 0;
    let mut colons: u8 = 0;
    let mut pending_attribute = false;
    let mut trees = tokens.into_iter().peekable();

    while let Some(tree) = trees.next() {
        let was_attribute = std::mem::take(&mut pending_attribute);
        match tree {
            TokenTree::Ident(ident) => {
                let ident_line = ident.span().start().line;
                if colons != 2 {
                    flush(&mut segments, line, facts);
                    line = ident_line;
                }
                segments.push(ident.to_string());
                colons = 0;
                if matches!(trees.peek(), Some(TokenTree::Punct(punct)) if punct.as_char() == '!') {
                    trees.next();
                    if let Some(TokenTree::Group(group)) = trees.peek() {
                        let name = segments.last().cloned().unwrap_or_default();
                        facts.macros.push((name, group.stream(), ident_line));
                    }
                }
            }
            TokenTree::Punct(punct) => match punct.as_char() {
                ':' => colons = colons.saturating_add(1),
                '#' => {
                    flush(&mut segments, line, facts);
                    colons = 0;
                    pending_attribute = true;
                }
                _ => {
                    flush(&mut segments, line, facts);
                    colons = 0;
                }
            },
            TokenTree::Group(group) => {
                flush(&mut segments, line, facts);
                colons = 0;
                if !(was_attribute && group.delimiter() == Delimiter::Bracket) {
                    collect_tokens(group.stream(), facts);
                }
            }
            TokenTree::Literal(_) => {
                flush(&mut segments, line, facts);
                colons = 0;
            }
        }
    }

    flush(&mut segments, line, facts);
}

/// Where `needle` begins inside `segments`, as a consecutive subsequence.
fn sequence_position(segments: &[String], needle: &[&str]) -> Option<usize> {
    segments
        .windows(needle.len())
        .position(|window| window.iter().zip(needle).all(|(have, want)| have == want))
}

fn contains_sequence(segments: &[String], needle: &[&str]) -> bool {
    sequence_position(segments, needle).is_some()
}

/// Parse one source file and extract the facts the architecture rules need.
fn parse_source(path: &Path, include_test_code: bool) -> Source {
    let text = std::fs::read_to_string(path).expect("read Rust source");
    let ast = match syn::parse_file(&text) {
        Ok(ast) => ast,
        Err(error) => panic!("{} is not parseable Rust: {error}", path.display()),
    };

    let mut collector = Collector {
        facts: Facts::default(),
        include_test_code,
    };
    collector.visit_file(&ast);

    Source {
        relative: relative_source_path(path),
        path: path.to_path_buf(),
        facts: collector.facts,
    }
}

/// Parse every non-test Rust file beneath `root`.
fn production_sources(root: &Path) -> Vec<Source> {
    rust_files(root)
        .iter()
        .filter(|path| !is_test_source(path))
        .map(|path| parse_source(path, false))
        .collect()
}

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
    let mut violations = Vec::new();

    for source in production_sources(&source_root()) {
        if source.relative.starts_with("infra/") || source.relative == "app/commands/reexec.rs" {
            continue;
        }

        for (segments, line) in &source.facts.paths {
            if contains_sequence(segments, &["Command", "new"]) {
                violations.push(format!(
                    "{}:{line} constructs a process directly",
                    source.path.display()
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
    let allowed = [
        // Release artifact and installed binary names are compile-target metadata.
        "domains/dotfiles/self_update/paths.rs",
        // Interpreter fallback must distinguish Windows PowerShell from pwsh.
        "domains/overlay/resources/script.rs",
    ];
    let mut violations = Vec::new();

    for source in production_sources(&source_root()) {
        if source.relative == "infra/platform.rs" || allowed.contains(&source.relative.as_str()) {
            continue;
        }

        let mut report = |line: &usize| {
            violations.push(format!(
                "{}:{line} performs runtime platform detection",
                source.path.display()
            ));
        };

        for (name, tokens, line) in &source.facts.macros {
            if name == "cfg"
                && PLATFORM_PREDICATES
                    .iter()
                    .any(|predicate| mentions_ident(tokens, predicate))
            {
                report(line);
            }
        }

        for (segments, line) in &source.facts.paths {
            if contains_sequence(segments, &["consts", "OS"]) {
                report(line);
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
    let mut task_types = BTreeMap::new();

    for source in production_sources(&src_root.join("domains")) {
        for name in source
            .facts
            .task_impls
            .iter()
            .chain(&source.facts.declared_tasks)
        {
            task_types.insert(name.clone(), source.path.clone());
        }
    }

    // Only names reached through real code count as registration: `use`
    // declarations are collected separately, so importing a task without ever
    // constructing or referencing it is not enough.
    let mut referenced = BTreeSet::new();
    let mut registration = production_sources(&src_root.join("app/commands"));
    registration.push(parse_source(&src_root.join("app/catalog.rs"), false));
    for source in &registration {
        for (segments, _) in &source.facts.paths {
            referenced.extend(segments.iter().cloned());
        }
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
        if !referenced.contains(&task_type) {
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
        if !source
            .lines()
            .any(|line| !line.trim_start().starts_with('#') && line.contains(forwarding))
        {
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
            // Test code is intentionally included: a test that reaches across
            // domains couples them just as tightly as production code does.
            let source = parse_source(&file, true);
            let references = source
                .facts
                .paths
                .iter()
                .chain(&source.facts.imports)
                .map(|(segments, line)| (segments.as_slice(), *line));

            for (segments, line) in references {
                if contains_sequence(segments, &["crate", "app"]) {
                    violations.push(format!("{}:{line} imports crate::app", file.display()));
                }

                if let Some(offset) = sequence_position(segments, &["crate", "domains"])
                    && let Some(referenced_domain) =
                        offset.checked_add(2).and_then(|start| segments.get(start))
                    && referenced_domain != domain
                {
                    violations.push(format!(
                        "{}:{line} imports sibling domain '{referenced_domain}' from '{domain}'",
                        file.display()
                    ));
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
