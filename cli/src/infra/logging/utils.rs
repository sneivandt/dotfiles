//! Utility functions for path resolution, ANSI stripping, and time formatting.
use std::fs;
use std::path::PathBuf;

use crate::infra::env::{Env, SystemEnv};
use crate::infra::platform::Platform;

/// Format a duration as a human-readable string (e.g., "1.2s", "2m 5s").
pub(crate) fn format_elapsed(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{:.1}s", d.as_secs_f64())
    } else {
        let mins = secs / 60;
        let remaining = secs % 60;
        format!("{mins}m {remaining}s")
    }
}

/// Whether a console line says nothing the task's own message does not.
///
/// A task that ends with a reason states that reason on its status row, so a
/// buffered line carrying the same text — bare, or behind one of the prefixes
/// tasks conventionally use — would report the same fact twice.
pub(super) fn duplicates_task_message(line: &str, task_message: Option<&str>) -> bool {
    let Some(message) = task_message else {
        return false;
    };
    if line == message {
        return true;
    }
    ["skipped: ", "skipping: ", "failed: ", "interrupted: "]
        .iter()
        .any(|prefix| line.strip_prefix(prefix) == Some(message))
}

/// Strip ANSI escape sequences from a string.
///
/// Handles CSI sequences, OSC strings terminated by BEL or ST, and
/// character-set designations and single-character escape sequences without
/// consuming visible text.
pub(super) fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.next() {
                Some('[') => {
                    for inner in chars.by_ref() {
                        if ('@'..='~').contains(&inner) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    let mut escaped = false;
                    for inner in chars.by_ref() {
                        if inner == '\x07' || (escaped && inner == '\\') {
                            break;
                        }
                        escaped = inner == '\x1b';
                    }
                }
                Some(' '..='/') => {
                    // Escape sequences may contain intermediate bytes before
                    // their final byte, e.g. ESC ( B emitted by `tput sgr0`.
                    for inner in chars.by_ref() {
                        if !(' '..='/').contains(&inner) {
                            break;
                        }
                    }
                }
                Some(_) | None => {}
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
        .map(|(w, _)| usize::from(w.0))
        .filter(|&n| n > 0)
        .or_else(|| {
            columns_env
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|&n| n > 0)
        })
        .unwrap_or(80)
}

/// Return the run-log directory, creating it if needed.
pub(super) fn dotfiles_log_dir() -> Option<PathBuf> {
    let dir = log_dir_path();
    create_log_directory(&dir).ok()?;
    Some(dir)
}

// Set permissions at creation time, including newly created parents. Existing
// directories (including explicit overrides) retain their owner's permissions.
fn create_log_directory(dir: &std::path::Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(dir)
}

/// Return the run-log directory without creating it.
pub(crate) fn dotfiles_log_dir_readonly() -> PathBuf {
    log_dir_path()
}

/// Resolve the run-log directory from the environment.
///
/// Run logs are durable state rather than regenerable cache, so they live in
/// the platform state directory: `%LOCALAPPDATA%\dotfiles\logs` on Windows and
/// `$XDG_STATE_HOME/dotfiles/logs` (default `~/.local/state/dotfiles/logs`)
/// elsewhere. `DOTFILES_LOG_DIR` overrides the whole path.
fn log_dir_path() -> PathBuf {
    log_dir_from(&SystemEnv, Platform::detect().is_windows())
}

