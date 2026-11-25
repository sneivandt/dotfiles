//! Resource implementations for the packages domain.

mod pacman;
mod paru;
mod winget;

#[cfg(test)]
pub(super) use winget::mock_inventory;

pub(crate) mod package;
pub(crate) mod report;

#[cfg(test)]
mod tests;
