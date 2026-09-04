//! Declarative validation helpers for configuration items.
use super::diagnostics::{Diagnostic, DiagnosticCode, Severity};

/// A single check result: `(code, severity, message)`.
///
/// `Some` means the check fired; `None` means the item passed.  Returned by
/// [`check`] and [`check_error`], and consumed by
/// [`Validator::check_each`].
pub(crate) type CheckItem = Option<(DiagnosticCode, Severity, String)>;

/// Builder for collecting [`Diagnostic`]s against a single config source.
///
/// Captures the TOML filename once and provides a fluent interface for
/// checking each item, eliminating the repeated `Vec::new()` + `push()` loop
/// found across config modules.
///
/// # Examples
///
/// ```ignore
/// let diagnostics = Validator::new("packages.toml")
///     .check_each(&packages, |pkg| &pkg.name, |pkg| {
///         [
///             check(!pkg.name.trim().is_empty(), "package.empty-name", "package name is empty"),
///         ]
///     })
///     .finish();
/// ```
pub(crate) struct Validator {
    source: &'static str,
    diagnostics: Vec<Diagnostic>,
}

impl Validator {
    /// Create a new validator for the given config source file.
    #[must_use]
    pub(crate) const fn new(source: &'static str) -> Self {
        Self {
            source,
            diagnostics: Vec::new(),
        }
    }

    /// Push a standalone [`Severity::Warning`] diagnostic.
    pub(crate) fn warn(
        &mut self,
        code: DiagnosticCode,
        item: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.diagnostics.push(Diagnostic::new(
            self.source,
            item,
            Severity::Warning,
            code,
            message,
        ));
    }

    /// Push a [`Severity::Warning`] diagnostic if `condition` is `true`.
    #[must_use]
    pub(crate) fn warn_if(
        mut self,
        condition: bool,
        code: DiagnosticCode,
        item: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        if condition {
            self.warn(code, item, message);
        }
        self
    }

    /// Validate each item in a slice, running `check_fn` per item.
    ///
    /// `item_label` extracts the human-readable identifier for diagnostics.
    /// `check_fn` returns any iterable of [`CheckItem`]s — each `Some` value
    /// becomes a diagnostic.  Fixed-count callers can return a stack array
    /// (`[...]`) to avoid a per-item heap allocation, while callers with a
    /// variable number of checks can return a `Vec`.
    pub(crate) fn check_each<T, I>(
        mut self,
        items: &[T],
        item_label: impl Fn(&T) -> &str,
        check_fn: impl Fn(&T) -> I,
    ) -> Self
    where
        I: IntoIterator<Item = CheckItem>,
    {
        for item in items {
            let label = item_label(item);
            for (code, severity, message) in check_fn(item).into_iter().flatten() {
                self.diagnostics.push(match severity {
                    Severity::Warning => Diagnostic::warning(self.source, label, code, message),
                    Severity::Error => Diagnostic::error(self.source, label, code, message),
                });
            }
        }
        self
    }

    /// Reject different desired values for the same target, retaining both
    /// declaration locations. Identical declarations are harmless.
    pub(crate) fn check_conflicts<T, V: PartialEq>(
        mut self,
        items: &[T],
        code: DiagnosticCode,
        identity: impl Fn(&T) -> String,
        value: impl Fn(&T) -> V,
        source: impl Fn(&T) -> Option<&str>,
    ) -> Self {
        let mut seen = std::collections::BTreeMap::new();
        for (index, item) in items.iter().enumerate() {
            let key = identity(item);
            let desired = value(item);
            let location = source(item).map_or_else(
                || format!("{} entry {}", self.source, index.saturating_add(1)),
                str::to_owned,
            );
            if let Some((previous_value, previous_location)) = seen.get(&key) {
                if previous_value != &desired {
                    self.diagnostics.push(Diagnostic::error(
                        self.source,
                        key,
                        code,
                        format!(
                            "conflicting desired values at {previous_location} and {location}; keep only one desired value for this target"
                        ),
                    ));
                }
            } else {
                seen.insert(key, (desired, location));
            }
        }
        self
    }

    /// Consume the builder and return the collected diagnostics.
    #[must_use]
    pub(crate) fn finish(self) -> Vec<Diagnostic> {
        self.diagnostics
    }
}

/// Return a [`Severity::Warning`] [`CheckItem`] if `condition` is `true`.
///
/// Designed for use inside [`Validator::check_each`] closures:
///
/// ```ignore
/// check(name.is_empty(), "package.empty-name", "name is empty")
/// ```
#[must_use]
pub(crate) fn check(
    condition: bool,
    code: DiagnosticCode,
    message: impl Into<String>,
) -> CheckItem {
    condition.then(|| (code, Severity::Warning, message.into()))
}

