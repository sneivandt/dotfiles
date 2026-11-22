use super::*;
use crate::domains::files::config::symlinks::Symlink;
use crate::engine::{RemovableResource as _, ResourceChange};
use crate::infra::ConfigHandle;
use crate::test_helpers::make_linux_context;
use crate::test_helpers::symlink;
use std::path::PathBuf;

fn handle(symlinks: Vec<Symlink>) -> ConfigHandle<Vec<Symlink>> {
    ConfigHandle::new(symlinks)
}

#[path = "symlinks_installation.rs"]
mod installation;
#[path = "symlinks_removal.rs"]
mod removal;
#[path = "symlinks_targets.rs"]
mod targets;
