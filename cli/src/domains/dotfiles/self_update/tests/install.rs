use super::*;
use sha2::{Digest, Sha256};
use std::io::Write as _;

use super::super::cache::cache_path;
use super::super::http::test_support::MockHttpClient;

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
