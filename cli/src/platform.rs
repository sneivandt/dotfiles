use std::fmt;

/// Detected operating system platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Windows,
}

impl fmt::Display for Os {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Os::Linux => write!(f, "linux"),
            Os::Windows => write!(f, "windows"),
        }
    }
}

/// Platform information for the current system.
#[derive(Debug, Clone)]
pub struct Platform {
    pub os: Os,
    pub is_arch: bool,
}

impl Platform {
    /// Detect the current platform.
    pub fn detect() -> Self {
        Self {
            os: Self::detect_os(),
            is_arch: Self::detect_arch(),
        }
    }

    /// Create a platform with explicit values (for testing).
    #[cfg(test)]
    pub fn new(os: Os, is_arch: bool) -> Self {
        Self { os, is_arch }
    }

    pub fn is_linux(&self) -> bool {
        self.os == Os::Linux
    }

    pub fn is_windows(&self) -> bool {
        self.os == Os::Windows
    }

    /// Check whether a profile category tag should be excluded based on platform.
    /// Returns true if the tag is incompatible with this platform.
    pub fn excludes_category(&self, category: &str) -> bool {
        match category {
            "windows" => self.os != Os::Windows,
            "arch" => !self.is_arch,
            _ => false,
        }
    }

    fn detect_os() -> Os {
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
        // On any system this should succeed
        assert!(p.is_linux() || p.is_windows());
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
}
