//! Expected package manager protocol, independent of the providers' command builders.

use super::resources::package::PackageManager;
use crate::infra::exec::CommandSpec;

pub(super) fn inventory() -> CommandSpec {
    CommandSpec::new("pacman").arg("-Q")
}

pub(super) fn pacman_uses_sudo() -> bool {
    #[cfg(unix)]
    {
        !nix::unistd::Uid::effective().is_root()
    }
    #[cfg(not(unix))]
    {
        true
    }
}

pub(super) fn native_install(manager: PackageManager, names: &[&str]) -> CommandSpec {
    match manager {
        PackageManager::Pacman => {
            let pacman = if pacman_uses_sudo() {
                CommandSpec::new("sudo").arg("pacman")
            } else {
                CommandSpec::new("pacman")
            };
            pacman
                .args(&["-Syu", "--needed", "--noconfirm"])
                .args(names)
        }
        PackageManager::Paru => CommandSpec::new("/usr/bin/paru")
            .args(&["-S", "--needed", "--noconfirm"])
            .args(names),
        PackageManager::Winget => panic!("winget installs packages individually"),
    }
}

pub(super) fn winget_install(name: &str) -> CommandSpec {
    CommandSpec::new("winget")
        .args(&[
            "install",
            "--id",
            name,
            "--exact",
            "--no-upgrade",
            "--source",
            "winget",
            "--accept-source-agreements",
            "--accept-package-agreements",
            "--disable-interactivity",
            "--scope",
            "user",
        ])
        .unchecked()
}
