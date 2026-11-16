//! Resource implementations for the packages domain.

mod pacman;
mod paru;
mod winget;

#[cfg(test)]
pub(super) use winget::mock_inventory;

pub mod package;
pub mod report;

#[cfg(test)]
mod tests;
