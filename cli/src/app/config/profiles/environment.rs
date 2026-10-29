//! Environment-based profile selection.

/// Try to read the profile from the `DOTFILES_PROFILE` environment variable.
///
/// # Errors
///
/// Returns an error if the selected value is not valid Unicode.
pub fn read_from_env(env: &dyn crate::infra::env::Env) -> anyhow::Result<Option<String>> {
    env.var_os("DOTFILES_PROFILE")
        .filter(|value| !value.is_empty())
        .map(|value| {
            value.into_string().map_err(|_| {
                anyhow::anyhow!(
                    "DOTFILES_PROFILE must contain valid Unicode; pass --profile <base|desktop> to override"
                )
            })
        })
        .transpose()
}
