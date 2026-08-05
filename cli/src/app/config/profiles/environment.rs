//! Environment-based profile selection.

/// Try to read the profile from the `DOTFILES_PROFILE` environment variable.
#[must_use]
pub fn read_from_env(env: &dyn crate::infra::env::Env) -> Option<String> {
    parse_env_profile(env.var("DOTFILES_PROFILE"))
}

pub(super) fn parse_env_profile(raw: Option<String>) -> Option<String> {
    raw.filter(|value| !value.is_empty())
}
