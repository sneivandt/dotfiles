use std::path::Path;

use crate::platform::Platform;

/// Valid Windows registry hives for validation.
const VALID_REGISTRY_HIVES: &[&str] = &["HKCU:", "HKLM:", "HKCR:", "HKU:", "HKCC:"];

/// Minimum length for octal mode strings.
const OCTAL_MODE_MIN_LEN: usize = 3;

/// Maximum length for octal mode strings.
const OCTAL_MODE_MAX_LEN: usize = 4;

/// A validation warning detected during configuration loading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationWarning {
    /// The configuration source (e.g., "symlinks.ini", "packages.ini").
    pub source: String,
    /// The specific item or section that triggered the warning.
    pub item: String,
    /// Human-readable warning message.
    pub message: String,
}

impl ValidationWarning {
    #[must_use]
    pub fn new(
        source: impl Into<String>,
        item: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            source: source.into(),
            item: item.into(),
            message: message.into(),
        }
    }
}

/// Trait for configuration validators.
///
/// Implementations should check configuration for common issues:
/// - Missing required fields
/// - Invalid values
/// - Platform incompatibilities
/// - Non-existent file paths
pub trait ConfigValidator {
    /// Validate the configuration and return any warnings found.
    fn validate(&self, root: &Path, platform: &Platform) -> Vec<ValidationWarning>;

    /// Return a human-readable name for this validator (e.g., "symlinks", "packages").
    #[allow(dead_code)] // Part of trait contract; implementors define it
    fn name(&self) -> &'static str;
}

/// Validator for symlink configurations.
#[derive(Debug)]
pub struct SymlinkValidator<'a> {
    symlinks: &'a [super::symlinks::Symlink],
}

impl<'a> SymlinkValidator<'a> {
    #[must_use]
    pub const fn new(symlinks: &'a [super::symlinks::Symlink]) -> Self {
        Self { symlinks }
    }
}

impl ConfigValidator for SymlinkValidator<'_> {
    fn validate(&self, root: &Path, _platform: &Platform) -> Vec<ValidationWarning> {
        let mut warnings = Vec::new();
        let symlinks_dir = root.join("symlinks");

        for symlink in self.symlinks {
            let source_path = symlinks_dir.join(&symlink.source);

            // Check if source file exists
            if !source_path.exists() {
                warnings.push(ValidationWarning::new(
                    "symlinks.ini",
                    &symlink.source,
                    format!("source file does not exist: {}", source_path.display()),
                ));
            }

            // Check for absolute paths (should be relative)
            if Path::new(&symlink.source).is_absolute() || symlink.source.starts_with('/') {
                warnings.push(ValidationWarning::new(
                    "symlinks.ini",
                    &symlink.source,
                    "source path should be relative to symlinks/ directory",
                ));
            }
        }

        warnings
    }

    fn name(&self) -> &'static str {
        "symlinks"
    }
}

/// Validator for package configurations.
#[derive(Debug)]
pub struct PackageValidator<'a> {
    packages: &'a [super::packages::Package],
}

impl<'a> PackageValidator<'a> {
    #[must_use]
    pub const fn new(packages: &'a [super::packages::Package]) -> Self {
        Self { packages }
    }
}

impl ConfigValidator for PackageValidator<'_> {
    fn validate(&self, _root: &Path, platform: &Platform) -> Vec<ValidationWarning> {
        let mut warnings = Vec::new();

        for package in self.packages {
            // Warn about AUR packages on non-Arch platforms
            if package.is_aur && !platform.is_arch_linux() {
                warnings.push(ValidationWarning::new(
                    "packages.ini",
                    &package.name,
                    "AUR package specified but platform is not Arch Linux",
                ));
            }

            // Check for empty package names
            if package.name.trim().is_empty() {
                warnings.push(ValidationWarning::new(
                    "packages.ini",
                    &package.name,
                    "package name is empty",
                ));
            }
        }

        warnings
    }

    fn name(&self) -> &'static str {
        "packages"
    }
}

