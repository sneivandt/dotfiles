//! Repository file discovery for validation tasks.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

/// Recursively discover files in a directory tree that match a predicate.
pub(crate) fn discover_files<F>(dir: &Path, predicate: F, out: &mut Vec<PathBuf>) -> Result<()>
where
    F: Fn(&Path) -> Result<bool> + Copy,
{
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("reading linter input directory {}", dir.display()))?;
    for entry in entries {
        let entry =
            entry.with_context(|| format!("reading directory entry in {}", dir.display()))?;
        let path = entry.path();
        let metadata = std::fs::metadata(&path)
            .with_context(|| format!("inspecting linter input {}", path.display()))?;
        if metadata.is_dir() {
            discover_files(&path, predicate, out)?;
        } else if metadata.is_file() && predicate(&path)? {
            out.push(path);
        }
    }
    Ok(())
}

/// Recursively discover shell scripts in a directory.
pub(crate) fn discover_shell_scripts(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    discover_files(
        dir,
        |path| {
            if path.extension().is_some_and(|extension| extension == "zsh") {
                return Ok(false);
            }
            if path.extension().is_some_and(|extension| extension == "sh") {
                return Ok(true);
            }
            shebang_matches(path, SHELL_INTERPRETERS)
        },
        out,
    )
}

/// Recursively discover `PowerShell` scripts in a directory.
pub(crate) fn discover_powershell_scripts(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    discover_files(
        dir,
        |path| {
            if path.extension().is_some_and(|extension| {
                extension == "ps1" || extension == "psm1" || extension == "psd1"
            }) {
                return Ok(true);
            }
            shebang_matches(path, POWERSHELL_INTERPRETERS)
        },
        out,
    )
}

/// Collect linter inputs from the repository root.
///
/// Adds each existing path in `files`, then recursively walks each existing
/// directory in `dirs` with `walk`. Missing optional paths are ignored; failures
/// inspecting existing paths are returned rather than treated as empty input.
pub(crate) fn discover_linter_inputs(
    root: &Path,
    files: &[&str],
    dirs: &[&str],
    walk: fn(&Path, &mut Vec<PathBuf>) -> Result<()>,
) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    for name in files {
        let path = root.join(name);
        if path
            .try_exists()
            .with_context(|| format!("inspecting linter input {}", path.display()))?
        {
            found.push(path);
        }
    }
    for dir in dirs {
        let path = root.join(dir);
        if path
            .try_exists()
            .with_context(|| format!("inspecting linter input directory {}", path.display()))?
        {
            walk(&path, &mut found)?;
        }
    }
    Ok(found)
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

fn shebang_matches(path: &Path, interpreters: &[&[u8]]) -> Result<bool> {
    let first_line = read_first_line(path)?;
    Ok(parse_shebang_interpreter(&first_line).is_some_and(|name| {
        let trimmed = name.strip_suffix(b".exe").unwrap_or(name.as_slice());
        interpreters.contains(&trimmed)
    }))
}

fn parse_shebang_interpreter(first_line: &[u8]) -> Option<Vec<u8>> {
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

fn read_first_line(path: &Path) -> Result<Vec<u8>> {
    use std::io::Read as _;

    let mut file = std::fs::File::open(path)
        .with_context(|| format!("opening linter input {}", path.display()))?;
    let mut buffer = [0_u8; 256];
    let count = file
        .read(&mut buffer)
        .with_context(|| format!("reading linter input {}", path.display()))?;
    let end = buffer
        .get(..count)
        .and_then(|slice| slice.iter().position(|&byte| byte == b'\n'))
        .unwrap_or(count);
    Ok(buffer.get(..end).unwrap_or_default().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shebang_probe_reports_open_and_read_failures() {
        let dir = tempfile::tempdir_in(".").unwrap();
        for path in [dir.path().to_path_buf(), dir.path().join("missing")] {
            let error = shebang_matches(&path, SHELL_INTERPRETERS)
                .expect_err("unreadable input must not be classified as a non-script");
            assert!(error.to_string().contains(&path.display().to_string()));
            assert!(error.downcast_ref::<std::io::Error>().is_some());
        }
    }

    #[cfg(unix)]
    #[test]
    fn nested_discovery_reports_metadata_failures() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let nested = dir.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        let broken = nested.join("broken");
        std::os::unix::fs::symlink("missing", &broken).unwrap();

        let error = discover_shell_scripts(dir.path(), &mut Vec::new())
            .expect_err("an unreadable nested entry must fail discovery");
        assert!(error.to_string().contains(&broken.display().to_string()));
        assert!(error.downcast_ref::<std::io::Error>().is_some());
    }
}
