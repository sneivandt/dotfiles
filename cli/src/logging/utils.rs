//! Utility functions for path resolution, ANSI stripping, and time formatting.
use std::fs;
use std::path::PathBuf;

/// Strip ANSI escape sequences from a string.
///
/// Handles SGR sequences (ending in `m`) and other CSI sequences (ending
/// in any letter in the `@`..`~` range), so cursor movement, erase, etc.
/// are also stripped without consuming unrelated text.
pub(super) fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if let Some(next) = chars.next()
                && next == '['
            {
                for inner in chars.by_ref() {
                    if ('@'..='~').contains(&inner) {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Return the terminal width in columns.
///
/// Reads the `COLUMNS` environment variable, falling back to 80 if unset
/// or unparseable.
pub(super) fn terminal_columns() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(80)
}

/// Return the `$XDG_CACHE_HOME/dotfiles/` directory, creating it if needed.
pub(super) fn dotfiles_cache_dir() -> Option<PathBuf> {
    let cache_dir = std::env::var("XDG_CACHE_HOME").map_or_else(
        |_| {
            std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .map_or_else(|_| PathBuf::from("."), PathBuf::from)
                .join(".cache")
        },
        PathBuf::from,
    );
    let dir = cache_dir.join("dotfiles");
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Return the log file path under `$XDG_CACHE_HOME/dotfiles/` (or `~/.cache/dotfiles/`).
pub(super) fn log_file_path(command: &str) -> Option<PathBuf> {
    Some(dotfiles_cache_dir()?.join(format!("{command}.log")))
}

/// Return the diagnostic log file path under `$XDG_CACHE_HOME/dotfiles/`.
pub(super) fn diag_log_file_path(command: &str) -> Option<PathBuf> {
    Some(dotfiles_cache_dir()?.join(format!("{command}.diag.log")))
}

/// Format the current UTC time as `YYYY-MM-DDTHH:MM:SS.ffffffZ` (microsecond precision).
pub(super) fn format_utc_datetime_us() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.6fZ")
        .to_string()
}

/// Format the current UTC time as `YYYY-MM-DD HH:MM:SS`.
pub(super) fn format_utc_datetime() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Format the current UTC time as `HH:MM:SS`.
pub(super) fn format_utc_time() -> String {
    chrono::Utc::now().format("%H:%M:%S").to_string()
}

#[cfg(test)]
#[allow(unsafe_code)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn strip_ansi_removes_colors() {
        assert_eq!(strip_ansi("\x1b[31mERROR\x1b[0m hello"), "ERROR hello");
        assert_eq!(strip_ansi("no codes here"), "no codes here");
        assert_eq!(
            strip_ansi("\x1b[1;34m==>\x1b[0m \x1b[1mstage\x1b[0m"),
            "==> stage"
        );
    }

    #[test]
    fn strip_ansi_handles_csi_sequences() {
        assert_eq!(strip_ansi("\x1b[2;5Htext"), "text");
        assert_eq!(strip_ansi("\x1b[2Jhello"), "hello");
        assert_eq!(strip_ansi("\x1b[Kworld"), "world");
        assert_eq!(strip_ansi("\x1b[31m\x1b[2JERROR\x1b[0m"), "ERROR");
        assert_eq!(strip_ansi("\x1bMtext"), "text");
        assert_eq!(strip_ansi("\x1b7text"), "text");
        assert_eq!(strip_ansi("\x1b8text"), "text");
    }

    #[test]
    fn strip_ansi_empty_string() {
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn terminal_columns_returns_positive() {
        let cols = terminal_columns();
        assert!(
            cols > 0,
            "terminal_columns should always return a positive value"
        );
    }

    #[test]
    #[allow(unsafe_code)]
    fn terminal_columns_reads_env_var() {
        let _lock = ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        unsafe {
            std::env::set_var("COLUMNS", "120");
        }
        let cols = terminal_columns();
        unsafe {
            std::env::remove_var("COLUMNS");
        }
        assert_eq!(cols, 120);
    }

    #[test]
    #[allow(unsafe_code)]
    fn terminal_columns_ignores_zero() {
        let _lock = ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        unsafe {
            std::env::set_var("COLUMNS", "0");
        }
        let cols = terminal_columns();
        unsafe {
            std::env::remove_var("COLUMNS");
        }
        assert_eq!(cols, 80, "zero COLUMNS should fall back to 80");
    }

    #[test]
    fn format_utc_datetime_us_has_microseconds() {
        let s = format_utc_datetime_us();
        assert!(s.ends_with('Z'), "should end with Z");
        assert!(s.contains('T'), "should contain T separator");
        // Find the decimal point and check 6 digits follow it
        let dot_pos = s.find('.').expect("should have decimal point");
        let after_dot = &s[dot_pos + 1..s.len() - 1]; // strip trailing Z
        assert_eq!(
            after_dot.len(),
            6,
            "should have 6 decimal digits for microseconds"
        );
    }

    #[test]
    fn format_utc_time_has_correct_format() {
        let s = format_utc_time();
        assert_eq!(s.len(), 8, "HH:MM:SS should be 8 chars");
        assert_eq!(&s[2..3], ":", "colon at position 2");
        assert_eq!(&s[5..6], ":", "colon at position 5");
    }

    #[test]
    fn format_utc_datetime_has_correct_format() {
        let s = format_utc_datetime();
        assert_eq!(s.len(), 19, "YYYY-MM-DD HH:MM:SS should be 19 chars");
        assert_eq!(&s[4..5], "-", "dash at position 4");
        assert_eq!(&s[7..8], "-", "dash at position 7");
        assert_eq!(&s[10..11], " ", "space at position 10");
    }
}