/// Validator for registry configurations.
#[derive(Debug)]
pub struct RegistryValidator<'a> {
    entries: &'a [super::registry::RegistryEntry],
}

impl<'a> RegistryValidator<'a> {
    #[must_use]
    pub const fn new(entries: &'a [super::registry::RegistryEntry]) -> Self {
        Self { entries }
    }
}

impl ConfigValidator for RegistryValidator<'_> {
    fn validate(&self, _root: &Path, platform: &Platform) -> Vec<ValidationWarning> {
        let mut warnings = Vec::new();

        // Warn if registry entries are defined on non-Windows platform
        if !self.entries.is_empty() && !platform.has_registry() {
            warnings.push(ValidationWarning::new(
                "registry.ini",
                "registry entries",
                "registry entries defined but platform does not support registry",
            ));
        }

        for entry in self.entries {
            // Check for empty key paths
            if entry.key_path.trim().is_empty() {
                warnings.push(ValidationWarning::new(
                    "registry.ini",
                    &entry.value_name,
                    "registry key path is empty",
                ));
            }

            // Check for empty value names
            if entry.value_name.trim().is_empty() {
                warnings.push(ValidationWarning::new(
                    "registry.ini",
                    &entry.key_path,
                    "registry value name is empty",
                ));
            }

            // Validate registry key format (should start with HKCU:, HKLM:, etc.)
            let path_upper = entry.key_path.to_uppercase();
            if !VALID_REGISTRY_HIVES
                .iter()
                .any(|hive| path_upper.starts_with(hive))
            {
                warnings.push(ValidationWarning::new(
                    "registry.ini",
                    &entry.key_path,
                    "registry key path should start with a valid hive (HKCU:, HKLM:, etc.)",
                ));
            }
        }

        warnings
    }

    fn name(&self) -> &'static str {
        "registry"
    }
}

/// Validates an octal mode string (e.g., "644", "0755").
///
/// Returns `Some(error_message)` if the mode is invalid, or `None` if valid.
fn validate_octal_mode(mode: &str) -> Option<String> {
    if !mode.chars().all(|c| c.is_ascii_digit()) {
        return Some(format!(
            "invalid octal mode '{mode}': must contain only digits"
        ));
    }

    if mode.len() < OCTAL_MODE_MIN_LEN || mode.len() > OCTAL_MODE_MAX_LEN {
        return Some(format!(
            "invalid mode length '{mode}': must be {OCTAL_MODE_MIN_LEN} or {OCTAL_MODE_MAX_LEN} digits"
        ));
    }

    // Check each digit is valid octal (0-7)
    if let Some(c) = mode.chars().find(|&c| c > '7') {
        return Some(format!("invalid octal digit '{c}' in mode '{mode}'"));
    }

    None
}

/// Validator for chmod configurations.
#[derive(Debug)]
pub struct ChmodValidator<'a> {
    entries: &'a [super::chmod::ChmodEntry],
}

impl<'a> ChmodValidator<'a> {
    #[must_use]
    pub const fn new(entries: &'a [super::chmod::ChmodEntry]) -> Self {
        Self { entries }
    }
}

impl ConfigValidator for ChmodValidator<'_> {
    fn validate(&self, _root: &Path, platform: &Platform) -> Vec<ValidationWarning> {
        let mut warnings = Vec::new();

        // Warn if chmod entries are defined on non-Unix platform
        if !self.entries.is_empty() && !platform.supports_chmod() {
            warnings.push(ValidationWarning::new(
                "chmod.ini",
                "chmod entries",
                "chmod entries defined but platform does not support chmod",
            ));
        }

        for entry in self.entries {
            // Validate mode is octal (3 or 4 digits)
            if let Some(error) = validate_octal_mode(&entry.mode) {
                warnings.push(ValidationWarning::new("chmod.ini", &entry.path, error));
            }

            // Check for absolute paths (should be relative to $HOME)
            if Path::new(&entry.path).is_absolute() {
                warnings.push(ValidationWarning::new(
                    "chmod.ini",
                    &entry.path,
                    "path should be relative to $HOME directory",
                ));
            }
        }

        warnings
    }

    fn name(&self) -> &'static str {
        "chmod"
    }
}

