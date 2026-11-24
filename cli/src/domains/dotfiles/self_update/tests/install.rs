use super::*;
use sha2::{Digest, Sha256};
use std::io::Write as _;

use super::super::cache::cache_path;
use super::super::http::test_support::MockHttpClient;

#[test]
fn smoke_test_retries_transient_busy_with_linear_backoff_and_stops_at_limit() {
    use crate::infra::exec::{ExecError, ExecResult};
    use std::io;
    use std::time::Duration;

    assert!(is_transient_busy(&io::Error::from(
        io::ErrorKind::ResourceBusy
    )));
    assert!(is_transient_busy(&io::Error::from(
        io::ErrorKind::ExecutableFileBusy
    )));
    #[cfg(unix)]
    assert!(is_transient_busy(&io::Error::from_raw_os_error(26)));
    #[cfg(windows)]
    assert!(is_transient_busy(&io::Error::from(
        io::ErrorKind::PermissionDenied
    )));
    assert!(!is_transient_busy(&io::Error::from(
        io::ErrorKind::NotFound
    )));

    let busy = || {
        ExecError::spawn(
            "smoke",
            #[cfg(unix)]
            io::Error::from_raw_os_error(26),
            #[cfg(not(unix))]
            io::Error::from(io::ErrorKind::ResourceBusy),
        )
    };

    // Succeeds on the final retry (attempt 6 after 5 sleeps with linear backoff).
    let mut calls = 0_u32;
    let mut sleeps = Vec::new();
    let out = run_smoke_test_with(
        Path::new("dotfiles"),
        |_| {
            calls += 1;
            if calls <= SMOKE_TEST_MAX_RETRIES {
                Err(busy())
            } else {
                Ok(ExecResult::success("dotfiles v1\n"))
            }
        },
        |delay| sleeps.push(delay),
    )
    .unwrap();
    assert_eq!(out, "dotfiles v1\n");
    assert_eq!(calls, SMOKE_TEST_MAX_RETRIES + 1);
    assert_eq!(
        sleeps,
        vec![
            Duration::from_millis(50),
            Duration::from_millis(100),
            Duration::from_millis(150),
            Duration::from_millis(200),
            Duration::from_millis(250),
        ]
    );

    // Exhausting retries stops after MAX_RETRIES sleeps.
    let mut exhausted_calls = 0_u32;
    let mut exhausted_sleeps = 0_u32;
    assert!(
        run_smoke_test_with(
            Path::new("dotfiles"),
            |_| {
                exhausted_calls += 1;
                assert!(
                    exhausted_calls <= SMOKE_TEST_MAX_RETRIES + 1,
                    "smoke test must stop retrying once SMOKE_TEST_MAX_RETRIES is reached"
                );
                Err(busy())
            },
            |_| exhausted_sleeps += 1,
        )
        .is_err()
    );
    assert_eq!(exhausted_calls, SMOKE_TEST_MAX_RETRIES + 1);
    assert_eq!(exhausted_sleeps, SMOKE_TEST_MAX_RETRIES);

    // Non-transient errors fail immediately without sleeping.
    let mut non_busy_calls = 0_u32;
    let mut non_busy_sleeps = 0_usize;
    assert!(
        run_smoke_test_with(
            Path::new("dotfiles"),
            |_| {
                non_busy_calls += 1;
                assert!(
                    non_busy_calls <= 1,
                    "non-transient smoke test error must not be retried"
                );
                Err(ExecError::spawn(
                    "smoke",
                    io::Error::from(io::ErrorKind::NotFound),
                ))
            },
            |_| non_busy_sleeps += 1,
        )
        .is_err()
    );
    assert_eq!(non_busy_calls, 1);
    assert_eq!(non_busy_sleeps, 0);
}

fn replace_binary(path: &Path, data: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap();
    fs::create_dir_all(dir)?;
    let (mut staged, mut file) =
        crate::infra::fs::TempGuard::create_unique_file(dir, ".fixture", "tmp")?;
    file.write_all(data)?;
    drop(file);
    #[cfg(unix)]
    crate::infra::fs::set_executable(staged.path())?;
    super::replace_binary(path, staged.path())?;
    staged.persist();
    Ok(())
}

#[derive(Debug)]
struct StubGh {
    verified: bool,
}

impl GhCli for StubGh {
    fn available(&self) -> bool {
        true
    }

    fn verify(&self, _path: &Path, _repo: &str) -> Result<super::super::attestation::Verification> {
        Ok(if self.verified {
            super::super::attestation::Verification::Verified
        } else {
            super::super::attestation::Verification::Unverified("not verified".to_string())
        })
    }
}

#[cfg(unix)]
#[test]
#[ignore = "subprocess fixture launched by the interrupted self-update test"]
fn interrupted_install_child() {
    let root = PathBuf::from(std::env::var_os("DOTFILES_TEST_UPDATE_ROOT").unwrap());
    let bin = root.join("dotfiles");
    match std::env::var("DOTFILES_TEST_UPDATE_PHASE")
        .unwrap()
        .as_str()
    {
        "backup" => assert!(backup_current_binary(&bin).unwrap().is_some()),
        "replace" => super::replace_binary(&bin, &root.join("staged")).unwrap(),
        phase => panic!("unexpected interrupted update phase: {phase}"),
    }
    // Exit without unwinding, so no caller rollback or guard cleanup runs.
    std::process::exit(86);
}

#[path = "install_recovery.rs"]
mod recovery;
#[path = "install_replacement.rs"]
mod replacement;
#[path = "install_verification.rs"]
mod verification;
