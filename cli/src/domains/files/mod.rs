//! Files domain: managed symlinks and file permissions.

pub mod chmod;
pub mod config;
pub mod resources;
pub mod symlinks;

/// A validated octal file permission mode (e.g., `"600"`, `"0755"`).
///
/// Parsing validates that the string is 3-4 ASCII octal digits, so consumers
/// can call [`as_u32`](Self::as_u32) without error handling.
///
/// # Examples
///
/// ```
/// use dotfiles_cli::testing::resources::chmod::OctalMode;
///
/// let mode = OctalMode::parse("755").unwrap();
/// assert_eq!(mode.as_u32(), 0o755);
/// assert_eq!(mode.as_str(), "755");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OctalMode {
    raw: String,
    bits: u32,
}

const OCTAL_MODE_MIN_LEN: usize = 3;
const OCTAL_MODE_MAX_LEN: usize = 4;

impl OctalMode {
    /// Parse and validate an octal mode string.
    ///
    /// # Errors
    ///
    /// Returns a human-readable error message if the string is invalid.
    pub fn parse(s: &str) -> Result<Self, String> {
        if !s.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!(
                "invalid octal mode '{s}': must contain only digits"
            ));
        }
        if s.len() < OCTAL_MODE_MIN_LEN || s.len() > OCTAL_MODE_MAX_LEN {
            return Err(format!(
                "invalid mode length '{s}': must be {OCTAL_MODE_MIN_LEN} or {OCTAL_MODE_MAX_LEN} digits"
            ));
        }
        if let Some(c) = s.chars().find(|&c| c > '7') {
            return Err(format!("invalid octal digit '{c}' in mode '{s}'"));
        }
        let bits = u32::from_str_radix(s, 8).map_err(|e| e.to_string())?;
        Ok(Self {
            raw: s.to_string(),
            bits,
        })
    }

    /// The numeric permission bits.
    #[must_use]
    #[cfg_attr(
        not(unix),
        allow(
            dead_code,
            reason = "numeric chmod bits are only consumed by Unix permission operations"
        )
    )]
    pub const fn as_u32(&self) -> u32 {
        self.bits
    }

    /// The original string representation.
    #[must_use]
    #[cfg(any(test, feature = "internal-api", doctest))]
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

impl std::fmt::Display for OctalMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.raw)
    }
}