/// Return a [`Severity::Error`] [`CheckItem`] if `condition` is `true`.
///
/// Use for rules that represent structurally invalid or unsafe configuration
/// (e.g., path traversal components, unsafe file names).
#[must_use]
pub(crate) fn check_error(
    condition: bool,
    code: DiagnosticCode,
    message: impl Into<String>,
) -> CheckItem {
    condition.then(|| (code, Severity::Error, message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Item {
        label: &'static str,
        value: i32,
    }

    #[test]
    fn check_returns_warning_item_when_condition_is_true() {
        let result = check(true, DiagnosticCode::new("test", "rule"), "invalid value");
        assert_eq!(
            result,
            Some((
                DiagnosticCode::new("test", "rule"),
                Severity::Warning,
                "invalid value".to_string()
            ))
        );
    }

    #[test]
    fn check_returns_none_when_condition_is_false() {
        assert_eq!(
            check(false, DiagnosticCode::new("test", "rule"), "invalid value"),
            None
        );
    }

    #[test]
    fn conflicts_aggregate_against_the_first_declaration_without_exposing_values() {
        let items = [
            ("a", "first-private-value", Some("main.toml [base] entry 1")),
            ("a", "first-private-value", Some("main.toml [base] entry 2")),
            ("b", "unrelated-private-value", None),
            (
                "a",
                "second-private-value",
                Some("overlay.toml [base] entry 1"),
            ),
            ("a", "third-private-value", None),
        ];
        let diagnostics = Validator::new("settings.toml")
            .check_conflicts(
                &items,
                DiagnosticCode::new("test", "conflicting-values"),
                |item| item.0.to_string(),
                |item| item.1.to_string(),
                |item| item.2,
            )
            .finish();
        assert_eq!(
            diagnostics.len(),
            2,
            "all conflicting declarations should be reported"
        );
        for diagnostic in &diagnostics {
            assert_eq!(diagnostic.item, "a");
            assert_eq!(diagnostic.severity, Severity::Error);
            assert!(diagnostic.message.contains("main.toml [base] entry 1"));
            assert!(
                !diagnostic.message.contains("private-value"),
                "do not print setting values"
            );
        }
        assert!(
            diagnostics[0]
                .message
                .contains("overlay.toml [base] entry 1")
        );
        assert!(diagnostics[1].message.contains("settings.toml entry 5"));
    }

    #[test]
    fn check_error_returns_error_item_when_condition_is_true() {
        let result = check_error(true, DiagnosticCode::new("test", "unsafe"), "unsafe path");
        assert_eq!(
            result,
            Some((
                DiagnosticCode::new("test", "unsafe"),
                Severity::Error,
                "unsafe path".to_string()
            ))
        );
    }

    #[test]
    fn check_error_returns_none_when_condition_is_false() {
        assert_eq!(
            check_error(false, DiagnosticCode::new("test", "unsafe"), "unsafe path"),
            None
        );
    }

    #[test]
    fn warn_records_source_item_code_and_message() {
        let mut validator = Validator::new("example.toml");

        validator.warn(DiagnosticCode::new("test", "rule"), "item-a", "is invalid");

        assert_eq!(
            validator.finish(),
            vec![Diagnostic::warning(
                "example.toml",
                "item-a",
                DiagnosticCode::new("test", "rule"),
                "is invalid"
            )],
        );
    }

    #[test]
    fn warn_if_records_only_true_conditions() {
        let validator = Validator::new("example.toml")
            .warn_if(
                false,
                DiagnosticCode::new("test", "rule"),
                "item-a",
                "should not appear",
            )
            .warn_if(
                true,
                DiagnosticCode::new("test", "rule"),
                "item-b",
                "should appear",
            );

        assert_eq!(
            validator.finish(),
            vec![Diagnostic::warning(
                "example.toml",
                "item-b",
                DiagnosticCode::new("test", "rule"),
                "should appear"
            )],
        );
    }

    #[test]
    fn check_each_collects_all_some_items_and_ignores_none() {
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

        let diagnostics = Validator::new("numbers.toml")
            .check_each(
                &items,
                |item| item.label,
                |item| {
                    [
                        check(
                            item.value < 0,
                            DiagnosticCode::new("test", "negative"),
                            "must be non-negative",
                        ),
                        check(
                            item.value == 0,
                            DiagnosticCode::new("test", "zero"),
                            "must be non-zero",
                        ),
                        check(
                            item.value > 10,
                            DiagnosticCode::new("test", "too-large"),
                            "must be at most 10",
                        ),
                    ]
                },
            )
            .finish();

        assert_eq!(
            diagnostics,
            vec![
                Diagnostic::warning(
                    "numbers.toml",
                    "negative",
                    DiagnosticCode::new("test", "negative"),
                    "must be non-negative"
                ),
                Diagnostic::warning(
                    "numbers.toml",
                    "zero",
                    DiagnosticCode::new("test", "zero"),
                    "must be non-zero"
                ),
                Diagnostic::warning(
                    "numbers.toml",
                    "large",
                    DiagnosticCode::new("test", "too-large"),
                    "must be at most 10"
                ),
            ],
        );
    }

    #[test]
    fn check_each_emits_error_severity_for_check_error_results() {
        let items = [Item {
            label: "traversal",
            value: 0,
        }];

        let diagnostics = Validator::new("paths.toml")
            .check_each(
                &items,
                |item| item.label,
                |item| {
                    [check_error(
                        item.value == 0,
                        DiagnosticCode::new("test", "unsafe"),
                        "unsafe path",
                    )]
                },
            )
            .finish();

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(diagnostics[0].code.to_string(), "test.unsafe");
    }

    #[test]
    fn check_each_can_be_chained_after_standalone_warnings() {
        let mut validator = Validator::new("numbers.toml");
        validator.warn(
            DiagnosticCode::new("test", "global"),
            "file",
            "global warning",
        );

        let diagnostics = validator
            .check_each(
                &[Item {
                    label: "negative",
                    value: -5,
                }],
                |item| item.label,
                |item| {
                    [check(
                        item.value < 0,
                        DiagnosticCode::new("test", "negative"),
                        "must be non-negative",
                    )]
                },
            )
            .finish();

        assert_eq!(
            diagnostics,
            vec![
                Diagnostic::warning(
                    "numbers.toml",
                    "file",
                    DiagnosticCode::new("test", "global"),
                    "global warning"
                ),
                Diagnostic::warning(
                    "numbers.toml",
                    "negative",
                    DiagnosticCode::new("test", "negative"),
                    "must be non-negative"
                ),
            ],
        );
    }

    #[test]
    fn severity_label_returns_ascii_short_codes() {
        assert_eq!(Severity::Warning.label(), "warn");
        assert_eq!(Severity::Error.label(), "err");
    }
}
