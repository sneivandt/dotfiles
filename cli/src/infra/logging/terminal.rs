//! Console capabilities used by live progress and styled output.

use std::io;

pub(super) fn columns() -> Option<usize> {
    stream_columns(&io::stdout())
        .or_else(|| stream_columns(&io::stderr()))
        .or_else(|| stream_columns(&io::stdin()))
}

#[cfg(unix)]
fn stream_columns(stream: &impl std::os::fd::AsFd) -> Option<usize> {
    let size = rustix::termios::tcgetwinsize(stream).ok()?;
    (size.ws_row > 0 && size.ws_col > 0).then_some(usize::from(size.ws_col))
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Win32 exposes the visible console window through an out-pointer"
)]
fn stream_columns_windows(stream: &impl std::os::windows::io::AsRawHandle) -> Option<usize> {
    use windows_sys::Win32::System::Console::{
        CONSOLE_SCREEN_BUFFER_INFO, GetConsoleScreenBufferInfo,
    };
    let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
    // SAFETY: the handle belongs to the live stream and info is writable.
    if unsafe { GetConsoleScreenBufferInfo(stream.as_raw_handle(), &raw mut info) } == 0 {
        return None;
    }
    let width = i32::from(info.srWindow.Right) - i32::from(info.srWindow.Left) + 1;
    usize::try_from(width).ok().filter(|&cols| cols > 0)
}

#[cfg(windows)]
use stream_columns_windows as stream_columns;

/// Enable ANSI output on the attached Windows console, even if stdout is redirected.
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Win32 console modes have no standard-library API"
)]
pub(crate) fn enable_console_ansi() {
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::System::Console::{
        ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, SetConsoleMode,
    };
    let Ok(console) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(r"\\.\CONOUT$")
    else {
        return;
    };
    let handle = console.as_raw_handle();
    let mut mode = 0;
    // SAFETY: console owns the handle for both calls and mode is writable.
    unsafe {
        if GetConsoleMode(handle, &raw mut mode) != 0
            && mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING == 0
        {
            SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::stream_columns;

    #[test]
    fn redirected_streams_do_not_report_a_console_width() {
        let (reader, writer) = std::io::pipe().expect("test pipe");
        assert_eq!(stream_columns(&writer), None);
        assert_eq!(stream_columns(&reader), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn pty_reports_positive_dimensions_and_rejects_zero_axes() {
        let ptmx = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/ptmx")
            .expect("open /dev/ptmx");
        for (rows, cols, expected) in [
            (24, 120, Some(120)),
            (0, 120, None),
            (24, 0, None),
            (0, 0, None),
        ] {
            let size = rustix::termios::Winsize {
                ws_row: rows,
                ws_col: cols,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            rustix::termios::tcsetwinsize(&ptmx, size).expect("set pty winsize");
            assert_eq!(
                stream_columns(&ptmx),
                expected,
                "unexpected columns for rows={rows}, cols={cols}"
            );
        }
    }
}
