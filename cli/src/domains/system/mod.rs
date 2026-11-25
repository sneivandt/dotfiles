//! System domain: registry, systemd units, developer mode, WSL.

pub(crate) mod config;
pub(crate) mod developer_mode;
pub(crate) mod registry;
pub(crate) mod resources;
pub(crate) mod system_files;
pub(crate) mod systemd_units;

#[cfg(test)]
mod systemd_fixture;
