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
/// # Arguments
///
/// * `section_categories` - Categories from the INI section header
/// * `active_categories` - Currently active categories from the profile
/// * `mode` - Whether to match all (AND) or any (OR)
///
/// # Examples
///
/// ```
/// use dotfiles_cli::config::category_matcher::{MatchMode, matches};
///
/// let section = vec!["arch".to_string(), "desktop".to_string()];
/// let active = vec!["arch".to_string(), "base".to_string()];
///
/// // AND mode: both "arch" and "desktop" must be active
/// assert!(!matches(&section, &active, MatchMode::All));
///
/// // OR mode: at least one of "arch" or "desktop" must be active
/// assert!(matches(&section, &active, MatchMode::Any));
/// ```
#[must_use]
pub fn matches(
    section_categories: &[String],
    active_categories: &[String],
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
        let section = vec!["arch".to_string(), "desktop".to_string()];
        let active_both = vec!["arch".to_string(), "desktop".to_string()];
        let active_one = vec!["arch".to_string()];

        assert!(matches(&section, &active_both, MatchMode::All));
        assert!(!matches(&section, &active_one, MatchMode::All));
    }

    #[test]
    fn any_mode_requires_at_least_one_category() {
        let section = vec!["arch".to_string(), "desktop".to_string()];
        let active_one = vec!["arch".to_string()];
        let active_miss = vec!["windows".to_string()];

        assert!(matches(&section, &active_one, MatchMode::Any));
        assert!(!matches(&section, &active_miss, MatchMode::Any));
    }

    #[test]
    fn all_mode_single_category_match() {
        let section = vec!["base".to_string()];
        let active = vec!["base".to_string(), "arch".to_string()];

        assert!(matches(&section, &active, MatchMode::All));
    }

    #[test]
    fn all_mode_single_category_no_match() {
        let section = vec!["desktop".to_string()];
        let active = vec!["base".to_string(), "arch".to_string()];

        assert!(!matches(&section, &active, MatchMode::All));
    }

    #[test]
    fn empty_section_categories_all_mode() {
        let section: Vec<String> = vec![];
        let active = vec!["arch".to_string()];

        // all() on empty iterator returns true (vacuous truth)
        assert!(matches(&section, &active, MatchMode::All));
    }

    #[test]
    fn empty_section_categories_any_mode() {
        let section: Vec<String> = vec![];
        let active = vec!["arch".to_string()];

        // any() on empty iterator returns false
        assert!(!matches(&section, &active, MatchMode::Any));
    }

    #[test]
    fn empty_active_categories_all_mode() {
        let section = vec!["arch".to_string()];
        let active: Vec<String> = vec![];

        assert!(!matches(&section, &active, MatchMode::All));
    }

    #[test]
    fn empty_active_categories_any_mode() {
        let section = vec!["arch".to_string()];
        let active: Vec<String> = vec![];

        assert!(!matches(&section, &active, MatchMode::Any));
    }

    #[test]
    fn both_empty_all_mode() {
        let section: Vec<String> = vec![];
        let active: Vec<String> = vec![];

        // all() on empty iterator returns true (vacuous truth)
        assert!(matches(&section, &active, MatchMode::All));
    }

    #[test]
    fn both_empty_any_mode() {
        let section: Vec<String> = vec![];
        let active: Vec<String> = vec![];

        assert!(!matches(&section, &active, MatchMode::Any));
    }
}
