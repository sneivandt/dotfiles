//! Declarative validation helpers for configuration items.
use crate::config::ValidationWarning;

/// Builder for collecting [`ValidationWarning`]s against a single config source.
///
/// Captures the TOML filename once and provides a fluent interface for
/// checking each item, eliminating the repeated `Vec::new()` + `push()` loop
/// found across config modules.
///
/// # Examples
///
/// ```ignore
/// let warnings = Validator::new("packages.toml")
///     .check_each(&packages, |pkg| &pkg.name, |pkg| {
///         check(!pkg.name.trim().is_empty(), "package name is empty")
///     })
///     .finish();
/// ```
pub struct Validator {
    source: &'static str,
    warnings: Vec<ValidationWarning>,
}

impl Validator {
    /// Create a new validator for the given config source file.
    #[must_use]
    pub const fn new(source: &'static str) -> Self {
        Self {
            source,
            warnings: Vec::new(),
        }
    }

    /// Push a standalone warning not tied to a specific item.
    pub fn warn(&mut self, item: impl Into<String>, message: impl Into<String>) {
        self.warnings
            .push(ValidationWarning::new(self.source, item, message));
    }

    /// Push a warning if the condition is `true`.
    pub fn warn_if(
        &mut self,
        condition: bool,
        item: impl Into<String>,
        message: impl Into<String>,
    ) {
        if condition {
            self.warn(item, message);
        }
    }

    /// Validate each item in a slice, running `check_fn` per item.
    ///
    /// `item_label` extracts the human-readable identifier for warnings.
    /// `check_fn` returns an iterator of optional error messages — each
    /// `Some(message)` becomes a warning.
    pub fn check_each<T>(
        mut self,
        items: &[T],
        item_label: impl Fn(&T) -> &str,
        check_fn: impl Fn(&T) -> Vec<Option<String>>,
    ) -> Self {
        for item in items {
            let label = item_label(item);
            for message in check_fn(item).into_iter().flatten() {
                self.warnings
                    .push(ValidationWarning::new(self.source, label, message));
            }
        }
        self
    }

    /// Consume the builder and return the collected warnings.
    #[must_use]
    pub fn finish(self) -> Vec<ValidationWarning> {
        self.warnings
    }
}

/// Return `Some(message)` if the condition is `true`, else `None`.
///
/// Designed for use inside [`Validator::check_each`] closures:
///
/// ```ignore
/// check(name.is_empty(), "name is empty")
/// ```
#[must_use]
pub fn check(condition: bool, message: impl Into<String>) -> Option<String> {
    condition.then(|| message.into())
}
