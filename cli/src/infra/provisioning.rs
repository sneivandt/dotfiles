//! Explicit detection of installation environments that lack a real user session.

use super::env::Env;

/// Environment variable set by an operating-system installer around dotfiles.
pub const ENV_VAR: &str = "DOTFILES_PROVISIONING";

/// Value used while `install-arch` invokes dotfiles through `arch-chroot`.
pub const ARCH_CHROOT: &str = "arch-chroot";

/// Return whether dotfiles is configuring a target from an Arch chroot.
#[must_use]
pub fn is_arch_chroot(env: &dyn Env) -> bool {
    env.var(ENV_VAR).as_deref() == Some(ARCH_CHROOT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::env::MapEnv;

    #[test]
    fn detects_only_the_explicit_arch_chroot_value() {
        assert!(is_arch_chroot(&MapEnv::new().with(ENV_VAR, ARCH_CHROOT)));
        assert!(!is_arch_chroot(&MapEnv::new()));
        assert!(!is_arch_chroot(&MapEnv::new().with(ENV_VAR, "container")));
    }
}
