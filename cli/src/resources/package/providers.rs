//! Concrete package manager providers.

use std::collections::HashSet;

use anyhow::Result;

use super::PackageProvider;
use crate::exec::Executor;
use crate::resources::ResourceChange;

#[derive(Clone, Copy)]
enum ParseMode {
    FirstToken,
}

fn query_names(
    executor: &dyn Executor,
    cmd: &str,
    args: &[&str],
    mode: ParseMode,
) -> Result<HashSet<String>> {
    let result = executor.run_unchecked(cmd, args)?;
    if !result.success {
        anyhow::bail!(
            "{cmd} query failed (exit {:?}): {}",
            result.code,
            result.stderr.trim()
        );
    }
    let mut set = HashSet::new();
    for line in result.stdout.lines() {
        match mode {
            ParseMode::FirstToken => {
                if let Some(name) = line.split_whitespace().next() {
                    set.insert(name.to_string());
                }
            }
        }
    }
    Ok(set)
}

fn split_padded_columns(line: &str) -> Vec<String> {
    let mut cols = Vec::new();
    let mut current = String::new();
    let mut spaces = 0usize;

    for ch in line.chars() {
        if ch == ' ' {
            spaces += 1;
            if spaces < 2 {
                continue;
            }

            if !current.is_empty() {
                cols.push(std::mem::take(&mut current));
            }
        } else {
            if spaces == 1 {
                current.push(' ');
            }
            spaces = 0;
            current.push(ch);
        }
    }

    if !current.is_empty() {
        cols.push(current);
    }

    cols
}

fn byte_offset_of_col(line: &str, col_idx: usize) -> Option<usize> {
    let mut col = 0usize;
    let mut in_col = false;
    let mut spaces = 0usize;

    for (i, ch) in line.char_indices() {
        if ch == ' ' {
            spaces += 1;
            if in_col && spaces >= 2 {
                col += 1;
                in_col = false;
            }
        } else {
            spaces = 0;
            if !in_col {
                if col == col_idx {
                    return Some(i);
                }
                in_col = true;
            }
        }
    }
    None
}

pub(super) fn parse_winget_ids(stdout: &str) -> HashSet<String> {
    let mut ids = HashSet::new();
    let mut id_col_idx: Option<usize> = None;

    for line in stdout.lines() {
        if let Some(col_idx) = id_col_idx {
            if line.bytes().all(|b| b == b'-' || b == b' ') {
                continue;
            }
            if let Some(start) = byte_offset_of_col(line, col_idx)
                && let Some(slice) = line.get(start..)
            {
                let id = slice
                    .find("  ")
                    .map_or_else(|| slice.trim(), |sep_pos| slice[..sep_pos].trim());
                if !id.is_empty() {
                    ids.insert(id.to_string());
                }
            }
        } else {
            let cols = split_padded_columns(line);
            if let Some(id_idx) = cols.iter().position(|c| c == "Id") {
                id_col_idx = Some(id_idx);
            }
        }
    }

    ids
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PacmanProvider;

impl PackageProvider for PacmanProvider {
    fn name(&self) -> &'static str {
        "pacman"
    }

    fn query_installed(&self, executor: &dyn Executor) -> Result<HashSet<String>> {
        query_names(executor, "pacman", &["-Q"], ParseMode::FirstToken)
    }

    fn is_installed(&self, name: &str, executor: &dyn Executor) -> Result<bool> {
        let result = executor.run_unchecked("pacman", &["-Q", name])?;
        Ok(result.success)
    }

    fn install(&self, name: &str, executor: &dyn Executor) -> Result<ResourceChange> {
        executor.run("sudo", &["pacman", "-Sy", "--needed", "--noconfirm", name])?;
        Ok(ResourceChange::Applied)
    }

    fn supports_batch(&self) -> bool {
        true
    }

    fn batch_install(&self, names: &[&str], executor: &dyn Executor) -> Result<()> {
        let mut args = vec!["pacman", "-Sy", "--needed", "--noconfirm"];
        args.extend(names);
        executor.run("sudo", &args)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ParuProvider;

impl PackageProvider for ParuProvider {
    fn name(&self) -> &'static str {
        "paru"
    }

    fn query_installed(&self, executor: &dyn Executor) -> Result<HashSet<String>> {
        PacmanProvider.query_installed(executor)
    }

    fn is_installed(&self, name: &str, executor: &dyn Executor) -> Result<bool> {
        PacmanProvider.is_installed(name, executor)
    }

    fn install(&self, name: &str, executor: &dyn Executor) -> Result<ResourceChange> {
        executor.run("paru", &["-S", "--needed", "--noconfirm", name])?;
        Ok(ResourceChange::Applied)
    }

    fn supports_batch(&self) -> bool {
        true
    }

    fn batch_install(&self, names: &[&str], executor: &dyn Executor) -> Result<()> {
        let mut args = vec!["-S", "--needed", "--noconfirm"];
        args.extend(names);
        executor.run("paru", &args)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct WingetProvider;

impl PackageProvider for WingetProvider {
    fn name(&self) -> &'static str {
        "winget"
    }

    fn query_installed(&self, executor: &dyn Executor) -> Result<HashSet<String>> {
        let result = executor.run_unchecked(
            "winget",
            &[
                "list",
                "--accept-source-agreements",
                "--disable-interactivity",
            ],
        )?;

        if !result.success {
            anyhow::bail!(
                "winget list failed (exit {:?}): {}",
                result.code,
                result.stderr.trim()
            );
        }

        Ok(parse_winget_ids(&result.stdout))
    }

    fn is_installed(&self, name: &str, executor: &dyn Executor) -> Result<bool> {
        let result = executor.run_unchecked(
            "winget",
            &[
                "list",
                "--id",
                name,
                "--exact",
                "--accept-source-agreements",
            ],
        )?;
        Ok(result.success && result.stdout.contains(name))
    }

    fn install(&self, name: &str, executor: &dyn Executor) -> Result<ResourceChange> {
        let result = executor.run_unchecked(
            "winget",
            &[
                "install",
                "--id",
                name,
                "--exact",
                "--source",
                "winget",
                "--accept-source-agreements",
                "--accept-package-agreements",
            ],
        )?;
        if result.success {
            Ok(ResourceChange::Applied)
        } else {
            let detail = if result.stderr.trim().is_empty() {
                result.stdout.trim().to_string()
            } else {
                format!("{}\n{}", result.stdout.trim(), result.stderr.trim())
            };
            Ok(ResourceChange::Skipped {
                reason: format!("winget install failed: {detail}"),
            })
        }
    }
}
