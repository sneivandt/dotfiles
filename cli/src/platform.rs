use std::fmt;

/// Detected operating system platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Windows,
}

impl Os {
    /// Returns a human-readable name for the OS.
    #[cfg(test)]
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Linux => "Linux",
            Self::Windows => "Windows",
        }
    }

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
#[derive(Debug, Clone)]
pub struct Platform {
    pub os: Os,
    pub is_arch: bool,
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
        }
    }

    /// Create a platform with explicit values (for testing).
    #[cfg(test)]
    #[must_use]
    pub const fn new(os: Os, is_arch: bool) -> Self {
        Self { os, is_arch }
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

    /// Returns whether this platform uses pacman as the primary package manager.
    #[must_use]
    pub const fn uses_pacman(&self) -> bool {
        self.os.is_unix_like() && self.is_arch
    }

    /// Returns whether this platform supports AUR packages.
    #[must_use]
    pub const fn supports_aur(&self) -> bool {
        self.is_arch
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
    pub fn excludes_category(&self, category: &str) -> bool {
        match category {
            "linux" => self.os != Os::Linux,
            "windows" => self.os != Os::Windows,
            "arch" => !self.is_arch_linux(),
            _ => false,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_detect_returns_valid() {
        let p = Platform::detect();
        assert!(
            p.is_linux() || p.is_windows(),
            "detected platform should be linux or windows"
        );
    }

    #[test]
    fn platform_new_linux() {
        let p = Platform::new(Os::Linux, false);
        assert!(p.is_linux());
        assert!(!p.is_windows());
        assert!(!p.is_arch);
    }

    #[test]
    fn platform_new_windows() {
        let p = Platform::new(Os::Windows, false);
        assert!(p.is_windows());
        assert!(!p.is_linux());
    }

    #[test]
    fn platform_new_arch() {
        let p = Platform::new(Os::Linux, true);
        assert!(p.is_arch);
    }

    #[test]
    fn excludes_category_linux_on_linux() {
        let p = Platform::new(Os::Linux, false);
        assert!(!p.excludes_category("linux"));
    }

    #[test]
    fn excludes_category_linux_on_windows() {
        let p = Platform::new(Os::Windows, false);
        assert!(p.excludes_category("linux"));
    }

    #[test]
    fn excludes_category_windows_on_linux() {
        let p = Platform::new(Os::Linux, false);
        assert!(p.excludes_category("windows"));
        assert!(!p.excludes_category("desktop"));
    }

    #[test]
    fn excludes_category_arch_on_non_arch() {
        let p = Platform::new(Os::Linux, false);
        assert!(p.excludes_category("arch"));
    }

    #[test]
    fn excludes_category_arch_on_arch() {
        let p = Platform::new(Os::Linux, true);
        assert!(!p.excludes_category("arch"));
    }

    #[test]
    fn excludes_category_windows_on_windows() {
        let p = Platform::new(Os::Windows, false);
        assert!(!p.excludes_category("windows"));
        assert!(p.excludes_category("arch"));
    }

    #[test]
    fn os_display() {
        assert_eq!(Os::Linux.to_string(), "linux");
        assert_eq!(Os::Windows.to_string(), "windows");
    }

    #[test]
    fn os_name() {
        assert_eq!(Os::Linux.name(), "Linux");
        assert_eq!(Os::Windows.name(), "Windows");
    }

    #[test]
    fn os_is_unix_like() {
        assert!(Os::Linux.is_unix_like());
        assert!(!Os::Windows.is_unix_like());
    }

    #[test]
    fn os_supports_posix_permissions() {
        assert!(Os::Linux.supports_posix_permissions());
        assert!(!Os::Windows.supports_posix_permissions());
    }

    #[test]
    fn os_has_registry() {
        assert!(!Os::Linux.has_registry());
        assert!(Os::Windows.has_registry());
    }

    #[test]
    fn platform_supports_chmod() {
        let linux = Platform::new(Os::Linux, false);
        let windows = Platform::new(Os::Windows, false);
        assert!(linux.supports_chmod());
        assert!(!windows.supports_chmod());
    }

    #[test]
    fn platform_supports_systemd() {
        let linux = Platform::new(Os::Linux, false);
        let windows = Platform::new(Os::Windows, false);
        assert!(linux.supports_systemd());
        assert!(!windows.supports_systemd());
    }

    #[test]
    fn platform_has_registry() {
        let linux = Platform::new(Os::Linux, false);
        let windows = Platform::new(Os::Windows, false);
        assert!(!linux.has_registry());
        assert!(windows.has_registry());
    }

    #[test]
    fn platform_is_arch_linux() {
        let arch = Platform::new(Os::Linux, true);
        let generic_linux = Platform::new(Os::Linux, false);
        let windows = Platform::new(Os::Windows, false);
        assert!(arch.is_arch_linux());
        assert!(!generic_linux.is_arch_linux());
        assert!(!windows.is_arch_linux());
    }

    #[test]
    fn platform_uses_pacman() {
        let arch = Platform::new(Os::Linux, true);
        let generic_linux = Platform::new(Os::Linux, false);
        let windows = Platform::new(Os::Windows, false);
        assert!(arch.uses_pacman());
        assert!(!generic_linux.uses_pacman());
        assert!(!windows.uses_pacman());
    }

    #[test]
    fn platform_supports_aur() {
        let arch = Platform::new(Os::Linux, true);
        let generic_linux = Platform::new(Os::Linux, false);
        let windows = Platform::new(Os::Windows, false);
        assert!(arch.supports_aur());
        assert!(!generic_linux.supports_aur());
        assert!(!windows.supports_aur());
    }

    #[test]
    fn platform_description() {
        let arch = Platform::new(Os::Linux, true);
        let generic_linux = Platform::new(Os::Linux, false);
        let windows = Platform::new(Os::Windows, false);
        assert_eq!(arch.description(), "Arch Linux");
        assert_eq!(generic_linux.description(), "Linux");
        assert_eq!(windows.description(), "Windows");
    }

    #[test]
    fn platform_display() {
        let arch = Platform::new(Os::Linux, true);
        let generic_linux = Platform::new(Os::Linux, false);
        let windows = Platform::new(Os::Windows, false);
        assert_eq!(arch.to_string(), "Arch Linux");
        assert_eq!(generic_linux.to_string(), "Linux");
        assert_eq!(windows.to_string(), "Windows");
    }
}
