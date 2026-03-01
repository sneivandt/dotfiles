//! Category tag matching for profile-based filtering.
use std::fmt;

/// A profile category tag used for configuration filtering.
///
/// Categories are used to group configuration items and determine which ones
/// are active for a given profile and platform. Known categories correspond to
/// well-understood filtering axes; custom categories are supported via [`Category::Other`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Category {
    /// Core configuration included in all profiles.
    Base,
    /// Linux-specific configuration.
    Linux,
    /// Windows-specific configuration.
    Windows,
    /// Arch Linux-specific configuration.
    Arch,
    /// Desktop/GUI configuration.
    Desktop,
    /// A custom or user-defined category not covered by the known variants.
    Other(String),
}

impl Category {
    /// Parse a category tag from a string slice.
    ///
    /// The input is case-insensitive and trimmed. Any value not matching a known
    /// category becomes [`Category::Other`].
    ///
    /// # Examples
    ///
    /// ```
    /// use dotfiles_cli::config::category_matcher::Category;
    ///
    /// assert_eq!(Category::from_tag("arch"), Category::Arch);
    /// assert_eq!(Category::from_tag("DESKTOP"), Category::Desktop);
    /// assert_eq!(Category::from_tag("custom"), Category::Other("custom".to_string()));
    /// ```
    #[must_use]
    pub fn from_tag(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "base" => Self::Base,
            "linux" => Self::Linux,
            "windows" => Self::Windows,
            "arch" => Self::Arch,
            "desktop" => Self::Desktop,
            other => Self::Other(other.to_string()),
        }
    }

    /// Returns the canonical string representation of this category.
    ///
    /// # Examples
    ///
    /// ```
    /// use dotfiles_cli::config::category_matcher::Category;
    ///
    /// assert_eq!(Category::Arch.as_str(), "arch");
    /// assert_eq!(Category::Other("custom".to_string()).as_str(), "custom");
    /// ```
    #[must_use]
    pub const fn as_str(&self) -> &str {
        match self {
            Self::Base => "base",
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::Arch => "arch",
            Self::Desktop => "desktop",
            Self::Other(s) => s.as_str(),
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialOrd for Category {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Category {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}

/// Match mode for category filtering.
///
/// Controls whether all or any of a section's categories must be active
/// for the section to be considered a match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    /// All categories must be active (AND logic).
    All,
    /// Any category must be active (OR logic).
    Any,
}

/// Check if a section's categories match the active categories.
///
/// # Examples
///
/// ```
/// use dotfiles_cli::config::category_matcher::{Category, MatchMode, matches};
///
/// let section = vec![Category::Arch, Category::Desktop];
/// let active = vec![Category::Arch, Category::Base];
///
/// // AND mode: both "arch" and "desktop" must be active
/// assert!(!matches(&section, &active, MatchMode::All));
///
/// // OR mode: at least one of "arch" or "desktop" must be active
/// assert!(matches(&section, &active, MatchMode::Any));
/// ```
#[must_use]
pub fn matches(
    section_categories: &[Category],
    active_categories: &[Category],
    mode: MatchMode,
) -> bool {
    match mode {
        MatchMode::All => section_categories
            .iter()
            .all(|cat| active_categories.contains(cat)),
        MatchMode::Any => section_categories
            .iter()
            .any(|cat| active_categories.contains(cat)),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn all_mode_requires_all_categories() {
        let section = vec![Category::Arch, Category::Desktop];
        let active_both = vec![Category::Arch, Category::Desktop];
        let active_one = vec![Category::Arch];

        assert!(matches(&section, &active_both, MatchMode::All));
        assert!(!matches(&section, &active_one, MatchMode::All));
    }

    #[test]
    fn any_mode_requires_at_least_one_category() {
        let section = vec![Category::Arch, Category::Desktop];
        let active_one = vec![Category::Arch];
        let active_miss = vec![Category::Windows];

        assert!(matches(&section, &active_one, MatchMode::Any));
        assert!(!matches(&section, &active_miss, MatchMode::Any));
    }

    #[test]
    fn all_mode_single_category_match() {
        let section = vec![Category::Base];
        let active = vec![Category::Base, Category::Arch];

        assert!(matches(&section, &active, MatchMode::All));
    }

    #[test]
    fn all_mode_single_category_no_match() {
        let section = vec![Category::Desktop];
        let active = vec![Category::Base, Category::Arch];

        assert!(!matches(&section, &active, MatchMode::All));
    }

    #[test]
    fn empty_section_categories_all_mode() {
        let section: Vec<Category> = vec![];
        let active = vec![Category::Arch];

        // all() on empty iterator returns true (vacuous truth)
        assert!(matches(&section, &active, MatchMode::All));
    }

    #[test]
    fn empty_section_categories_any_mode() {
        let section: Vec<Category> = vec![];
        let active = vec![Category::Arch];

        // any() on empty iterator returns false
        assert!(!matches(&section, &active, MatchMode::Any));
    }

    #[test]
    fn empty_active_categories_all_mode() {
        let section = vec![Category::Arch];
        let active: Vec<Category> = vec![];

        assert!(!matches(&section, &active, MatchMode::All));
    }

    #[test]
    fn empty_active_categories_any_mode() {
        let section = vec![Category::Arch];
        let active: Vec<Category> = vec![];

        assert!(!matches(&section, &active, MatchMode::Any));
    }

    #[test]
    fn both_empty_all_mode() {
        let section: Vec<Category> = vec![];
        let active: Vec<Category> = vec![];

        // all() on empty iterator returns true (vacuous truth)
        assert!(matches(&section, &active, MatchMode::All));
    }

    #[test]
    fn both_empty_any_mode() {
        let section: Vec<Category> = vec![];
        let active: Vec<Category> = vec![];

        assert!(!matches(&section, &active, MatchMode::Any));
    }

    #[test]
    fn from_tag_known_variants() {
        assert_eq!(Category::from_tag("base"), Category::Base);
        assert_eq!(Category::from_tag("linux"), Category::Linux);
        assert_eq!(Category::from_tag("windows"), Category::Windows);
        assert_eq!(Category::from_tag("arch"), Category::Arch);
        assert_eq!(Category::from_tag("desktop"), Category::Desktop);
    }

    #[test]
    fn from_tag_case_insensitive() {
        assert_eq!(Category::from_tag("ARCH"), Category::Arch);
        assert_eq!(Category::from_tag("Desktop"), Category::Desktop);
        assert_eq!(Category::from_tag("  linux  "), Category::Linux);
    }

    #[test]
    fn from_tag_unknown_becomes_other() {
        assert_eq!(
            Category::from_tag("custom"),
            Category::Other("custom".to_string())
        );
    }

    #[test]
    fn display_and_as_str() {
        assert_eq!(Category::Arch.as_str(), "arch");
        assert_eq!(Category::Other("custom".to_string()).as_str(), "custom");
        assert_eq!(Category::Linux.to_string(), "linux");
    }

    #[test]
    fn ord_sorts_by_string() {
        let mut cats = vec![Category::Windows, Category::Arch, Category::Base];
        cats.sort();
        assert_eq!(
            cats,
            vec![Category::Arch, Category::Base, Category::Windows]
        );
    }
}
