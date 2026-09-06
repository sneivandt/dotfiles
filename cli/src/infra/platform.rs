//! Platform detection (OS and Arch Linux).
use std::fmt;

/// Detected operating system platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    /// Linux operating system.
    Linux,
    /// Windows operating system.
    Windows,
}

impl Os {
    /// Returns whether this OS is Unix-like.
    #[must_use]
    pub const fn is_unix_like(self) -> bool {
        matches!(self, Self::Linux)
    }

    /// Returns whether this OS supports POSIX file permissions.
    #[must_use]
    pub const fn supports_posix_permissions(self) -> bool {
        matches!(self, Self::Linux)
    }

    /// Returns whether this OS uses the Windows Registry.
    #[must_use]
    pub const fn has_registry(self) -> bool {
        matches!(self, Self::Windows)
    }
}

impl fmt::Display for Os {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Linux => write!(f, "linux"),
            Self::Windows => write!(f, "windows"),
        }
    }
}

/// Platform information for the current system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Platform {
    /// Detected operating system.
    pub os: Os,
    /// Whether the platform is Arch Linux.
    pub is_arch: bool,
    /// Whether running inside Windows Subsystem for Linux.
    pub is_wsl: bool,
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.description())
    }
}

impl Platform {
    /// Detect the current platform.
    #[must_use]
    pub fn detect() -> Self {
        Self {
            os: Self::detect_os(),
            is_arch: Self::detect_arch(),
            is_wsl: Self::detect_wsl(),
        }
    }

    /// Create a platform with explicit values (for testing).
    #[cfg(test)]
    #[must_use]
    pub const fn new(os: Os, is_arch: bool) -> Self {
        Self {
            os,
            is_arch,
            is_wsl: false,
        }
    }

    /// Create a platform with WSL flag set (for testing).
    #[cfg(test)]
    #[must_use]
    pub const fn new_wsl() -> Self {
        Self {
            os: Os::Linux,
            is_arch: false,
            is_wsl: true,
        }
    }

    /// Returns whether this platform is Linux.
    #[must_use]
    pub const fn is_linux(&self) -> bool {
        matches!(self.os, Os::Linux)
    }

    /// Returns whether this platform is Windows.
    #[must_use]
    pub const fn is_windows(&self) -> bool {
        matches!(self.os, Os::Windows)
    }

    /// Returns whether this platform supports POSIX file permissions (chmod).
    #[must_use]
    pub const fn supports_chmod(&self) -> bool {
        self.os.supports_posix_permissions()
    }

    /// Returns whether this platform supports systemd.
    #[must_use]
    pub const fn supports_systemd(&self) -> bool {
        self.os.is_unix_like()
    }

    /// Returns whether this platform uses the Windows Registry.
    #[must_use]
    pub const fn has_registry(&self) -> bool {
        self.os.has_registry()
    }

    /// Returns whether this platform is Arch Linux.
    #[must_use]
    pub const fn is_arch_linux(&self) -> bool {
        self.is_arch
    }

    /// Returns whether this platform is Windows Subsystem for Linux.
    #[must_use]
    pub const fn is_wsl(&self) -> bool {
        self.is_wsl
    }

    /// Returns whether this platform uses pacman as the primary package manager.
    #[must_use]
    pub const fn uses_pacman(&self) -> bool {
        self.os.is_unix_like() && self.is_arch && !self.is_wsl
    }

    /// Returns whether this platform supports AUR packages.
    #[must_use]
    pub const fn supports_aur(&self) -> bool {
        self.is_arch && !self.is_wsl
    }

    /// Returns a display-friendly description of the platform.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        match (&self.os, self.is_arch_linux()) {
            (Os::Linux, true) => "Arch Linux",
            (Os::Linux, false) => "Linux",
            (Os::Windows, _) => "Windows",
        }
    }

    /// Check whether a profile category tag should be excluded based on platform.
    /// Returns true if the tag is incompatible with this platform.
    #[must_use]
    pub fn excludes_category(
        &self,
        category: &crate::infra::config::category_matcher::Category,
    ) -> bool {
        use crate::infra::config::category_matcher::Category;
        match category {
            Category::Linux => self.os != Os::Linux,
            Category::Windows => self.os != Os::Windows,
            Category::Arch => !self.is_arch_linux() || self.is_wsl(),
            Category::Base | Category::Desktop | Category::Other(_) => false,
        }
    }

    const fn detect_os() -> Os {
        if cfg!(target_os = "linux") {
            Os::Linux
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            // Default to Linux for other Unix-like systems
            Os::Linux
        }
    }

    fn detect_arch() -> bool {
        if cfg!(target_os = "linux") {
            std::path::Path::new("/etc/arch-release").exists()
        } else {
            false
        }
    }

    fn detect_wsl() -> bool {
        if cfg!(target_os = "linux") {
            std::fs::read_to_string("/proc/version").is_ok_and(|v| {
                let lower = v.to_lowercase();
                lower.contains("microsoft") || lower.contains("wsl")
            })
        } else {
            false
        }
    }
}

