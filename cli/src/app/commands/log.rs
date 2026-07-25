//! Log command implementation.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context as _, Result};

const NO_LOG_FOUND: &str = "No dotfiles log found yet.";

/// Run the log command.
///
/// # Errors
///
/// Returns an error if the log directory or selected log file cannot be read.
pub fn run() -> Result<()> {
    let cache_dir = crate::infra::logging::dotfiles_cache_dir_readonly();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    run_with_cache_dir(&cache_dir, &mut out)
}

fn run_with_cache_dir(cache_dir: &Path, out: &mut dyn std::io::Write) -> Result<()> {
    let Some(path) = newest_log_path(cache_dir)? else {
        writeln!(out, "{NO_LOG_FOUND}").context("writing log output")?;
        return Ok(());
    };

    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("reading dotfiles log {}", path.display()))?;
    out.write_all(contents.as_bytes())
        .context("writing log output")?;
    Ok(())
}

fn newest_log_path(cache_dir: &Path) -> Result<Option<PathBuf>> {
    if !cache_dir.is_dir() {
        return Ok(None);
    }

    let entries = std::fs::read_dir(cache_dir)
        .with_context(|| format!("reading dotfiles log directory {}", cache_dir.display()))?;
    let mut newest: Option<(SystemTime, PathBuf)> = None;

    for entry in entries {
        let entry = entry.with_context(|| format!("reading entry in {}", cache_dir.display()))?;
        let path = entry.path();
        if !is_log_file(&path) {
            continue;
        }
        let modified = entry
            .metadata()
            .with_context(|| format!("reading metadata for {}", path.display()))?
            .modified()
            .with_context(|| format!("reading modified time for {}", path.display()))?;

        if newest
            .as_ref()
            .is_none_or(|(newest_modified, _)| modified > *newest_modified)
        {
            newest = Some((modified, path));
        }
    }

    Ok(newest.map(|(_, path)| path))
}

fn is_log_file(path: &Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("log"))
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;

    #[test]
    fn prints_existing_log_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cache_dir = tmp.path().join("dotfiles");
        std::fs::create_dir(&cache_dir).expect("create cache dir");
        std::fs::write(cache_dir.join("install.log"), "normal log\n").expect("write log");

        let mut output = Vec::new();
        run_with_cache_dir(&cache_dir, &mut output).expect("log command should succeed");

        assert_eq!(String::from_utf8(output).unwrap(), "normal log\n");
    }

    #[test]
    fn prints_missing_log_message() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cache_dir = tmp.path().join("dotfiles");

        let mut output = Vec::new();
        run_with_cache_dir(&cache_dir, &mut output).expect("log command should succeed");

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "No dotfiles log found yet.\n"
        );
    }
}
