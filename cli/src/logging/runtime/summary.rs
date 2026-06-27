//! End-of-run summary printing for [`Logger`].
//!
//! Renders the per-domain task breakdown (verbose mode only) followed by a
//! single-line aggregate count and the elapsed time.

use std::time::Duration;

use super::Logger;
use crate::logging::types::{TaskEntry, TaskStatus};
use crate::tasks::Domain;

#[allow(clippy::print_stdout, reason = "intentional user-facing output")]
impl Logger {
    /// Print the summary of all recorded tasks, grouped by domain.
    pub fn print_summary(&self) {
        let tasks = match self.tasks.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };
        if tasks.is_empty() {
            return;
        }

        let mut ok = 0u32;
        let mut not_applicable = 0u32;
        let mut skipped = 0u32;
        let mut dry_run = 0u32;
        let mut failed = 0u32;

        for task in &tasks {
            match task.status {
                TaskStatus::Ok => ok = ok.saturating_add(1),
                TaskStatus::NotApplicable => not_applicable = not_applicable.saturating_add(1),
                TaskStatus::Skipped => skipped = skipped.saturating_add(1),
                TaskStatus::DryRun => dry_run = dry_run.saturating_add(1),
                TaskStatus::Failed => failed = failed.saturating_add(1),
            }
        }

        // Show the full per-task breakdown on verbose consoles and always keep
        // it in the persistent log file via DEBUG records.
        if self.verbose {
            println!();
            self.phase("Summary");
        } else {
            self.debug("");
            self.debug("Summary");
        }

        for domain in Domain::all() {
            let domain_tasks: Vec<&TaskEntry> =
                tasks.iter().filter(|t| t.domain == *domain).collect();
            let has_visible = domain_tasks
                .iter()
                .any(|t| t.status != TaskStatus::NotApplicable);
            if !has_visible {
                continue;
            }
            self.summary_detail(&format!("\x1b[1m{}\x1b[0m", domain.label()));
            for task in &domain_tasks {
                let Some((icon, color)) = task.status.icon_and_color() else {
                    continue;
                };

                let suffix = task
                    .message
                    .as_ref()
                    .map_or_else(String::new, |msg| format!(" \u{2014} {msg}"));

                self.summary_detail(&format!("{color}  {icon} {}{suffix}\x1b[0m", task.name));
            }
        }

        self.always("");

        // Overall status icon reflects the worst outcome: failures dominate,
        // then dry-run previews, otherwise a clean success.
        let (icon, icon_color) = if failed > 0 {
            ("\u{2717}", "\x1b[31m")
        } else if dry_run > 0 {
            ("~", "\x1b[35m")
        } else {
            ("\u{2713}", "\x1b[32m")
        };

        let mut parts: Vec<String> = vec![format!("\x1b[32m{ok} ok\x1b[0m")];
        if skipped > 0 {
            parts.push(format!("\x1b[33m{skipped} skipped\x1b[0m"));
        }
        if dry_run > 0 {
            parts.push(format!("\x1b[35m{dry_run} dry-run\x1b[0m"));
        }
        if failed > 0 {
            parts.push(format!("\x1b[31m{failed} failed\x1b[0m"));
        }
        if not_applicable > 0 {
            parts.push(format!("\x1b[2m{not_applicable} not applicable\x1b[0m"));
        }

        let elapsed = self.start.elapsed();
        let elapsed_str = format_elapsed(elapsed);

        let separator = " \x1b[2m\u{00b7}\x1b[0m ";
        self.always(&format!(
            "{icon_color}{icon}\x1b[0m {}",
            parts.join(separator),
        ));

        self.always(&format!("\x1b[2mcompleted in {elapsed_str}\x1b[0m"));
        if let Some(path) = self.log_path() {
            Self::file_only(&format!("log: {}", path.display()));
        }
        if let Some(diagnostic) = self.diagnostic() {
            Self::file_only(&format!("diagnostic log: {}", diagnostic.path().display()));
        }
    }

    fn summary_detail(&self, msg: &str) {
        if self.verbose {
            self.info(msg);
        } else {
            self.debug(msg);
        }
    }

    fn file_only(msg: &str) {
        tracing::info!(target: "dotfiles::file_only", "{msg}");
    }
}

/// Format a duration as a human-readable string (e.g., "1.2s", "2m 5s").
fn format_elapsed(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{:.1}s", d.as_secs_f64())
    } else {
        let mins = secs / 60;
        let remaining = secs % 60;
        format!("{mins}m {remaining}s")
    }
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
    fn format_elapsed_sub_second() {
        let d = Duration::from_millis(450);
        assert_eq!(format_elapsed(d), "0.5s");
    }

    #[test]
    fn format_elapsed_seconds() {
        let d = Duration::from_secs_f64(3.7);
        assert_eq!(format_elapsed(d), "3.7s");
    }

    #[test]
    fn format_elapsed_minutes() {
        let d = Duration::from_secs(125);
        assert_eq!(format_elapsed(d), "2m 5s");
    }
}
