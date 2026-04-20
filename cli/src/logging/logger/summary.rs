//! End-of-run summary printing for [`Logger`].
//!
//! Renders the per-phase task breakdown (verbose mode only) followed by a
//! single-line aggregate count and the elapsed time.

use std::time::Duration;

use super::Logger;
use crate::logging::types::{TaskEntry, TaskStatus};
use crate::phases::TaskPhase;

#[allow(clippy::print_stdout)]
impl Logger {
    /// Print the summary of all recorded tasks, grouped by phase.
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
                TaskStatus::Ok => ok += 1,
                TaskStatus::NotApplicable => not_applicable += 1,
                TaskStatus::Skipped => skipped += 1,
                TaskStatus::DryRun => dry_run += 1,
                TaskStatus::Failed => failed += 1,
            }
        }

        // In verbose mode, show the full per-task breakdown.
        if self.verbose {
            println!();
            self.phase("Summary");

            let phases = [
                TaskPhase::Bootstrap,
                TaskPhase::Repository,
                TaskPhase::Apply,
            ];
            for phase in &phases {
                let phase_tasks: Vec<&TaskEntry> =
                    tasks.iter().filter(|t| t.phase == *phase).collect();
                let has_visible = phase_tasks
                    .iter()
                    .any(|t| t.status != TaskStatus::NotApplicable);
                if !has_visible {
                    continue;
                }
                self.info(&format!("\x1b[1m{phase}\x1b[0m"));
                for task in &phase_tasks {
                    let (icon, color) = match task.status {
                        TaskStatus::NotApplicable => continue,
                        TaskStatus::Ok => ("\u{2713}", "\x1b[32m"),
                        TaskStatus::Skipped => ("\u{25cb}", "\x1b[33m"),
                        TaskStatus::DryRun => ("~", "\x1b[35m"),
                        TaskStatus::Failed => ("\u{2717}", "\x1b[31m"),
                    };

                    let suffix = task
                        .message
                        .as_ref()
                        .map_or_else(String::new, |msg| format!(" ({msg})"));

                    self.info(&format!("{color}  {icon} {}{suffix}\x1b[0m", task.name));
                }
            }
        }

        self.always("");
        let active = ok + skipped + dry_run + failed;
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

        let na_suffix = if not_applicable > 0 {
            format!(" \x1b[2m({not_applicable} not applicable)\x1b[0m")
        } else {
            String::new()
        };

        let elapsed = self.start.elapsed();
        let elapsed_str = format_elapsed(elapsed);

        self.always(&format!(
            "  {active} tasks: {}{na_suffix}",
            parts.join(", "),
        ));

        self.always(&format!("  \x1b[2mcompleted in {elapsed_str}\x1b[0m"));
        if let Some(path) = &self.log_file {
            self.always(&format!("  \x1b[2mlog: {}\x1b[0m", path.display()));
        }
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
#[allow(clippy::expect_used, clippy::unwrap_used)]
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
