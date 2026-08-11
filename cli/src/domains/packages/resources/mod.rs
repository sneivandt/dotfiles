//! Resource implementations for the packages domain.

mod pacman;
mod paru;
mod winget;

pub mod package;
pub mod report;

#[cfg(test)]
mod tests;
