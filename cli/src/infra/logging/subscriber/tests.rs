use std::fs;
use std::sync::Arc;

use super::console::console_line_with_style;
use super::run_log::RunLogLayer;
use crate::infra::logging::MsgKind;
use crate::infra::logging::runlog::RunLog;
use crate::infra::logging::style::StyleChoice;
use tracing_subscriber::layer::SubscriberExt as _;

/// Install a [`RunLogLayer`] backed by a temp-directory run log and return the
/// log path, temp dir (must outlive the layer), and a tracing dispatcher guard.
fn isolated_run_log_layer() -> (
    std::path::PathBuf,
    tempfile::TempDir,
    super::super::TestDispatchGuard,
) {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let dir = super::super::utils::dotfiles_log_subdir(tmp.path()).expect("log subdir");
    let run_log = Arc::new(
        RunLog::new("test", &dir, std::time::Instant::now()).expect("run log should be created"),
    );
    let path = run_log.path().to_path_buf();
    let subscriber = tracing_subscriber::registry().with(RunLogLayer::new(run_log));
    let dispatch = tracing::Dispatch::new(subscriber);
    let guard = super::super::test_dispatch_guard(&dispatch);
    (path, tmp, guard)
}

#[test]
fn run_log_layer_writes_header() {
    let (path, _tmp, _guard) = isolated_run_log_layer();
    let content = fs::read_to_string(&path).unwrap();
    assert!(
        content.starts_with("# Dotfiles "),
        "header should identify the tool and version: {content}"
    );
    assert!(
        content.contains("# Columns:"),
        "header should document the column layout: {content}"
    );
}

#[test]
fn run_log_layer_records_level_as_event_name() {
    let (path, _tmp, _guard) = isolated_run_log_layer();
    tracing::error!("something broke");
    tracing::warn!("careful now");
    tracing::debug!("extra detail");
    tracing::info!("regular info");

    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("[error] something broke"), "{content}");
    assert!(content.contains("[warn] careful now"), "{content}");
    assert!(content.contains("[debug] extra detail"), "{content}");
    assert!(content.contains("[info] regular info"), "{content}");
}

#[test]
fn run_log_layer_uses_thread_task_context() {
    let (path, _tmp, _guard) = isolated_run_log_layer();
    let _task = crate::infra::logging::log_task_context("example-task");

    tracing::info!("task detail");

    let content = fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("[example-task] [info] task detail"),
        "task context should precede the event name: {content}"
    );
}

#[test]
fn run_log_layer_strips_ansi_and_leading_whitespace() {
    let (path, _tmp, _guard) = isolated_run_log_layer();
    tracing::info!("\x1b[31mcolored\x1b[0m text");
    tracing::info!("  padded info");

    let content = fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("colored text") && !content.contains("\x1b["),
        "ANSI codes should be stripped: {content}"
    );
    let padded = content
        .lines()
        .find(|line| line.contains("padded info"))
        .unwrap();
    assert!(
        padded.ends_with("[info] padded info"),
        "leading whitespace should be stripped: {padded}"
    );
}

#[test]
fn run_log_layer_omits_empty_messages() {
    let (path, _tmp, _guard) = isolated_run_log_layer();
    let before = fs::read_to_string(&path).unwrap();

    tracing::info!("");
    tracing::warn!("   ");
    tracing::error!("\t");
    tracing::debug!("\x1b[31m\x1b[0m");

    let after = fs::read_to_string(&path).unwrap();
    assert_eq!(
        after, before,
        "empty log messages should not write timestamp-only file lines"
    );
}

#[test]
fn message_presentation_golden_matrix() {
    for (kind, verbose_only, plain, colored) in [
        (MsgKind::Stage, true, "detail", "detail"),
        (MsgKind::TaskStage, true, "detail", "detail"),
        (MsgKind::Info, true, "  detail", "  detail"),
        (MsgKind::Debug, true, "  detail", "  detail"),
        (MsgKind::Context, true, "detail", "\x1b[2mdetail\x1b[0m"),
        (MsgKind::Trace, false, "", ""),
        (MsgKind::Summary, false, "", ""),
        (
            MsgKind::Warn,
            false,
            "WARN  detail",
            "\x1b[33mWARN\x1b[0m  detail",
        ),
        (
            MsgKind::Error,
            false,
            "ERROR detail",
            "\x1b[31mERROR\x1b[0m detail",
        ),
        (MsgKind::DryRun, false, "  detail", "  detail"),
        (MsgKind::Always, false, "detail", "detail"),
        (MsgKind::Startup, false, "detail", "\x1b[2mdetail\x1b[0m"),
    ] {
        for (terminal, no_color, ansi) in [
            (false, false, false),
            (false, true, false),
            (true, false, true),
            (true, true, false),
        ] {
            for verbose in [false, true] {
                let expected = (!matches!(kind, MsgKind::Trace | MsgKind::Summary)
                    && (!verbose_only || verbose))
                    .then_some(if ansi { colored } else { plain });
                assert_eq!(
                    crate::infra::logging::console::ui_line_with_style(
                        kind,
                        "detail",
                        StyleChoice::auto(terminal, no_color),
                        verbose,
                    )
                    .as_deref(),
                    expected,
                    "{kind:?}, verbose={verbose}, terminal={terminal}, NO_COLOR={no_color}"
                );
            }
        }
    }
}

