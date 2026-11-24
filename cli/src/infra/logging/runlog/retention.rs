use super::naming::{parse_run_log_file_name, read_dotfiles_header, run_log_file_name, sort_stamp};
use super::warn_degraded;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Attempts made to find an unused run-log file name before giving up.
const MAX_NAME_ATTEMPTS: u32 = 64;

pub(super) const RETENTION_LOCK: &str = ".dotfiles-log-retention.lock";

pub(super) fn lease_path(path: &Path) -> PathBuf {
    path.with_extension("log.lock")
}

pub(super) fn open_lock(path: &Path) -> std::io::Result<fs::File> {
    private_file_options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

// Logs and their lock sidecars must not inherit group/other access from umask.
fn private_file_options() -> fs::OpenOptions {
    #[cfg(unix)]
    let mut options = fs::OpenOptions::new();
    #[cfg(not(unix))]
    let options = fs::OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options
}

// Serialize lease creation/removal so no contender can lock an unlinked,
// obsolete lease while a new writer holds a different file at the same path.
fn lock_log_directory(dir: &Path) -> std::io::Result<fs::File> {
    let file = open_lock(&dir.join(RETENTION_LOCK))?;
    file.lock()?;
    Ok(file)
}

/// Create a fresh run-log file, returning its path and an append handle.
///
/// Always uses `create_new`, so an existing log is never truncated. When the
/// name is already taken — two runs starting in the same second from the same
/// process — the disambiguating suffix is bumped until a free name is found
/// rather than silently reusing or destroying the existing file.
pub(super) fn create_run_log_file(
    command: &str,
    log_dir: &Path,
    stamp: &str,
) -> Option<(PathBuf, fs::File, fs::File)> {
    create_run_log_file_with(command, log_dir, stamp, |path| {
        private_file_options()
            .create_new(true)
            .append(true)
            .open(path)
    })
}

pub(super) fn create_run_log_file_with(
    command: &str,
    log_dir: &Path,
    stamp: &str,
    open_log: impl Fn(&Path) -> std::io::Result<fs::File>,
) -> Option<(PathBuf, fs::File, fs::File)> {
    let _directory_lock = lock_log_directory(log_dir)
        .map_err(|error| {
            warn_degraded(&format!(
                "locking log directory {}: {error}",
                log_dir.display()
            ));
        })
        .ok()?;
    let pid = std::process::id();
    for attempt in 0..MAX_NAME_ATTEMPTS {
        let path = log_dir.join(run_log_file_name(command, stamp, pid.wrapping_add(attempt)));
        // Lock a sidecar, not the log: Windows byte-range locks would prevent
        // readers from viewing a live log.
        let lease = open_lock(&lease_path(&path))
            .map_err(|error| {
                warn_degraded(&format!("opening log lease {}: {error}", path.display()));
            })
            .ok()?;
        match lease.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => continue,
            Err(fs::TryLockError::Error(error)) => {
                warn_degraded(&format!("locking log lease {}: {error}", path.display()));
                return None;
            }
        }
        match open_log(&path) {
            Ok(file) => return Some((path, file, lease)),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                // Holding both the directory lock and this exclusive lease proves
                // the existing log is closed, so cleaning up the lease leaves no orphan.
                drop(lease);
                if let Err(cleanup_error) = fs::remove_file(lease_path(&path)) {
                    tracing::debug!(
                        "could not remove unused log lease {}: {cleanup_error}",
                        path.display()
                    );
                }
            }
            Err(error) => {
                warn_degraded(&format!("creating run log {}: {error}", path.display()));
                drop(lease);
                if let Err(cleanup_error) = fs::remove_file(lease_path(&path)) {
                    tracing::debug!(
                        "could not remove unused log lease {}: {cleanup_error}",
                        path.display()
                    );
                }
                return None;
            }
        }
    }
    None
}

/// Delete the oldest run logs until at most `keep` remain.
///
/// Require a matching filename and a readable Dotfiles header before counting
/// or pruning. The log directory override can be shared with other tools. Active leases are
/// skipped even if that leaves more than `keep` logs. Cleanup failures are
/// diagnostic-only: retention is housekeeping and must never fail a run.
pub(super) fn prune_run_logs(dir: &Path, keep: usize) {
    prune_run_logs_with(dir, keep, read_dotfiles_header);
}

pub(super) fn prune_run_logs_with(
    dir: &Path,
    keep: usize,
    mut read_header: impl FnMut(&Path) -> Option<String>,
) {
    let _directory_lock = match lock_log_directory(dir) {
        Ok(lock) => lock,
        Err(error) => {
            tracing::debug!("could not lock log retention in {}: {error}", dir.display());
            return;
        }
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let candidates: Vec<(PathBuf, String, String)> = entries
        .flatten()
        .filter_map(|entry| {
            if !entry.file_type().ok()?.is_file() {
                return None;
            }
            let name = entry.file_name().into_string().ok()?;
            let parsed = parse_run_log_file_name(&name)?;
            let stamp = parsed.stamp.to_string();
            Some((entry.path(), stamp, name))
        })
        .collect();
    if candidates.len() <= keep {
        return;
    }
    let mut runs: Vec<(String, String)> = candidates
        .into_iter()
        .filter_map(|(path, stamp, name)| {
            let header = read_header(&path)?;
            Some((sort_stamp(Some(&header), &stamp), name))
        })
        .collect();
    let mut excess = runs.len().saturating_sub(keep);
    if excess == 0 {
        return;
    }
    runs.sort_unstable();
    for (_, name) in runs {
        if excess == 0 {
            break;
        }
        let path = dir.join(name);
        let lease_path = lease_path(&path);
        let lease = match open_lock(&lease_path) {
            Ok(lease) => lease,
            Err(error) => {
                tracing::debug!("could not open log lease {}: {error}", lease_path.display());
                continue;
            }
        };
        match lease.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => continue,
            Err(fs::TryLockError::Error(error)) => {
                tracing::debug!("could not lock log lease {}: {error}", lease_path.display());
                continue;
            }
        }
        if !apply_prune_removal(fs::remove_file(&path), &path, &mut excess) {
            continue;
        }
        drop(lease);
        if let Err(error) = fs::remove_file(&lease_path) {
            tracing::debug!(
                "could not remove log lease {}: {error}",
                lease_path.display()
            );
        }
    }
}

fn apply_prune_removal(result: std::io::Result<()>, path: &Path, excess: &mut usize) -> bool {
    match result {
        Ok(()) => {
            *excess -= 1;
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            *excess -= 1;
            true
        }
        Err(error) => {
            tracing::debug!("could not prune run log {}: {error}", path.display());
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_prune_removal_decrements_for_ok_and_not_found_only() {
        let path = Path::new("sample.log");
        let mut excess = 3;
        assert!(apply_prune_removal(Ok(()), path, &mut excess));
        assert_eq!(excess, 2);
        assert!(apply_prune_removal(
            Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
            path,
            &mut excess
        ));
        assert_eq!(excess, 1);
        assert!(!apply_prune_removal(
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            path,
            &mut excess
        ));
        assert_eq!(excess, 1);
    }
}
