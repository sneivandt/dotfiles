//! Structured configuration diagnostics shared by config loaders, validators,
//! and validation tasks.

/// Severity level of a configuration diagnostic.
///
/// Severity affects rendering and metadata only — both variants cause
/// `dotfiles test` to fail when any diagnostic is present.
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

/// A structured diagnostic emitted during configuration validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Configuration source file (e.g., `"symlinks.toml"`, `"packages.toml"`).
    pub source: String,
    /// The specific item or section that triggered the finding.
    pub item: String,
    /// Severity of the finding.
    pub severity: Severity,
    /// Stable machine-readable rule code (e.g., `"package.empty-name"`).
    pub code: &'static str,
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
        code: &'static str,
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
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self::new(source, item, Severity::Warning, code, message)
    }

    /// Create a [`Severity::Error`] diagnostic.
    #[must_use]
    pub fn error(
        source: impl Into<String>,
        item: impl Into<String>,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self::new(source, item, Severity::Error, code, message)
    }
}
