---
name: rust-docs
description: >
  Rustdoc conventions for the dotfiles Rust engine. Use when documenting public
  items in cli/src/ — doc comment syntax, required sections, must_use, and doc-test annotations.
metadata:
  author: sneivandt
  version: "1.0"
---

# Rust Documentation Conventions

All public items (modules, structs, enums, traits, functions) require documentation
comments using `///` syntax.

## Standard Sections

Use these headers in order when applicable:

1. **Main description** (first, no header) — brief summary of purpose and behaviour
2. **`# Examples`** — code examples demonstrating usage
3. **`# Errors`** — required for all functions returning `Result<T>`
4. **`# Panics`** — document panic conditions (use sparingly; prefer `Result`)
5. **`# Safety`** — required for `unsafe` functions

## Examples in Doc Comments

Code blocks in doc comments are compiled and run by `cargo test` unless annotated.

```rust
/// Parse configuration from file.
///
/// # Examples
///
/// ```
/// use my_crate::parse_config;
/// let config = parse_config("path/to/file");
/// ```
pub fn parse_config(path: &str) -> Config { /* ... */ }
```

**Annotations:**

| Annotation | When to use |
|---|---|
| *(none)* | Rust code — compiled and tested as doctests |
| `` `ignore `` | Conceptual examples with pseudo-code that shouldn't compile |
| `` `ini `` / `` `bash `` / `` `text `` | Non-Rust code |

Use `` `ignore `` for comment-only or pseudo-code examples:

```rust
/// Filter sections using AND logic.
///
/// # Examples
///
/// ```ignore
/// // A section tagged [arch,desktop] requires both "arch" AND "desktop"
/// // to be in the active set to be included.
/// ```
pub fn filter_sections_and(sections: &[Section], active: &[String]) -> Vec<Section> { /* ... */ }
```

## Errors Section

All public functions returning `Result<T>` **must** document error conditions:

```rust
/// Load configuration from INI file.
///
/// # Errors
///
/// Returns an error if the file cannot be read or contains invalid syntax.
pub fn load_config(path: &Path) -> Result<Config> { /* ... */ }
```

Be specific: I/O failures, permission issues, validation errors, etc.

## `#[must_use]`

Apply `#[must_use]` where ignoring the return value is likely a bug:

```rust
/// Returns whether this platform supports systemd.
#[must_use]
pub const fn supports_systemd(&self) -> bool {
    self.os == Os::Linux
}
```

Common cases: boolean queries (`is_*`, `has_*`, `supports_*`), constructors returning
`Self`, pure functions, functions that clone or transform data.

## Structs and Enums

Document all public fields and enum variants:

```rust
/// Result of a command execution.
#[derive(Debug)]
pub struct ExecResult {
    /// Standard output as UTF-8 string.
    pub stdout: String,
    /// Standard error as UTF-8 string.
    pub stderr: String,
    /// Whether the command exited successfully (status code 0).
    pub success: bool,
}

/// State of a resource (file, registry entry, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceState {
    /// Resource does not exist or is not present.
    Missing,
    /// Resource exists and matches the desired state.
    Correct,
    /// Resource exists but does not match the desired state.
    Incorrect { current: String },
}
```

## Traits

Document overall purpose, implementor responsibilities, and example usage:

```rust
/// Unified interface for resources that can be checked and applied.
///
/// # Examples
///
/// ```ignore
/// // All resources follow the same pattern:
/// // 1. Check current state: resource.current_state()?
/// // 2. Apply if needed: resource.apply()?
/// ```
pub trait Resource {
    fn description(&self) -> String;
    // ...
}
```

## Rules

- Document **all** public items with `///` comments
- Include `# Errors` for every public function returning `Result<T>`
- Use `#[must_use]` on constructors, queries, and pure functions
- Annotate non-compilable examples with `` `ignore ``; use language tags for non-Rust blocks