/// Resolve native state paths without consulting the other platform's variables.
///
/// Explicit overrides may be relative; standard state/home variables must be
/// absolute. Paths retain their native encoding, including non-Unicode values.
fn log_dir_from(env: &dyn Env, windows: bool) -> PathBuf {
    if let Some(dir) = env
        .var_os("DOTFILES_LOG_DIR")
        .filter(|value| !value.is_empty())
    {
        return PathBuf::from(dir);
    }
    let absolute_path = |key| {
        env.var_os(key)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    let (state_key, home_key) = if windows {
        ("LOCALAPPDATA", "USERPROFILE")
    } else {
        ("XDG_STATE_HOME", "HOME")
    };
    absolute_path(state_key)
        .or_else(|| {
            absolute_path(home_key).map(|home| {
                if windows {
                    home.join("AppData").join("Local")
                } else {
                    home.join(".local").join("state")
                }
            })
        })
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dotfiles")
        .join("logs")
}

/// Return `<base>/dotfiles/logs/`, creating it if needed.
///
/// Mirrors the layout produced by [`dotfiles_log_dir`] under an explicit base
/// path so tests can isolate the run log without touching the environment.
pub(super) fn dotfiles_log_subdir(base: &std::path::Path) -> Option<PathBuf> {
    let dir = base.join("dotfiles").join("logs");
    create_log_directory(&dir).ok()?;
    Some(dir)
}

/// Decompose seconds since the Unix epoch into `(year, month, day, hour, min, sec)`.
///
/// Uses Howard Hinnant's civil-from-days algorithm.
#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::as_conversions,
    reason = "Hinnant civil-from-days integer algorithm; all terms bounded for valid epoch seconds"
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

/// Format the current UTC time as `YYYYMMDDTHHMMSSZ` (second precision).
///
/// Used for stable run-log file names. Headers provide the subsecond precision
/// used for ordering runs without depending on file modification times.
pub(super) fn format_utc_compact() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, mo, d, h, mi, s) = civil_from_epoch_secs(secs);
    format!("{y:04}{mo:02}{d:02}T{h:02}{mi:02}{s:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::env::MapEnv;

    #[cfg(unix)]
    #[test]
    fn new_log_directories_are_private_and_existing_parents_are_preserved() {
        use std::os::unix::fs::PermissionsExt as _;

        let fixture = tempfile::tempdir().unwrap();
        fs::set_permissions(fixture.path(), fs::Permissions::from_mode(0o755)).unwrap();
        let dir = dotfiles_log_subdir(fixture.path()).unwrap();
        for path in [&dir, &fixture.path().join("dotfiles")] {
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        assert_eq!(
            fs::metadata(fixture.path()).unwrap().permissions().mode() & 0o777,
            0o755,
            "creating logs must not chmod an existing parent"
        );
        assert_eq!(dotfiles_log_subdir(fixture.path()), Some(dir));
    }

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
    fn strip_ansi_removes_character_set_designations_without_losing_text() {
        for (input, expected) in [
            ("\x1b(B\x1b[mplain", "plain"),
            ("before\x1b(0line art\x1b(Bafter", "beforeline artafter"),
            ("\x1b%G日本語", "日本語"),
            ("before\x1b(BB after", "beforeB after"),
            ("incomplete\x1b(", "incomplete"),
        ] {
            assert_eq!(strip_ansi(input), expected, "{input:?}");
        }
    }

    #[test]
    fn strip_ansi_removes_osc_strings_and_preserves_visible_text() {
        for (name, input, expected) in [
            (
                "BEL hyperlink",
                "\x1b]8;;https://example.invalid\x07label\x1b]8;;\x07",
                "label",
            ),
            (
                "ST hyperlink",
                "\x1b]8;;https://example.invalid\x1b\\label\x1b]8;;\x1b\\",
                "label",
            ),
            ("title", "before\x1b]0;window title\x07after", "beforeafter"),
            (
                "backslash inside OSC",
                "before\x1b]0;C:\\temp\x07after",
                "beforeafter",
            ),
            (
                "escape without ST inside OSC",
                "before\x1b]0;title\x1bXsuffix\x07after",
                "beforeafter",
            ),
            (
                "mixed",
                "\x1b[31m\x1b]8;;url\x07label\x1b]8;;\x1b\\\x1b[0m",
                "label",
            ),
            (
                "unicode",
                "\x1b]8;;url\x07\u{65e5}\u{672c}\x1b]8;;\x07",
                "\u{65e5}\u{672c}",
            ),
            ("incomplete escape", "label\x1b", "label"),
            ("incomplete CSI", "label\x1b[31", "label"),
            ("incomplete OSC", "label\x1b]8;;url", "label"),
            ("incomplete ST", "label\x1b]8;;url\x1b", "label"),
            ("empty OSC", "before\x1b]\x07after", "beforeafter"),
        ] {
            assert_eq!(strip_ansi(input), expected, "{name}");
        }
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
    fn format_utc_compact_is_fixed_width_and_sortable() {
        let s = format_utc_compact();
        assert_eq!(s.len(), 16, "compact stamp should be YYYYMMDDTHHMMSSZ: {s}");
        assert!(s.ends_with('Z'), "compact stamp should end with Z: {s}");
        assert_eq!(
            s.chars().nth(8),
            Some('T'),
            "compact stamp should separate date and time with T: {s}"
        );
        assert!(
            s.chars()
                .filter(|c| *c != 'T' && *c != 'Z')
                .all(|c| c.is_ascii_digit()),
            "compact stamp should otherwise be digits: {s}"
        );
    }

    #[test]
    fn log_dir_prefers_explicit_override() {
        let root = std::path::absolute("log-path-fixture").unwrap();
        let env = MapEnv::new()
            .with("DOTFILES_LOG_DIR", "explicit relative logs")
            .with("LOCALAPPDATA", root.join("local"))
            .with("XDG_STATE_HOME", root.join("state"));
        for windows in [false, true] {
            assert_eq!(
                log_dir_from(&env, windows),
                PathBuf::from("explicit relative logs"),
                "DOTFILES_LOG_DIR should be used verbatim"
            );
        }
    }

    #[test]
    fn log_dir_uses_only_native_state_and_home_variables() {
        let root = std::path::absolute("log-path-fixture").unwrap();
        let local = root.join("local");
        let state = root.join("state");
        let userprofile = root.join("windows-home");
        let home = root.join("unix-home");
        let env = MapEnv::new()
            .with("LOCALAPPDATA", &local)
            .with("XDG_STATE_HOME", &state)
            .with("USERPROFILE", &userprofile)
            .with("HOME", &home);
        for (windows, expected, fallback) in [
            (true, local, userprofile.join("AppData").join("Local")),
            (false, state, home.join(".local").join("state")),
        ] {
            assert_eq!(
                log_dir_from(&env, windows),
                expected.join("dotfiles").join("logs")
            );
            for value in ["", "relative-state"] {
                let without_state = env
                    .clone()
                    .with("LOCALAPPDATA", value)
                    .with("XDG_STATE_HOME", value);
                assert_eq!(
                    log_dir_from(&without_state, windows),
                    fallback.join("dotfiles").join("logs"),
                    "windows={windows}, state={value:?}: use the native home fallback"
                );
            }
        }
    }

    #[test]
    fn log_dir_ignores_foreign_and_relative_standard_paths() {
        let root = std::path::absolute("log-path-fixture").unwrap();
        for (windows, state_key, home_key) in [
            (false, "LOCALAPPDATA", "USERPROFILE"),
            (true, "XDG_STATE_HOME", "HOME"),
        ] {
            let foreign = MapEnv::new().with(state_key, &root).with(home_key, &root);
            assert_eq!(
                log_dir_from(&foreign, windows),
                PathBuf::from(".").join("dotfiles").join("logs")
            );
        }
        for windows in [false, true] {
            for value in ["", "relative"] {
                let env = MapEnv::new()
                    .with("DOTFILES_LOG_DIR", "")
                    .with("LOCALAPPDATA", value)
                    .with("XDG_STATE_HOME", value)
                    .with("USERPROFILE", value)
                    .with("HOME", value);
                assert_eq!(
                    log_dir_from(&env, windows),
                    PathBuf::from(".").join("dotfiles").join("logs"),
                    "windows={windows}, value={value:?}"
                );
            }
        }
    }

    #[test]
    fn log_dir_preserves_non_unicode_environment_paths() {
        #[cfg(unix)]
        let name = {
            use std::os::unix::ffi::OsStringExt as _;
            std::ffi::OsString::from_vec(vec![0xff])
        };
        #[cfg(windows)]
        let name = {
            use std::os::windows::ffi::OsStringExt as _;
            std::ffi::OsString::from_wide(&[0xd800])
        };
        let path = std::path::absolute("log-path-fixture").unwrap().join(name);
        for windows in [false, true] {
            let explicit_env = MapEnv::new().with("DOTFILES_LOG_DIR", &path);
            assert_eq!(log_dir_from(&explicit_env, windows), path);
            let env = MapEnv::new()
                .with("LOCALAPPDATA", &path)
                .with("XDG_STATE_HOME", &path);
            assert_eq!(
                log_dir_from(&env, windows),
                path.join("dotfiles").join("logs")
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn log_dir_preserves_windows_drive_unc_and_extended_paths() {
        for path in [r"C:\", r"\\server\share", r"\\?\C:\Users\Fixture"] {
            let env = MapEnv::new().with("LOCALAPPDATA", path);
            assert_eq!(
                log_dir_from(&env, true),
                PathBuf::from(path).join("dotfiles").join("logs")
            );
        }
    }
}
