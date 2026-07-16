//! Central console styling policy for user-facing log output.

use std::io::IsTerminal as _;

use super::utils::strip_ansi;

/// Whether ANSI styling should be emitted for a console stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::infra::logging) struct StyleChoice {
    ansi: bool,
}

/// Text styles used by the logging UI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::infra::logging) enum TextStyle {
    Bold,
    Dim,
    Red,
    RedBold,
    Yellow,
    Green,
    Magenta,
}

impl TextStyle {
    const fn ansi_code(self) -> &'static str {
        match self {
            Self::Bold => "1",
            Self::Dim => "2",
            Self::Red => "31",
            Self::RedBold => "31;1",
            Self::Yellow => "33",
            Self::Green => "32",
            Self::Magenta => "35",
        }
    }
}

impl StyleChoice {
    /// Create a style policy from `color=auto` inputs.
    #[must_use]
    pub(in crate::infra::logging) const fn auto(is_terminal: bool, no_color: bool) -> Self {
        Self {
            ansi: is_terminal && !no_color,
        }
    }

    /// Style policy that emits ANSI. Used by tests for deterministic formatting.
    #[cfg(test)]
    #[must_use]
    pub(in crate::infra::logging) const fn colored() -> Self {
        Self { ansi: true }
    }

    /// Style policy that strips ANSI. Used by tests for deterministic formatting.
    #[cfg(test)]
    #[must_use]
    pub(in crate::infra::logging) const fn plain() -> Self {
        Self { ansi: false }
    }

    /// Return whether this policy emits ANSI styling.
    #[cfg(test)]
    #[must_use]
    pub(in crate::infra::logging) const fn is_ansi_enabled(self) -> bool {
        self.ansi
    }

    /// Apply a text style, or return clean plain text when styling is disabled.
    #[must_use]
    pub(in crate::infra::logging) fn paint(self, style: TextStyle, text: &str) -> String {
        if self.ansi {
            format!("\x1b[{}m{text}\x1b[0m", style.ansi_code())
        } else {
            strip_ansi(text)
        }
    }

    /// Preserve existing ANSI only when this stream supports styling.
    #[must_use]
    pub(in crate::infra::logging) fn clean(self, text: &str) -> String {
        if self.ansi {
            text.to_string()
        } else {
            strip_ansi(text)
        }
    }
}

#[must_use]
pub(in crate::infra::logging) fn stdout_style() -> StyleChoice {
    StyleChoice::auto(std::io::stdout().is_terminal(), no_color_enabled())
}

#[must_use]
pub(in crate::infra::logging) fn stderr_style() -> StyleChoice {
    StyleChoice::auto(std::io::stderr().is_terminal(), no_color_enabled())
}

fn no_color_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_enables_ansi_only_for_terminal_without_no_color() {
        assert!(StyleChoice::auto(true, false).is_ansi_enabled());
        assert!(!StyleChoice::auto(false, false).is_ansi_enabled());
        assert!(!StyleChoice::auto(true, true).is_ansi_enabled());
    }

    #[test]
    fn plain_paint_strips_embedded_ansi() {
        assert_eq!(
            StyleChoice::plain().paint(TextStyle::Bold, "\x1b[31mred\x1b[0m"),
            "red"
        );
    }

    #[test]
    fn clean_strips_existing_ansi_when_plain() {
        assert_eq!(
            StyleChoice::plain().clean("\x1b[32m3 Changed\x1b[0m"),
            "3 Changed"
        );
    }
}
