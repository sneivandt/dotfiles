use std::fs;
use std::sync::Arc;

use super::console::console_line_with_style;
use super::run_log::RunLogLayer;
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
fn console_line_uses_ansi_when_style_enabled() {
    let line = console_line_with_style(
        tracing::Level::WARN,
        "dotfiles",
        "careful",
        StyleChoice::colored(),
        true,
    )
    .unwrap();

    assert_eq!(line, "\x1b[33mWARN\x1b[0m  careful");
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
fn console_line_never_emits_debug_events() {
    assert_eq!(
        console_line_with_style(
            tracing::Level::DEBUG,
            "dotfiles",
            "internal detail",
            StyleChoice::plain(),
            true,
        ),
        None
    );
}

#[test]
fn ui_startup_header_is_dim() {
    let colored = super::console::ui_line_with_style(
        crate::infra::logging::MsgKind::Startup,
        "Install · profile desktop · Arch Linux",
        StyleChoice::colored(),
        true,
    );
    let plain = super::console::ui_line_with_style(
        crate::infra::logging::MsgKind::Startup,
        "Install · profile desktop · Arch Linux",
        StyleChoice::plain(),
        true,
    );

    assert_eq!(
        colored.as_deref(),
        Some("\x1b[2mInstall · profile desktop · Arch Linux\x1b[0m")
    );
    assert_eq!(
        plain.as_deref(),
        Some("Install · profile desktop · Arch Linux")
    );
}

#[test]
fn ui_stage_and_task_stage_are_plain() {
    let task = super::console::ui_line_with_style(
        crate::infra::logging::MsgKind::TaskStage,
        "Install packages",
        StyleChoice::colored(),
        true,
    );
    let stage = super::console::ui_line_with_style(
        crate::infra::logging::MsgKind::Stage,
        "Loading configuration",
        StyleChoice::colored(),
        true,
    );

    assert_eq!(task.as_deref(), Some("Install packages"));
    assert_eq!(stage.as_deref(), Some("Loading configuration"));
}
