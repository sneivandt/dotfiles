//! Structured configuration diagnostics shared by config loaders, validators,
//! and validation tasks.

use std::fmt;

/// Severity level of a configuration diagnostic.
///
/// Severity affects rendering and metadata only — both variants cause
/// `dotfiles check` to fail when any diagnostic is present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// A suspicious or suboptimal configuration value.
    Warning,
    /// Structurally invalid or unsafe configuration that will likely cause
    /// failures or unsafe behaviour at apply time.
    Error,
}

impl Severity {
    /// Short ASCII label used in diagnostic output (`"warn"` or `"err"`).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Warning => "warn",
            Self::Error => "err",
        }
    }
}

/// A stable machine-readable rule code, rendered as `domain.rule`.
///
/// Diagnostic codes appear in `dotfiles check` output and are therefore a
/// de-facto public contract: they must stay stable across releases so that
/// users can grep for or suppress a specific finding.
///
/// The code is a distinct type rather than a bare `&str` so that a message can
/// never be passed where a code is expected, and so that the two-part
/// `domain.rule` shape is enforced at construction. Each domain declares its
/// own codes as constants next to the validator that raises them, which keeps
/// rule ownership with the domain that defines the rule.
///
/// ```ignore
/// const INVALID_MODE: DiagnosticCode = DiagnosticCode::new("chmod", "invalid-mode");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DiagnosticCode {
    /// Config domain the rule belongs to (e.g. `"chmod"`, `"symlink"`).
    domain: &'static str,
    /// Kebab-case rule name within the domain (e.g. `"invalid-mode"`).
    rule: &'static str,
}

impl DiagnosticCode {
    /// Declare a diagnostic code.
    #[must_use]
    pub const fn new(domain: &'static str, rule: &'static str) -> Self {
        Self { domain, rule }
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.domain, self.rule)
    }
}

/// A structured diagnostic emitted during configuration validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Configuration source file (e.g., `"symlinks.toml"`, `"packages.toml"`).
    pub source: String,
    /// The specific item or section that triggered the finding.
    pub item: String,
    /// Severity of the finding.
    pub severity: Severity,
    /// Stable machine-readable rule code (e.g. `package.empty-name`).
    pub code: DiagnosticCode,
    /// Human-readable description.
    pub message: String,
}

impl Diagnostic {
    /// Create a diagnostic with explicit severity and code.
    #[must_use]
    pub fn new(
        source: impl Into<String>,
        item: impl Into<String>,
        severity: Severity,
        code: DiagnosticCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            source: source.into(),
            item: item.into(),
            severity,
            code,
            message: message.into(),
        }
    }

    /// Create a [`Severity::Warning`] diagnostic.
    #[must_use]
    pub fn warning(
        source: impl Into<String>,
        item: impl Into<String>,
        code: DiagnosticCode,
        message: impl Into<String>,
    ) -> Self {
        Self::new(source, item, Severity::Warning, code, message)
    }

    /// Create a [`Severity::Error`] diagnostic.
    #[must_use]
    pub fn error(
        source: impl Into<String>,
        item: impl Into<String>,
        code: DiagnosticCode,
        message: impl Into<String>,
    ) -> Self {
        Self::new(source, item, Severity::Error, code, message)
    }
}
