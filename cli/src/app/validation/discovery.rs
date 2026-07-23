//! Repository file discovery for validation tasks.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

/// Recursively discover files in a directory tree that match a predicate.
pub(crate) fn discover_files<F>(dir: &Path, predicate: F, out: &mut Vec<PathBuf>)
where
    F: Fn(&Path) -> bool + Copy,
{
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            discover_files(&path, predicate, out);
        } else if path.is_file() && predicate(&path) {
            out.push(path);
        }
    }
}

/// Recursively discover shell scripts in a directory.
pub(crate) fn discover_shell_scripts(dir: &Path, out: &mut Vec<PathBuf>) {
    discover_files(
        dir,
        |path| {
            if path.extension().is_some_and(|extension| extension == "zsh") {
                return false;
            }
            path.extension().is_some_and(|extension| extension == "sh") || is_shell_shebang(path)
        },
        out,
    );
}

/// Recursively discover `PowerShell` scripts in a directory.
pub(crate) fn discover_powershell_scripts(dir: &Path, out: &mut Vec<PathBuf>) {
    discover_files(
        dir,
        |path| {
            path.extension().is_some_and(|extension| {
                extension == "ps1" || extension == "psm1" || extension == "psd1"
            }) || is_powershell_shebang(path)
        },
        out,
    );
}

/// Discover local APM plugin directories.
pub(crate) fn discover_apm_plugin_dirs(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("reading APM plugins directory {}", dir.display()));
        }
    };

    let mut plugins = Vec::new();
    for entry in entries {
        let entry =
            entry.with_context(|| format!("reading directory entry in {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() && path.join("apm.yml").is_file() {
            plugins.push(path);
        }
    }
    plugins.sort();
    Ok(plugins)
}

const SHELL_INTERPRETERS: &[&[u8]] = &[b"sh", b"bash", b"dash", b"ksh"];
const POWERSHELL_INTERPRETERS: &[&[u8]] = &[b"pwsh", b"powershell"];

fn is_shell_shebang(path: &Path) -> bool {
    shebang_matches(path, SHELL_INTERPRETERS)
}

fn is_powershell_shebang(path: &Path) -> bool {
    shebang_matches(path, POWERSHELL_INTERPRETERS)
}

fn shebang_matches(path: &Path, interpreters: &[&[u8]]) -> bool {
    parse_shebang_interpreter(path).is_some_and(|name| {
        let trimmed = name.strip_suffix(b".exe").unwrap_or(name.as_slice());
        interpreters.contains(&trimmed)
    })
}

fn parse_shebang_interpreter(path: &Path) -> Option<Vec<u8>> {
    let first_line = read_first_line(path);
    if !first_line.starts_with(b"#!") {
        return None;
    }
    let shebang = first_line.get(2..).unwrap_or(&[]);
    let mut tokens = shebang
        .split(|&byte| byte == b' ' || byte == b'\t')
        .filter(|token| !token.is_empty());
    let program_path = tokens.next()?;
    let program = program_path
        .rsplit(|&byte| byte == b'/')
        .next()
        .filter(|name| !name.is_empty())?;
    let program = program.strip_suffix(b".exe").unwrap_or(program);
    if program == b"env" {
        tokens
            .find(|token| !token.starts_with(b"-"))
            .map(<[u8]>::to_vec)
    } else {
        Some(program.to_vec())
    }
}

fn read_first_line(path: &Path) -> Vec<u8> {
    use std::io::Read as _;

    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut buffer = [0_u8; 256];
    let count = file.read(&mut buffer).unwrap_or(0);
    let end = buffer
        .get(..count)
        .and_then(|slice| slice.iter().position(|&byte| byte == b'\n'))
        .unwrap_or(count);
    buffer.get(..end).unwrap_or_default().to_vec()
}
