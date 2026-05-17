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
pub(crate) struct Validator {
    source: &'static str,
    warnings: Vec<ValidationWarning>,
}

impl Validator {
    /// Create a new validator for the given config source file.
    #[must_use]
    pub(crate) const fn new(source: &'static str) -> Self {
        Self {
            source,
            warnings: Vec::new(),
        }
    }

    /// Push a standalone warning not tied to a specific item.
    pub(crate) fn warn(&mut self, item: impl Into<String>, message: impl Into<String>) {
        self.warnings
            .push(ValidationWarning::new(self.source, item, message));
    }

    /// Push a warning if the condition is `true`.
    pub(crate) fn warn_if(
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
    pub(crate) fn check_each<T>(
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
    pub(crate) fn finish(self) -> Vec<ValidationWarning> {
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
pub(crate) fn check(condition: bool, message: impl Into<String>) -> Option<String> {
    condition.then(|| message.into())
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;

    struct Item {
        label: &'static str,
        value: i32,
    }

    #[test]
    fn check_returns_message_when_condition_is_true() {
        assert_eq!(
            check(true, "invalid value"),
            Some("invalid value".to_string())
        );
    }

    #[test]
    fn check_returns_none_when_condition_is_false() {
        assert_eq!(check(false, "invalid value"), None);
    }

    #[test]
    fn warn_records_source_item_and_message() {
        let mut validator = Validator::new("example.toml");

        validator.warn("item-a", "is invalid");

        assert_eq!(
            validator.finish(),
            vec![ValidationWarning::new(
                "example.toml",
                "item-a",
                "is invalid"
            )],
        );
    }

    #[test]
    fn warn_if_records_only_true_conditions() {
        let mut validator = Validator::new("example.toml");

        validator.warn_if(false, "item-a", "should not appear");
        validator.warn_if(true, "item-b", "should appear");

        assert_eq!(
            validator.finish(),
            vec![ValidationWarning::new(
                "example.toml",
                "item-b",
                "should appear"
            )],
        );
    }

    #[test]
    fn check_each_collects_all_some_messages_and_ignores_none() {
        let items = [
            Item {
                label: "negative",
                value: -1,
            },
            Item {
                label: "zero",
                value: 0,
            },
            Item {
                label: "large",
                value: 42,
            },
        ];

        let warnings = Validator::new("numbers.toml")
            .check_each(
                &items,
                |item| item.label,
                |item| {
                    vec![
                        check(item.value < 0, "must be non-negative"),
                        check(item.value == 0, "must be non-zero"),
                        check(item.value > 10, "must be at most 10"),
                    ]
                },
            )
            .finish();

        assert_eq!(
            warnings,
            vec![
                ValidationWarning::new("numbers.toml", "negative", "must be non-negative"),
                ValidationWarning::new("numbers.toml", "zero", "must be non-zero"),
                ValidationWarning::new("numbers.toml", "large", "must be at most 10"),
            ],
        );
    }

    #[test]
    fn check_each_can_be_chained_after_standalone_warnings() {
        let mut validator = Validator::new("numbers.toml");
        validator.warn("file", "global warning");

        let warnings = validator
            .check_each(
                &[Item {
                    label: "negative",
                    value: -5,
                }],
                |item| item.label,
                |item| vec![check(item.value < 0, "must be non-negative")],
            )
            .finish();

        assert_eq!(
            warnings,
            vec![
                ValidationWarning::new("numbers.toml", "file", "global warning"),
                ValidationWarning::new("numbers.toml", "negative", "must be non-negative"),
            ],
        );
    }
}
