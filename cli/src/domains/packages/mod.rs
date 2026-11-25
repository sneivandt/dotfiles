//! Packages domain: system package installation across providers.

/// Canonical executable installed by the Arch `paru` package.
pub(crate) const PARU_EXECUTABLE: &str = "/usr/bin/paru";
/// Package name recorded in the target Arch package database.
pub(crate) const PARU_PACKAGE: &str = "paru";

pub(crate) mod arch_recipes;
pub(crate) mod config;
pub(crate) mod install;
pub(crate) mod resources;

#[cfg(test)]
mod test_fixture;