/// Registry subkey holding the Windows Developer Mode flag.
#[cfg(windows)]
pub const DEVELOPER_MODE_SUBKEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock";

/// Registry value name for the Windows Developer Mode flag.
#[cfg(windows)]
pub const DEVELOPER_MODE_VALUE: &str = "AllowDevelopmentWithoutDevLicense";

/// Read the raw Windows Developer Mode registry flag.
///
/// Returns `Ok(None)` when the key or value is absent, which is the normal
/// state on a machine where Developer Mode has never been enabled.
///
/// # Errors
///
/// Returns an error when the registry can be reached but the read fails for a
/// reason other than the value being missing.
#[cfg(windows)]
pub fn developer_mode_flag() -> std::io::Result<Option<u32>> {
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    match hklm.open_subkey(DEVELOPER_MODE_SUBKEY) {
        Ok(key) => match key.get_value::<u32, _>(DEVELOPER_MODE_VALUE) {
            Ok(value) => Ok(Some(value)),
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        },
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Return whether Windows Developer Mode is enabled.
///
/// Developer Mode is what permits unprivileged symlink creation, so this is the
/// capability behind an unelevated `install` run. Off Windows the flag cannot
/// exist, so the answer is always `false` and callers fall back to the
/// platform's native symlink support.
#[must_use]
#[cfg_attr(
    not(windows),
    allow(
        clippy::missing_const_for_fn,
        reason = "the Windows body reads the registry, so this cannot be const on every platform"
    )
)]
pub fn developer_mode_enabled() -> bool {
    #[cfg(windows)]
    {
        matches!(developer_mode_flag(), Ok(Some(1)))
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::config::category_matcher::Category;

    #[test]
    fn platform_detect_returns_valid() {
        let p = Platform::detect();
        assert!(
            p.is_linux() || p.is_windows(),
            "detected platform should be linux or windows"
        );
    }

    #[test]
    fn os_display() {
        assert_eq!(Os::Linux.to_string(), "linux");
        assert_eq!(Os::Windows.to_string(), "windows");
    }

    fn platforms() -> [Platform; 5] {
        [
            Platform::new(Os::Linux, false),
            Platform::new(Os::Linux, true),
            Platform::new(Os::Windows, false),
            Platform::new_wsl(),
            Platform {
                os: Os::Linux,
                is_arch: true,
                is_wsl: true,
            },
        ]
    }

    #[test]
    fn platform_capabilities_and_labels() {
        // POSIX support, Arch identity, WSL identity, Arch package support, label.
        let expected = [
            (true, false, false, false, "Linux"),
            (true, true, false, true, "Arch Linux"),
            (false, false, false, false, "Windows"),
            (true, false, true, false, "Linux"),
            (true, true, true, false, "Arch Linux"),
        ];
        assert_eq!(platforms().len(), expected.len(), "cover every platform");
        for (platform, (posix, arch, wsl, arch_packages, label)) in
            platforms().into_iter().zip(expected)
        {
            assert_eq!(platform.is_linux(), posix, "{platform:?}: Linux");
            assert_eq!(platform.is_windows(), !posix, "{platform:?}: Windows");
            assert_eq!(platform.supports_chmod(), posix, "{platform:?}: chmod");
            assert_eq!(platform.supports_systemd(), posix, "{platform:?}: systemd");
            assert_eq!(platform.has_registry(), !posix, "{platform:?}: registry");
            assert_eq!(platform.is_arch_linux(), arch, "{platform:?}: Arch");
            assert_eq!(platform.is_wsl(), wsl, "{platform:?}: WSL");
            assert_eq!(
                platform.uses_pacman(),
                arch_packages,
                "{platform:?}: pacman"
            );
            assert_eq!(platform.supports_aur(), arch_packages, "{platform:?}: AUR");
            assert_eq!(platform.description(), label, "{platform:?}: description");
            assert_eq!(platform.to_string(), label, "{platform:?}: display");
        }
    }

    #[test]
    fn platform_category_exclusions() {
        let categories = [
            Category::Linux,
            Category::Windows,
            Category::Arch,
            Category::Base,
            Category::Desktop,
            Category::Other("custom".to_string()),
        ];
        let expected = [
            [false, true, true, false, false, false],
            [false, true, false, false, false, false],
            [true, false, true, false, false, false],
            [false, true, true, false, false, false],
            [false, true, true, false, false, false],
        ];
        assert_eq!(platforms().len(), expected.len(), "cover every platform");
        for (platform, exclusions) in platforms().into_iter().zip(expected) {
            for (category, excluded) in categories.iter().zip(exclusions) {
                assert_eq!(
                    platform.excludes_category(category),
                    excluded,
                    "{platform:?}: {category:?}"
                );
            }
        }
    }
}
