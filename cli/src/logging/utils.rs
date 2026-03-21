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
/// Queries the actual terminal size via ioctl (Unix) or the console API
/// (Windows), falling back to the `COLUMNS` environment variable, then 80.
pub(super) fn terminal_columns() -> usize {
    terminal_columns_with(std::env::var("COLUMNS").ok())
}

/// Inner implementation of [`terminal_columns`] that accepts the `COLUMNS`
/// environment variable value as a parameter so tests can exercise the
/// fallback logic without mutating process-global state.
pub(super) fn terminal_columns_with(columns_env: Option<String>) -> usize {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .filter(|&n| n > 0)
        .or_else(|| {
            columns_env
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|&n| n > 0)
        })
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
    dotfiles_cache_subdir(&cache_dir)
}

/// Return `<base>/dotfiles/`, creating it if needed.
///
/// Extracted from [`dotfiles_cache_dir`] so that callers (especially tests)
/// can supply an explicit base path without manipulating environment
/// variables.
pub(super) fn dotfiles_cache_subdir(base: &std::path::Path) -> Option<PathBuf> {
    let dir = base.join("dotfiles");
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Return the log file path under `$XDG_CACHE_HOME/dotfiles/` (or `~/.cache/dotfiles/`).
pub(super) fn log_file_path(command: &str) -> Option<PathBuf> {
    Some(dotfiles_cache_dir()?.join(format!("{command}.log")))
}

/// Return the log file path under `<base>/dotfiles/`.
///
/// Like [`log_file_path`] but uses an explicit base directory instead of
/// reading `XDG_CACHE_HOME` from the environment.
#[cfg(test)]
pub(super) fn log_file_path_in(command: &str, base: &std::path::Path) -> Option<PathBuf> {
    Some(dotfiles_cache_subdir(base)?.join(format!("{command}.log")))
}

/// Decompose seconds since the Unix epoch into `(year, month, day, hour, min, sec)`.
///
/// Uses Howard Hinnant's civil-from-days algorithm.
#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation
)]
fn civil_from_epoch_secs(epoch_secs: u64) -> (i32, u32, u32, u32, u32, u32) {
    let day_secs = (epoch_secs % 86_400) as u32;
    let hour = day_secs / 3600;
    let min = (day_secs % 3600) / 60;
    let sec = day_secs % 60;

    let z = (epoch_secs / 86_400) as i64 + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = i64::from(yoe) + era * 400 + i64::from(m <= 2);

    (y as i32, m, d, hour, min, sec)
}

/// Format the current UTC time as `YYYY-MM-DDTHH:MM:SS.ffffffZ` (microsecond precision).
pub(super) fn format_utc_datetime_us() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let (y, mo, d, h, mi, s) = civil_from_epoch_secs(dur.as_secs());
    format!(
        "{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{:06}Z",
        dur.subsec_micros()
    )
}

/// Format the current UTC time as `YYYY-MM-DD HH:MM:SS`.
pub(super) fn format_utc_datetime() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, mo, d, h, mi, s) = civil_from_epoch_secs(secs);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
}

/// Format the current UTC time as `HH:MM:SS`.
pub(super) fn format_utc_time() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let day_secs = secs % 86_400;
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    let s = day_secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

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
    fn terminal_columns_reads_env_var_as_fallback() {
        // Use the parameterized variant to test the env fallback path
        // without mutating process-global state.
        let has_tty = terminal_size::terminal_size().is_some();
        let cols = terminal_columns_with(Some("120".to_string()));
        if has_tty {
            // ioctl takes priority when a real TTY is attached.
            assert!(cols > 0);
        } else {
            assert_eq!(cols, 120);
        }
    }

    #[test]
    fn terminal_columns_ignores_zero() {
        let has_tty = terminal_size::terminal_size().is_some();
        let cols = terminal_columns_with(Some("0".to_string()));
        if has_tty {
            assert!(cols > 0);
        } else {
            assert_eq!(cols, 80, "zero COLUMNS should fall back to 80");
        }
    }

    #[test]
    fn terminal_columns_with_none_falls_back_to_default() {
        let has_tty = terminal_size::terminal_size().is_some();
        let cols = terminal_columns_with(None);
        if has_tty {
            assert!(cols > 0);
        } else {
            assert_eq!(cols, 80, "absent COLUMNS should fall back to 80");
        }
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