/// Validator for systemd unit configurations.
#[derive(Debug)]
pub struct SystemdUnitValidator<'a> {
    units: &'a [super::systemd_units::SystemdUnit],
}

impl<'a> SystemdUnitValidator<'a> {
    #[must_use]
    pub const fn new(units: &'a [super::systemd_units::SystemdUnit]) -> Self {
        Self { units }
    }
}

impl ConfigValidator for SystemdUnitValidator<'_> {
    fn validate(&self, _root: &Path, platform: &Platform) -> Vec<ValidationWarning> {
        const VALID_UNIT_EXTENSIONS: &[&str] = &[
            ".service", ".timer", ".socket", ".target", ".path", ".mount",
        ];

        let mut warnings = Vec::new();

        // Warn if units are defined on non-systemd platform
        if !self.units.is_empty() && !platform.supports_systemd() {
            warnings.push(ValidationWarning::new(
                "systemd-units.ini",
                "systemd units",
                "systemd units defined but platform does not support systemd",
            ));
        }

        for unit in self.units {
            // Check for empty unit names
            if unit.name.trim().is_empty() {
                warnings.push(ValidationWarning::new(
                    "systemd-units.ini",
                    &unit.name,
                    "unit name is empty",
                ));
            }

            // Validate unit name has proper extension
            // Note: systemd unit extensions are case-sensitive on Linux
            #[allow(clippy::case_sensitive_file_extension_comparisons)]
            if !VALID_UNIT_EXTENSIONS
                .iter()
                .any(|ext| unit.name.ends_with(ext))
            {
                warnings.push(ValidationWarning::new(
                    "systemd-units.ini",
                    &unit.name,
                    "unit name should end with a valid systemd extension (.service, .timer, .socket, etc.)",
                ));
            }
        }

        warnings
    }

    fn name(&self) -> &'static str {
        "systemd-units"
    }
}

/// Validator for VS Code extension configurations.
#[derive(Debug)]
pub struct VsCodeExtensionValidator<'a> {
    extensions: &'a [super::vscode_extensions::VsCodeExtension],
}

impl<'a> VsCodeExtensionValidator<'a> {
    #[must_use]
    pub const fn new(extensions: &'a [super::vscode_extensions::VsCodeExtension]) -> Self {
        Self { extensions }
    }
}

impl ConfigValidator for VsCodeExtensionValidator<'_> {
    fn validate(&self, _root: &Path, _platform: &Platform) -> Vec<ValidationWarning> {
        let mut warnings = Vec::new();

        for extension in self.extensions {
            // Check for empty extension IDs
            if extension.id.trim().is_empty() {
                warnings.push(ValidationWarning::new(
                    "vscode-extensions.ini",
                    &extension.id,
                    "extension ID is empty",
                ));
            }

            // Validate extension ID format (should be publisher.name)
            if !extension.id.contains('.') {
                warnings.push(ValidationWarning::new(
                    "vscode-extensions.ini",
                    &extension.id,
                    "extension ID should be in format 'publisher.name'",
                ));
            }
        }

        warnings
    }

    fn name(&self) -> &'static str {
        "vscode-extensions"
    }
}

/// Validator for Copilot skill configurations.
#[derive(Debug)]
pub struct CopilotSkillValidator<'a> {
    skills: &'a [super::copilot_skills::CopilotSkill],
}

impl<'a> CopilotSkillValidator<'a> {
    #[must_use]
    pub const fn new(skills: &'a [super::copilot_skills::CopilotSkill]) -> Self {
        Self { skills }
    }
}

