use std::fs;

use super::console::console_line_with_style;
use super::file::FileLayer;
use crate::infra::logging::style::StyleChoice;
use tracing_subscriber::layer::SubscriberExt as _;

/// Create a [`FileLayer`] in a temp directory and return the log file path,
/// temp dir (must outlive the layer), and a tracing dispatcher guard.
fn isolated_file_layer() -> (
    std::path::PathBuf,
    tempfile::TempDir,
    super::super::TestDispatchGuard,
) {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let layer = FileLayer::new_in("test", tmp.path()).expect("FileLayer::new_in should succeed");
    let path =
        super::super::utils::log_file_path_in("test", tmp.path()).expect("log path should resolve");
    let subscriber = tracing_subscriber::registry().with(layer);
    let dispatch = tracing::Dispatch::new(subscriber);
    let guard = super::super::test_dispatch_guard(&dispatch);
    (path, tmp, guard)
}

#[test]
fn file_layer_new_writes_header() {
    let (path, _tmp, _guard) = isolated_file_layer();
    let content = fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("=========================================="),
        "header should contain separator line"
    );
    assert!(
        content.contains("Dotfiles"),
        "header should contain 'Dotfiles'"
    );
}

#[test]
fn file_layer_formats_stage_with_arrow() {
    let (path, _tmp, _guard) = isolated_file_layer();
    tracing::info!(target: "dotfiles::stage", "my stage");
    let content = fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("==> my stage"),
        "stage should be prefixed with ==>: {content}"
    );
}

#[test]
fn file_layer_formats_dry_run_without_tag() {
    let (path, _tmp, _guard) = isolated_file_layer();
    tracing::info!(target: "dotfiles::dry_run", "would link");
    let content = fs::read_to_string(&path).unwrap();
    let line = content
        .lines()
        .find(|line| line.contains("would link"))
        .unwrap();
    assert!(
        !line.contains("[dry run]"),
        "dry_run should not have [dry run] tag in file: {line}"
    );
}

#[test]
fn file_layer_formats_error_level_label() {
    let (path, _tmp, _guard) = isolated_file_layer();
    tracing::error!("something broke");
    let content = fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("[error] something broke"),
        "error should have text level label: {content}"
    );
}

#[test]
fn file_layer_formats_warn_level_label() {
    let (path, _tmp, _guard) = isolated_file_layer();
    tracing::warn!("careful now");
    let content = fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("[warn] careful now"),
        "warn should have text level label: {content}"
    );
}

#[test]
fn file_layer_formats_debug_with_level_label() {
    let (path, _tmp, _guard) = isolated_file_layer();
    tracing::debug!("extra detail");
    let content = fs::read_to_string(&path).unwrap();
    let line = content
        .lines()
        .find(|line| line.contains("extra detail"))
        .unwrap();
    assert!(
        line.contains("[debug]"),
        "debug should have text level label: {line}"
    );
}

#[test]
fn file_layer_formats_info_with_level_label() {
    let (path, _tmp, _guard) = isolated_file_layer();
    tracing::info!("regular info");
    let content = fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("regular info"),
        "info message should appear: {content}"
    );
    let info_line = content
        .lines()
        .find(|line| line.contains("regular info"))
        .unwrap();
    assert!(
        info_line.contains("[info]") && !info_line.contains("==>"),
        "plain info should have text level label and no stage marker: {info_line}"
    );
}

#[test]
fn file_layer_formats_task_context_before_level_label() {
    let (path, _tmp, _guard) = isolated_file_layer();
    let span = tracing::info_span!("task", name = "example-task");
    let _enter = span.enter();

    tracing::info!("task detail");

    let content = fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("[example-task] [info] task detail"),
        "task context should precede level label: {content}"
    );
}

#[test]
fn file_layer_strips_ansi_codes() {
    let (path, _tmp, _guard) = isolated_file_layer();
    tracing::info!("\x1b[31mcolored\x1b[0m text");
    let content = fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("colored text"),
        "ANSI codes should be stripped: {content}"
    );
    assert!(
        !content.contains("\x1b["),
        "no ANSI escape should remain: {content}"
    );
}

#[test]
fn file_layer_includes_timestamp() {
    let (path, _tmp, _guard) = isolated_file_layer();
    tracing::info!("timestamped");
    let content = fs::read_to_string(&path).unwrap();
    let line = content
        .lines()
        .find(|line| line.contains("timestamped"))
        .unwrap();
    assert!(
        line.starts_with('['),
        "event line should start with timestamp bracket: {line}"
    );
}

#[test]
fn file_layer_strips_leading_whitespace() {
    let (path, _tmp, _guard) = isolated_file_layer();
    tracing::info!("  padded info");
    tracing::debug!("    deep indent");
    tracing::warn!("  padded warn");
    let content = fs::read_to_string(&path).unwrap();

    let info_line = content
        .lines()
        .find(|line| line.contains("padded info"))
        .unwrap();
    assert!(
        info_line.ends_with("] padded info"),
        "leading whitespace should be stripped from info: {info_line}"
    );

    let debug_line = content
        .lines()
        .find(|line| line.contains("deep indent"))
        .unwrap();
    assert!(
        debug_line.ends_with("] deep indent"),
        "leading whitespace should be stripped from debug: {debug_line}"
    );

    let warn_line = content
        .lines()
        .find(|line| line.contains("padded warn"))
        .unwrap();
    assert!(
        warn_line.ends_with("[warn] padded warn"),
        "leading whitespace should be stripped from warn: {warn_line}"
    );
}

#[test]
fn file_layer_omits_empty_messages() {
    let (path, _tmp, _guard) = isolated_file_layer();
    let before = fs::read_to_string(&path).unwrap();

    tracing::info!("");
    tracing::warn!("   ");
    tracing::error!("\t");
    tracing::debug!("\x1b[31m\x1b[0m");
    tracing::info!(target: "dotfiles::stage", "");
    tracing::info!(target: "dotfiles::dry_run", "  ");
    tracing::info!(target: "dotfiles::file_only", "");

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
        "dotfiles::always",
        "\x1b[32m3 Changed\x1b[0m",
        StyleChoice::plain(),
        true,
    )
    .unwrap();

    assert_eq!(line, "3 Changed");
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
fn console_stage_and_task_stage_are_plain() {
    let task = console_line_with_style(
        tracing::Level::INFO,
        "dotfiles::task_stage",
        "Install packages",
        StyleChoice::colored(),
        true,
    );
    let stage = console_line_with_style(
        tracing::Level::INFO,
        "dotfiles::stage",
        "Loading configuration",
        StyleChoice::colored(),
        true,
    );

    assert_eq!(task.as_deref(), Some("Install packages"));
    assert_eq!(stage.as_deref(), Some("Loading configuration"));
}
