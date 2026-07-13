//! Resource implementations for the packages domain.

mod pacman;
mod paru;
mod winget;

pub mod package;

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests;