impl ConfigValidator for CopilotSkillValidator<'_> {
    fn validate(&self, _root: &Path, _platform: &Platform) -> Vec<ValidationWarning> {
        let mut warnings = Vec::new();

        for skill in self.skills {
            // Check for empty skill URLs
            if skill.url.trim().is_empty() {
                warnings.push(ValidationWarning::new(
                    "copilot-skills.ini",
                    &skill.url,
                    "skill URL is empty",
                ));
            }

            // Validate URL format (should be a valid URL)
            if !skill.url.starts_with("http://") && !skill.url.starts_with("https://") {
                warnings.push(ValidationWarning::new(
                    "copilot-skills.ini",
                    &skill.url,
                    "skill URL should start with http:// or https://",
                ));
            }
        }

        warnings
    }

    fn name(&self) -> &'static str {
        "copilot-skills"
    }
}

/// Validate all configuration and return collected warnings.
#[must_use]
pub fn validate_all(config: &super::Config, platform: &Platform) -> Vec<ValidationWarning> {
    let validators: Vec<Box<dyn ConfigValidator>> = vec![
        Box::new(SymlinkValidator::new(&config.symlinks)),
        Box::new(PackageValidator::new(&config.packages)),
        Box::new(RegistryValidator::new(&config.registry)),
        Box::new(ChmodValidator::new(&config.chmod)),
        Box::new(SystemdUnitValidator::new(&config.units)),
        Box::new(VsCodeExtensionValidator::new(&config.vscode_extensions)),
        Box::new(CopilotSkillValidator::new(&config.copilot_skills)),
    ];

    let mut all_warnings = Vec::new();
    for validator in validators {
        let warnings = validator.validate(&config.root, platform);
        all_warnings.extend(warnings);
    }

    all_warnings
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::platform::Os;

    #[test]
    fn symlink_validator_detects_missing_source() {
        let temp_dir = tempfile::tempdir().unwrap();
        let symlinks = vec![super::super::symlinks::Symlink {
            source: "nonexistent.txt".to_string(),
        }];

        let validator = SymlinkValidator::new(&symlinks);
        let warnings = validator.validate(temp_dir.path(), &Platform::new(Os::Linux, false));

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("does not exist"));
    }

    #[test]
    fn symlink_validator_detects_absolute_path() {
        let symlinks = vec![super::super::symlinks::Symlink {
            source: "/absolute/path".to_string(),
        }];

        let temp_dir = tempfile::tempdir().unwrap();
        let validator = SymlinkValidator::new(&symlinks);
        let warnings = validator.validate(temp_dir.path(), &Platform::new(Os::Linux, false));

        // Expect 2 warnings: non-existent file AND absolute path
        assert_eq!(warnings.len(), 2);
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("should be relative"))
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("does not exist"))
        );
    }

    #[test]
    fn package_validator_warns_aur_on_non_arch() {
        let packages = vec![super::super::packages::Package {
            name: "yay".to_string(),
            is_aur: true,
        }];

        let platform = Platform::new(Os::Linux, false);

        let validator = PackageValidator::new(&packages);
        let warnings = validator.validate(Path::new("/tmp"), &platform);

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("not Arch Linux"));
    }

    #[test]
    fn chmod_validator_detects_invalid_mode() {
        let entries = vec![super::super::chmod::ChmodEntry {
            mode: "999".to_string(),
            path: ".ssh/config".to_string(),
        }];

        let validator = ChmodValidator::new(&entries);
        let warnings = validator.validate(Path::new("/tmp"), &Platform::new(Os::Linux, false));

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("invalid octal digit"));
    }

    #[test]
    fn chmod_validator_detects_invalid_mode_length() {
        let entries = vec![super::super::chmod::ChmodEntry {
            mode: "12".to_string(),
            path: ".ssh/config".to_string(),
        }];

        let validator = ChmodValidator::new(&entries);
        let warnings = validator.validate(Path::new("/tmp"), &Platform::new(Os::Linux, false));

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("must be 3 or 4 digits"));
    }

    #[test]
    fn registry_validator_warns_on_non_windows() {
        let entries = vec![super::super::registry::RegistryEntry {
            key_path: "HKCU:\\Console".to_string(),
            value_name: "FontSize".to_string(),
            value_data: "14".to_string(),
        }];

        let platform = Platform::new(Os::Linux, true);

        let validator = RegistryValidator::new(&entries);
        let warnings = validator.validate(Path::new("/tmp"), &platform);

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("does not support registry"));
    }

    #[test]
    fn registry_validator_detects_invalid_hive() {
        let entries = vec![super::super::registry::RegistryEntry {
            key_path: "INVALID:\\Path".to_string(),
            value_name: "Test".to_string(),
            value_data: "Value".to_string(),
        }];

        let validator = RegistryValidator::new(&entries);
        let warnings = validator.validate(Path::new("/tmp"), &Platform::new(Os::Windows, false));

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("valid hive"));
    }

    #[test]
    fn registry_validator_accepts_case_insensitive_hive() {
        let entries = vec![
            super::super::registry::RegistryEntry {
                key_path: "hkcu:\\Console".to_string(),
                value_name: "FontSize".to_string(),
                value_data: "14".to_string(),
            },
            super::super::registry::RegistryEntry {
                key_path: "HkLm:\\Software".to_string(),
                value_name: "Test".to_string(),
                value_data: "Value".to_string(),
            },
        ];

        let validator = RegistryValidator::new(&entries);
        let warnings = validator.validate(Path::new("/tmp"), &Platform::new(Os::Windows, false));

        // Should not have warnings about invalid hives
        assert!(
            !warnings.iter().any(|w| w.message.contains("valid hive")),
            "Should accept case-insensitive hive names"
        );
    }

    #[test]
    fn units_validator_detects_invalid_extension() {
        let units = vec![super::super::systemd_units::SystemdUnit {
            name: "myunit".to_string(),
        }];

        let validator = SystemdUnitValidator::new(&units);
        let warnings = validator.validate(Path::new("/tmp"), &Platform::new(Os::Linux, false));

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("valid systemd extension"));
    }

    #[test]
    fn vscode_validator_detects_invalid_format() {
        let extensions = vec![super::super::vscode_extensions::VsCodeExtension {
            id: "invalid_no_publisher".to_string(),
        }];

        let validator = VsCodeExtensionValidator::new(&extensions);
        let warnings = validator.validate(Path::new("/tmp"), &Platform::new(Os::Linux, false));

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("publisher.name"));
    }

    #[test]
    fn validate_octal_mode_accepts_valid_modes() {
        assert_eq!(validate_octal_mode("644"), None);
        assert_eq!(validate_octal_mode("755"), None);
        assert_eq!(validate_octal_mode("0644"), None);
        assert_eq!(validate_octal_mode("0755"), None);
        assert_eq!(validate_octal_mode("600"), None);
        assert_eq!(validate_octal_mode("777"), None);
    }

    #[test]
    fn validate_octal_mode_rejects_non_digits() {
        let result = validate_octal_mode("abc");
        assert!(result.is_some());
        assert!(result.unwrap().contains("must contain only digits"));
    }

    #[test]
    fn validate_octal_mode_rejects_invalid_length() {
        let result = validate_octal_mode("12");
        assert!(result.is_some());
        assert!(result.unwrap().contains("must be 3 or 4 digits"));

        let result = validate_octal_mode("12345");
        assert!(result.is_some());
        assert!(result.unwrap().contains("must be 3 or 4 digits"));
    }

    #[test]
    fn validate_octal_mode_rejects_invalid_octal_digits() {
        let result = validate_octal_mode("888");
        assert!(result.is_some());
        assert!(result.unwrap().contains("invalid octal digit '8'"));

        let result = validate_octal_mode("799");
        assert!(result.is_some());
        assert!(result.unwrap().contains("invalid octal digit '9'"));
    }
}
