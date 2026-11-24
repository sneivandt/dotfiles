use std::{
    fs,
    io::{BufRead as _, Read as _},
    path::Path,
};

/// File extension used for run logs.
const LOG_EXTENSION: &str = "log";

/// Components of a run-log file name.
///
/// Names are `<stamp>-<command>-<pid>.log`, e.g.
/// `20260731T154210Z-install-48213.log`. The pid disambiguates runs that start
/// within the same second; the header's microsecond timestamp orders them.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RunLogName<'a> {
    /// Compact UTC start stamp, `YYYYMMDDTHHMMSSZ`.
    pub(crate) stamp: &'a str,
    /// Command that produced the run.
    pub(crate) command: &'a str,
}

/// Build the run-log file name for a command.
pub(super) fn run_log_file_name(command: &str, stamp: &str, pid: u32) -> String {
    format!("{stamp}-{command}-{pid}.{LOG_EXTENSION}")
}

/// Parse a run-log file name, or return `None` if it is not one.
///
/// Split the timestamp from the left and PID from the right so hyphenated
/// diagnostic/test command names round-trip. The timestamp must match the
/// fixed-width `YYYYMMDDTHHMMSSZ` format.
/// Files that do not match are ignored rather than pruned or listed, so
/// unrelated files in the log directory are left alone.
pub(crate) fn parse_run_log_file_name(name: &str) -> Option<RunLogName<'_>> {
    let stem = name.strip_suffix(".log")?;
    let (stamp, rest) = stem.split_once('-')?;
    let (command, pid) = rest.rsplit_once('-')?;
    if command.is_empty()
        || !command
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        || stamp.len() != 16
        || !stamp.bytes().enumerate().all(|(index, byte)| match index {
            8 => byte == b'T',
            15 => byte == b'Z',
            _ => byte.is_ascii_digit(),
        })
    {
        return None;
    }
    pid.parse::<u32>().ok()?;
    Some(RunLogName { stamp, command })
}

/// Sortable start timestamp, using the precise header when available.
///
/// Legacy, incomplete, or unreadable logs fall back to their filename stamp.
/// Never use modification times: an older run may still be writing its log.
pub(crate) fn run_log_sort_stamp(path: &Path, filename_stamp: &str) -> String {
    sort_stamp(read_dotfiles_header(path).as_deref(), filename_stamp)
}

#[cfg(unix)]
const HEADER_OPEN_FLAGS: i32 = nix::libc::O_NONBLOCK | nix::libc::O_NOFOLLOW;

pub(super) fn read_dotfiles_header(path: &Path) -> Option<String> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(HEADER_OPEN_FLAGS);
    }
    let file = options.open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut header = String::new();
    // A damaged or legacy first line must not require reading the whole log.
    std::io::BufReader::new(file.take(512))
        .read_line(&mut header)
        .ok()?;
    header.starts_with("# Dotfiles ").then_some(header)
}

pub(super) fn sort_stamp(header: Option<&str>, filename_stamp: &str) -> String {
    header
        .and_then(precise_header_stamp)
        .unwrap_or_else(|| format!("{}.000000Z", filename_stamp.trim_end_matches('Z')))
}

fn precise_header_stamp(header: &str) -> Option<String> {
    let stamp = header.split_whitespace().next_back()?;
    if stamp.len() != 27
        || !stamp.bytes().enumerate().all(|(index, byte)| match index {
            4 | 7 => byte == b'-',
            10 => byte == b'T',
            13 | 16 => byte == b':',
            19 => byte == b'.',
            26 => byte == b'Z',
            _ => byte.is_ascii_digit(),
        })
    {
        return None;
    }
    Some(stamp.replace(['-', ':'], ""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_run_log_file_name_rejects_empty_command_and_invalid_chars() {
        assert!(parse_run_log_file_name("20260101T000000Z--123.log").is_none());
        assert!(parse_run_log_file_name("20260101T000000Z-bad_cmd-123.log").is_none());
        let parsed = parse_run_log_file_name("20260101T000000Z-update-123.log").unwrap();
        assert_eq!(parsed.stamp, "20260101T000000Z");
        assert_eq!(parsed.command, "update");
    }

    #[cfg(unix)]
    #[test]
    fn read_dotfiles_header_sets_nonblock_and_rejects_symlinks() {
        assert_ne!(HEADER_OPEN_FLAGS & nix::libc::O_NOFOLLOW, 0);
        assert_ne!(HEADER_OPEN_FLAGS & nix::libc::O_NONBLOCK, 0);
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.log");
        fs::write(&target, "# Dotfiles update 2026-01-01T00:00:00.123456Z\n").unwrap();
        let link = dir.path().join("link.log");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(read_dotfiles_header(&target).is_some());
        assert!(read_dotfiles_header(&link).is_none());
    }
}
