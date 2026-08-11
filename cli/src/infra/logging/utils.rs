//! Utility functions for path resolution, ANSI stripping, and time formatting.
use std::fs;
use std::path::PathBuf;

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

/// Whether a line is a task's aggregate counter summary (`"3 changed, 10
/// already ok"`).
///
/// These restate what the status row and the per-action lines already show, so
/// they are dropped from console output in every mode and kept in the run log.
pub(super) fn is_stats_summary(line: &str) -> bool {
    let Some((first, rest)) = line.split_once(' ') else {
        return false;
    };
    first.parse::<u32>().is_ok()
        && (rest.starts_with("changed, ") || rest.starts_with("would change, "))
        && rest.contains(" already ok")
}

/// Verbs used to introduce the per-item action lines a task emits.
const ACTION_VERBS: &[&str] = &["configure", "install", "link", "ok", "remove", "update"];

/// Rewrite a task's raw action line into its compact console form.
///
/// Tasks phrase their own output naturally (`would link: …`, `linked: …`,
/// `ok: …`), which reads well in the run log but is noisy in a column of
/// otherwise-identical rows. Collapsing every tense onto one imperative verb
/// keeps the console rows aligned and lets the summary status row carry the
/// tense instead.
pub(super) fn compact_detail_line(line: &str) -> String {
    const ACTION_PREFIXES: &[(&str, &str)] = &[
        ("would configure: ", "configure"),
        ("would install: ", "install"),
        ("would link: ", "link"),
        ("would remove: ", "remove"),
        ("would update: ", "update"),
        ("configured: ", "configure"),
        ("installed: ", "install"),
        ("linked: ", "link"),
        ("ok: ", "ok"),
        ("removed: ", "remove"),
        ("updated: ", "update"),
    ];

    let line = line.trim_start();
    for (prefix, verb) in ACTION_PREFIXES {
        if let Some(detail) = line.strip_prefix(prefix) {
            return format!("{verb} {detail}");
        }
    }
    for verb in ACTION_VERBS {
        if let Some(detail) = line
            .strip_prefix(*verb)
            .and_then(|rest| rest.strip_prefix(' '))
        {
            return format!("{verb} {detail}");
        }
    }
    line.to_string()
}

/// Whether a line is one of the per-item action lines a task emits.
///
/// Tested against the compact form so every tense of the same action
/// (`would link: …`, `linked: …`, `link …`) is recognised alike.
fn is_action_line(line: &str) -> bool {
    let compact = compact_detail_line(line);
    ACTION_VERBS.iter().any(|verb| {
        compact
            .strip_prefix(*verb)
            .is_some_and(|rest| rest.starts_with(' '))
    })
}

/// Sort each maximal run of consecutive per-item action lines in place.
///
/// Parallel resource processing completes in a nondeterministic order, so the
/// same run can print the same items in a different order each time. Sorting
/// makes consecutive runs diffable against each other.
///
/// Ordering uses the compact console form, so rows sort by what the reader
/// sees rather than by the tense the task happened to phrase them in.
///
/// Only runs of action lines are reordered. Any other line (a mode note such
/// as `using winget package manager`, or a warning) acts as a barrier and keeps
/// its position, so lines whose order carries meaning are left alone.
pub(super) fn sort_action_runs<T>(items: &mut [T], text: impl Fn(&T) -> &str) {
    let mut start = 0;
    let mut index = 0;
    while index <= items.len() {
        if !items
            .get(index)
            .is_some_and(|item| is_action_line(text(item)))
        {
            if let Some(run) = items.get_mut(start..index) {
                run.sort_by_cached_key(|item| compact_detail_line(text(item)));
            }
            start = index.saturating_add(1);
        }
        index = index.saturating_add(1);
    }
}

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
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
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
    log_dir_from(
        std::env::var("DOTFILES_LOG_DIR").ok(),
        std::env::var("LOCALAPPDATA").ok(),
        std::env::var("XDG_STATE_HOME").ok(),
        std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok(),
    )
}

/// Inner implementation of [`log_dir_path`] that accepts the relevant
/// environment variables as parameters so tests can exercise every branch
/// without mutating process-global state.
///
/// `LOCALAPPDATA` is only set on Windows and `XDG_STATE_HOME` only on Unix in
/// practice, so consulting both unconditionally keeps this function free of
/// platform conditionals while still resolving natively on each platform.
fn log_dir_from(
    explicit: Option<String>,
    local_app_data: Option<String>,
    state_home: Option<String>,
    home: Option<String>,
) -> PathBuf {
    let non_empty = |value: Option<String>| value.filter(|v| !v.is_empty());
    if let Some(dir) = non_empty(explicit) {
        return PathBuf::from(dir);
    }
    non_empty(local_app_data)
        .map(PathBuf::from)
        .or_else(|| non_empty(state_home).map(PathBuf::from))
        .or_else(|| non_empty(home).map(|dir| PathBuf::from(dir).join(".local").join("state")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dotfiles")
        .join("logs")
}

/// Return `<base>/dotfiles/logs/`, creating it if needed.
///
/// Mirrors the layout produced by [`dotfiles_log_dir`] under an explicit base
/// path so tests can isolate the run log without touching the environment.
#[cfg(test)]
pub(super) fn dotfiles_log_subdir(base: &std::path::Path) -> Option<PathBuf> {
    let dir = base.join("dotfiles").join("logs");
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Remove run logs written by earlier versions under the cache directory.
///
/// Logs are state, not cache, so they moved to the state directory. The old
/// files were truncated on every run and are therefore not worth migrating.
/// Runs at most once per process and ignores every failure, so a read-only or
/// missing cache directory never affects the run.
pub(super) fn remove_legacy_cache_logs_once() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(remove_legacy_cache_logs);
}

fn remove_legacy_cache_logs() {
    let Some(dir) = legacy_cache_log_dir() else {
        return;
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("log"))
        {
            drop(fs::remove_file(&path));
        }
    }
    // Best-effort tidy-up; fails harmlessly when the directory is not empty.
    drop(fs::remove_dir(&dir));
}

/// Resolve the pre-move log directory, or `None` when it cannot be located.
///
/// Unlike the current resolver this deliberately has no relative fallback:
/// without a cache home there is nothing to clean up, and guessing would risk
/// deleting `./dotfiles/*.log` in the working directory.
fn legacy_cache_log_dir() -> Option<PathBuf> {
    let base = std::env::var("XDG_CACHE_HOME")
        .ok()
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .ok()
                .filter(|dir| !dir.is_empty())
                .map(|dir| PathBuf::from(dir).join(".cache"))
        })?;
    Some(base.join("dotfiles"))
}