#[test]
fn structured_startup_line_emphasizes_its_label_only() {
    assert_eq!(
        crate::infra::logging::console::ui_line_with_style(
            MsgKind::Startup,
            "Install · profile desktop · Arch Linux",
            StyleChoice::auto(true, false),
            false,
        )
        .as_deref(),
        Some("\x1b[1mInstall\x1b[0m\x1b[2m · profile desktop · Arch Linux\x1b[0m")
    );
}

#[test]
fn cli_upgrade_remains_a_startup_notice_with_a_dim_version_transition() {
    assert_eq!(
        crate::infra::logging::console::ui_line_with_style(
            MsgKind::Startup,
            "CLI upgraded · old → new",
            StyleChoice::auto(true, false),
            false,
        )
        .as_deref(),
        Some("\x1b[1mCLI upgraded\x1b[0m\x1b[2m · old → new\x1b[0m")
    );
}

#[test]
fn raw_tracing_presentation_golden_matrix() {
    for (level, plain, colored) in [
        (
            tracing::Level::ERROR,
            Some("ERROR detail"),
            Some("\x1b[31mERROR\x1b[0m detail"),
        ),
        (
            tracing::Level::WARN,
            Some("WARN  detail"),
            Some("\x1b[33mWARN\x1b[0m  detail"),
        ),
        (tracing::Level::INFO, Some("  detail"), Some("  detail")),
        (tracing::Level::DEBUG, None, None),
        (tracing::Level::TRACE, None, None),
    ] {
        for verbose in [false, true] {
            for (style, expected) in [
                (StyleChoice::plain(), plain),
                (StyleChoice::colored(), colored),
            ] {
                let expected = expected.filter(|_| verbose || level != tracing::Level::INFO);
                assert_eq!(
                    console_line_with_style(level, "dotfiles", "detail", style, verbose).as_deref(),
                    expected,
                    "{level}, verbose={verbose}, style={style:?}"
                );
            }
        }
    }
}

#[test]
fn console_line_strips_ansi_when_style_disabled() {
    let line = console_line_with_style(
        tracing::Level::INFO,
        "dotfiles",
        "\x1b[32m3 Changed\x1b[0m",
        StyleChoice::plain(),
        true,
    )
    .unwrap();

    assert_eq!(line, "  3 Changed");
}

#[test]
fn console_line_plain_stderr_warning_has_no_ansi() {
    let line = console_line_with_style(
        tracing::Level::WARN,
        "dotfiles",
        "\x1b[1mcareful\x1b[0m",
        StyleChoice::plain(),
        true,
    )
    .unwrap();

    assert_eq!(line, "WARN  careful");
    assert!(!line.contains("\x1b["));
}

#[test]
fn raw_diagnostics_and_task_progress_share_one_console_state() {
    use crate::infra::logging::{OutputExt as _, isolated_logger_for};
    let (log, _tmp, _guard) = isolated_logger_for("install");
    log.startup("Install · fixture");
    log.notify_task_start_with_progress("first", true);
    tracing::debug!("stored only");
    tracing::warn!("raw warning");
    assert_eq!(log.progress_rows_count(), 0);
    log.notify_task_start_with_progress("second", true);
    log.always("durable message");
    log.notify_task_start_with_progress("third", true);
    log.clear_status();
    log.clear_status();

    let writes = log.console.lock().captured.clone();
    let expected = [
        (false, "Install · fixture\n"),
        (false, "\n"),
        (false, "Running · first"),
        (true, "\r\x1b[KWARN  raw warning\n"),
        (false, "\nRunning · first, second"),
        (false, "\r\x1b[K\x1b[1A\r\x1b[Kdurable message\n"),
        (false, "\nRunning · first, second, third"),
        (false, "\r\x1b[K\x1b[1A\r\x1b[K"),
    ];
    assert_eq!(
        writes,
        expected.map(|(error, text)| (error, text.to_string()))
    );
    let stored = fs::read_to_string(log.log_path().unwrap()).unwrap();
    assert_eq!(stored.matches("[debug] stored only").count(), 1);
    assert_eq!(stored.matches("[warn] raw warning").count(), 1);
    assert!(!stored.contains("Running"));
}

#[test]
fn another_logger_cannot_change_the_active_subscribers_verbosity() {
    use crate::infra::logging::isolated_logger;
    let (mut log, tmp, _guard) = isolated_logger();
    log.set_verbose(false);
    let mut other = crate::infra::logging::Logger::new_in("check", tmp.path());
    other.set_verbose(true);
    tracing::info!("hidden info");
    tracing::warn!("visible warning");
    assert_eq!(log.captured_lines(), ["WARN  visible warning"]);
    assert!(other.captured_lines().is_empty());
    log.set_verbose(true);
    tracing::info!("verbose info");
    assert_eq!(
        log.captured_lines(),
        ["WARN  visible warning", "  verbose info"]
    );
}
