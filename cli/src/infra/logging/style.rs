//! Central console styling policy for user-facing log output.

use std::io::IsTerminal as _;

use super::utils::strip_ansi;

/// Explicit or automatic ANSI color selection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ColorMode {
    /// Enable ANSI on terminals (or when `CLICOLOR_FORCE` is set) unless `NO_COLOR` is set.
    #[default]
    Auto,
    /// Always emit ANSI styling, overriding terminal detection and `NO_COLOR`.
    Always,
    /// Never emit ANSI styling, overriding terminal detection and `CLICOLOR_FORCE`.
    Never,
}

/// Whether ANSI styling should be emitted for a console stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StyleChoice {
    ansi: bool,
}

/// Text styles used by the logging UI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TextStyle {
    Bold,
    Dim,
    Red,
    Yellow,
    Green,
    Magenta,
    Cyan,
}

impl TextStyle {
    const fn ansi_code(self) -> &'static str {
        match self {
            Self::Bold => "1",
            Self::Dim => "2",
            Self::Red => "31",
            Self::Yellow => "33",
            Self::Green => "32",
            Self::Magenta => "35",
            Self::Cyan => "36",
        }
    }
}

impl StyleChoice {
    /// Resolve styling from an explicit color mode, terminal detection, and environment signals.
    ///
    /// Explicit `--color always|never` overrides environment variables and terminal detection.
    /// Under `Auto`, `NO_COLOR` takes precedence over `CLICOLOR_FORCE` and terminal detection.
    #[must_use]
    pub(crate) const fn resolve(
        mode: ColorMode,
        is_terminal: bool,
        no_color: bool,
        clicolor_force: bool,
    ) -> Self {
        let ansi = match mode {
            ColorMode::Always => true,
            ColorMode::Never => false,
            ColorMode::Auto => !no_color && (clicolor_force || is_terminal),
        };
        Self { ansi }
    }

    /// Create a style policy from `color=auto` inputs.
    #[must_use]
    pub(crate) const fn auto(is_terminal: bool, no_color: bool) -> Self {
        Self::resolve(ColorMode::Auto, is_terminal, no_color, false)
    }

    /// Style policy that emits ANSI. Used by tests for deterministic formatting.
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn colored() -> Self {
        Self { ansi: true }
    }

    /// Style policy that strips ANSI. Used by tests for deterministic formatting.
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn plain() -> Self {
        Self { ansi: false }
    }

    /// Return whether this policy emits ANSI styling.
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn is_ansi_enabled(self) -> bool {
        self.ansi
    }

    /// Apply a text style, or return clean plain text when styling is disabled.
    #[must_use]
    pub(crate) fn paint(self, style: TextStyle, text: &str) -> String {
        if self.ansi {
            format!("\x1b[{}m{text}\x1b[0m", style.ansi_code())
        } else {
            strip_ansi(text)
        }
    }

    /// Emphasize an interactive option using the same stream styling policy.
    pub(crate) fn bold(self, text: &str) -> String {
        self.paint(TextStyle::Bold, text)
    }

    /// Combine emphasis and color in one escape so resets do not cancel bold.
    #[must_use]
    pub(crate) fn paint_bold(self, style: TextStyle, text: &str) -> String {
        if self.ansi {
            format!("\x1b[1;{}m{text}\x1b[0m", style.ansi_code())
        } else {
            strip_ansi(text)
        }
    }

    /// Preserve existing ANSI only when this stream supports styling.
    #[must_use]
    pub(crate) fn clean(self, text: &str) -> String {
        if self.ansi {
            text.to_string()
        } else {
            strip_ansi(text)
        }
    }
}

#[must_use]
pub(crate) fn stdout_style_for(mode: ColorMode) -> StyleChoice {
    StyleChoice::resolve(
        mode,
        std::io::stdout().is_terminal(),
        no_color_enabled(),
        clicolor_force_enabled(),
    )
}

#[must_use]
pub(crate) fn stderr_style_for(mode: ColorMode) -> StyleChoice {
    StyleChoice::resolve(
        mode,
        std::io::stderr().is_terminal(),
        no_color_enabled(),
        clicolor_force_enabled(),
    )
}

fn no_color_enabled() -> bool {
    env_disables_color(std::env::var_os("NO_COLOR").as_deref())
}

fn clicolor_force_enabled() -> bool {
    env_forces_color(std::env::var_os("CLICOLOR_FORCE").as_deref())
}

fn env_disables_color(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|value| !value.is_empty())
}

fn env_forces_color(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|value| !value.is_empty() && value != "0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_disables_color_requires_non_empty_value() {
        assert!(!env_disables_color(None));
        assert!(!env_disables_color(Some(std::ffi::OsStr::new(""))));
        assert!(env_disables_color(Some(std::ffi::OsStr::new("1"))));
    }

    #[test]
    fn env_forces_color_requires_non_empty_non_zero_value() {
        assert!(!env_forces_color(None));
        assert!(!env_forces_color(Some(std::ffi::OsStr::new(""))));
        assert!(!env_forces_color(Some(std::ffi::OsStr::new("0"))));
        assert!(env_forces_color(Some(std::ffi::OsStr::new("1"))));
        assert!(env_forces_color(Some(std::ffi::OsStr::new("true"))));
    }

    #[test]
    fn resolve_follows_cli_flag_no_color_clicolor_force_and_tty_precedence() {
        for is_terminal in [false, true] {
            for no_color in [false, true] {
                for clicolor_force in [false, true] {
                    assert!(
                        StyleChoice::resolve(
                            ColorMode::Always,
                            is_terminal,
                            no_color,
                            clicolor_force
                        )
                        .is_ansi_enabled()
                    );
                    assert!(
                        !StyleChoice::resolve(
                            ColorMode::Never,
                            is_terminal,
                            no_color,
                            clicolor_force
                        )
                        .is_ansi_enabled()
                    );
                    let expected_auto = !no_color && (clicolor_force || is_terminal);
                    assert_eq!(
                        StyleChoice::resolve(
                            ColorMode::Auto,
                            is_terminal,
                            no_color,
                            clicolor_force
                        )
                        .is_ansi_enabled(),
                        expected_auto,
                        "tty={is_terminal}, no_color={no_color}, force={clicolor_force}"
                    );
                }
            }
        }
    }

    #[test]
    fn auto_enables_ansi_only_for_terminal_without_no_color() {
        assert!(StyleChoice::auto(true, false).is_ansi_enabled());
        assert!(!StyleChoice::auto(false, false).is_ansi_enabled());
        assert!(!StyleChoice::auto(true, true).is_ansi_enabled());
    }

    #[test]
    fn plain_paint_strips_embedded_ansi() {
        assert_eq!(
            StyleChoice::plain().paint(TextStyle::Green, "\x1b[31mred\x1b[0m"),
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