/// Decompose seconds since the Unix epoch into `(year, month, day, hour, min, sec)`.
///
/// Uses Howard Hinnant's civil-from-days algorithm.
#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::as_conversions,
    clippy::arithmetic_side_effects,
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
/// Used for run-log file names. The fixed-width, zero-padded form means
/// lexical ordering of file names equals chronological ordering, so run
/// selection never depends on file modification times.
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

    #[test]
    fn compact_detail_line_normalizes_every_tense() {
        assert_eq!(
            compact_detail_line("would link: a \u{2192} b"),
            "link a \u{2192} b"
        );
        assert_eq!(
            compact_detail_line("linked: a \u{2192} b"),
            "link a \u{2192} b"
        );
        assert_eq!(
            compact_detail_line("link a \u{2192} b"),
            "link a \u{2192} b"
        );
        assert_eq!(compact_detail_line("ok: a \u{2192} b"), "ok a \u{2192} b");
        assert_eq!(compact_detail_line("  installed: pkg"), "install pkg");
        assert_eq!(
            compact_detail_line("using winget package manager"),
            "using winget package manager",
            "non-action lines must pass through untouched"
        );
    }

    #[test]
    fn sort_action_runs_orders_only_consecutive_action_lines() {
        let mut lines = vec![
            "would link: z".to_string(),
            "would link: a".to_string(),
            "using winget package manager".to_string(),
            "installed: z".to_string(),
            "installed: a".to_string(),
        ];
        sort_action_runs(&mut lines, String::as_str);
        assert_eq!(
            lines,
            vec![
                "would link: a".to_string(),
                "would link: z".to_string(),
                "using winget package manager".to_string(),
                "installed: a".to_string(),
                "installed: z".to_string(),
            ],
            "non-action lines must act as barriers and keep their position"
        );
    }

    #[test]
    fn sort_action_runs_handles_empty_and_single_item_slices() {
        let mut empty: Vec<String> = Vec::new();
        sort_action_runs(&mut empty, String::as_str);
        assert!(empty.is_empty(), "empty input must stay empty");

        let mut one = vec!["link a".to_string()];
        sort_action_runs(&mut one, String::as_str);
        assert_eq!(
            one,
            vec!["link a".to_string()],
            "single item must be stable"
        );
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
        let dir = log_dir_from(
            Some("/explicit".to_string()),
            Some("/local".to_string()),
            Some("/state".to_string()),
            Some("/home".to_string()),
        );
        assert_eq!(
            dir,
            PathBuf::from("/explicit"),
            "DOTFILES_LOG_DIR should be used verbatim"
        );
    }

    #[test]
    fn log_dir_prefers_local_app_data_then_state_home() {
        assert_eq!(
            log_dir_from(
                None,
                Some("/local".to_string()),
                Some("/state".to_string()),
                Some("/home".to_string())
            ),
            PathBuf::from("/local").join("dotfiles").join("logs"),
            "LOCALAPPDATA should win when present"
        );
        assert_eq!(
            log_dir_from(
                None,
                None,
                Some("/state".to_string()),
                Some("/home".to_string())
            ),
            PathBuf::from("/state").join("dotfiles").join("logs"),
            "XDG_STATE_HOME should be used when LOCALAPPDATA is absent"
        );
    }

    #[test]
    fn log_dir_falls_back_to_home_state_dir() {
        assert_eq!(
            log_dir_from(None, None, None, Some("/home/user".to_string())),
            PathBuf::from("/home/user")
                .join(".local")
                .join("state")
                .join("dotfiles")
                .join("logs"),
            "home should resolve to the XDG default state directory"
        );
    }

    #[test]
    fn log_dir_ignores_empty_values_and_falls_back_to_relative() {
        assert_eq!(
            log_dir_from(
                Some(String::new()),
                Some(String::new()),
                Some(String::new()),
                Some(String::new())
            ),
            PathBuf::from(".").join("dotfiles").join("logs"),
            "empty environment values should be treated as unset"
        );
    }

    #[test]
    fn log_dir_is_never_the_cache_dir() {
        let dir = log_dir_from(None, None, Some("/state".to_string()), None);
        assert!(
            !dir.to_string_lossy().contains(".cache"),
            "run logs are state, not cache: {}",
            dir.display()
        );
    }
}
