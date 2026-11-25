//! Resource implementations for the dotfiles core domain.

pub(crate) mod path_entry;
pub(crate) mod wrapper;

#[cfg(test)]
#[path = "tests/path_entry.rs"]
mod tests;
