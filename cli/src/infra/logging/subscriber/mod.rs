//! Tracing subscriber setup and initialization.

mod console;
mod event;
mod file;

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests;

use console::DotfilesFormatter;
pub(in crate::infra::logging) use console::{
    set_transient_progress, set_verbose, take_transient_progress_rows, transient_progress_rows,
};
pub(in crate::infra::logging) use file::FileLayer;

/// Initialise the global [`tracing`] subscriber.
///
/// Sets up a console subscriber that formats events to match the dotfiles
/// output style and a file subscriber that writes all events (including
/// `debug`) to `$XDG_CACHE_HOME/dotfiles/<command>.log`.
/// Must be called once at program startup, before any logging.
pub fn init_subscriber(verbose: bool, command: &str) {
    use tracing_subscriber::fmt::writer::MakeWriterExt as _;
    use tracing_subscriber::{
        Layer as _, filter::LevelFilter, fmt, layer::SubscriberExt as _,
        util::SubscriberInitExt as _,
    };

    set_verbose(verbose);

    let make_writer = std::io::stderr
        .with_max_level(tracing::Level::WARN)
        .and(std::io::stdout.with_min_level(tracing::Level::INFO));

    let console_layer = fmt::layer()
        .event_format(DotfilesFormatter)
        .with_writer(make_writer)
        .with_filter(LevelFilter::INFO);

    let file_layer = FileLayer::new(command).map(|layer| layer.with_filter(LevelFilter::DEBUG));

    tracing_subscriber::registry()
        .with(console_layer)
        .with(file_layer)
        .init();
}
