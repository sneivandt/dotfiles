//! Privileged system-file convergence driven by `conf/system-files.toml`.

use std::fs;
use std::io::{ErrorKind, Write as _};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context as _, anyhow, bail};

use crate::domains::system::config::system_files::{MergeStrategy, SystemFile};
use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::infra::exec::{CommandSpec, Executor, OutputLog};

/// Converges one root-owned file from a tracked source fragment.
pub struct SystemFileResource {
    entry: SystemFile,
    temp_dir: PathBuf,
    executor: Arc<dyn Executor>,
    #[cfg(unix)]
    expected_uid: u32,
    #[cfg(unix)]
    expected_gid: u32,
}

enum CurrentFile {
    Missing,
    NonRegular,
    Present(String, fs::Metadata),
}

impl std::fmt::Debug for SystemFileResource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemFileResource")
            .field("target", &self.entry.target)
            .field("source", &self.entry.source_path())
            .field("merge", &self.entry.merge)
            .finish_non_exhaustive()
    }
}

impl SystemFileResource {
    /// Create a resource from one configured system file.
    #[must_use]
    pub fn new(entry: SystemFile, executor: Arc<dyn Executor>) -> Self {
        Self {
            entry,
            temp_dir: std::env::temp_dir(),
            executor,
            #[cfg(unix)]
            expected_uid: 0,
            #[cfg(unix)]
            expected_gid: 0,
        }
    }

    fn fragment(&self) -> ResourceResult<String> {
        fs::read_to_string(self.entry.source_path())
            .with_context(|| format!("reading {}", self.entry.source_path().display()))
            .map_err(Into::into)
    }

    fn current_content(&self) -> ResourceResult<CurrentFile> {
        let metadata = match fs::symlink_metadata(&self.entry.target) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(CurrentFile::Missing),
            Err(error) => {
                return Err(anyhow!(error)
                    .context(format!(
                        "reading metadata for {}",
                        self.entry.target.display()
                    ))
                    .into());
            }
        };
        if !metadata.is_file() {
            return Ok(CurrentFile::NonRegular);
        }
        let content = match fs::read_to_string(&self.entry.target) {
            Ok(content) => content,
            Err(error) if error.kind() == ErrorKind::PermissionDenied => {
                self.executor
                    .execute(
                        CommandSpec::new("sudo")
                            .arg("cat")
                            .arg("--")
                            .arg(&self.entry.target)
                            .output_log(OutputLog::Omit),
                    )
                    .with_context(|| format!("reading {}", self.entry.target.display()))?
                    .stdout
            }
            Err(error) => {
                return Err(anyhow!(error)
                    .context(format!("reading {}", self.entry.target.display()))
                    .into());
            }
        };
        Ok(CurrentFile::Present(content, metadata))
    }

    #[cfg(unix)]
    fn metadata_is_correct(&self, metadata: &fs::Metadata) -> bool {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        metadata.uid() == self.expected_uid
            && metadata.gid() == self.expected_gid
            && metadata.permissions().mode() & 0o7777 == 0o644
    }

    fn desired_content(&self, current: &str, fragment: &str) -> anyhow::Result<String> {
        match self.entry.merge {
            MergeStrategy::Toml => merge_toml(current, fragment),
            MergeStrategy::Ini => merge_ini(current, fragment),
            MergeStrategy::Pam => merge_pam(current, fragment),
        }
    }

    fn content_is_correct(&self, current: &str, fragment: &str) -> anyhow::Result<bool> {
        match self.entry.merge {
            MergeStrategy::Toml => contains_toml(current, fragment),
            MergeStrategy::Ini => contains_ini(current, fragment),
            MergeStrategy::Pam => Ok(merge_pam(current, fragment)? == current),
        }
    }
}

impl Resource for SystemFileResource {
    fn description(&self) -> String {
        self.entry.target.display().to_string()
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        if matches!(self.current_state()?, ResourceState::Correct) {
            return Ok(ResourceChange::AlreadyCorrect);
        }
        let fragment = self.fragment()?;
        let current = match self.current_content()? {
            CurrentFile::Missing => String::new(),
            CurrentFile::Present(text, _) => text,
            CurrentFile::NonRegular => {
                return Err(
                    anyhow!("{} is not a regular file", self.entry.target.display()).into(),
                );
            }
        };
        let desired = self.desired_content(&current, &fragment)?;
        let (temporary, mut file) = crate::infra::fs::TempGuard::create_unique_file_with_mode(
            &self.temp_dir,
            ".dotfiles-system-file",
            "tmp",
            0o600,
        )?;
        file.write_all(desired.as_bytes())?;
        file.sync_all()?;
        drop(file);
        self.executor
            .execute(
                CommandSpec::new("sudo")
                    .arg("install")
                    .arg("-D")
                    .arg("--owner=root")
                    .arg("--group=root")
                    .arg("--mode=0644")
                    .arg("--")
                    .arg(temporary.path())
                    .arg(&self.entry.target),
            )
            .with_context(|| format!("installing {}", self.entry.target.display()))?;
        Ok(ResourceChange::Applied)
    }
}

impl IntrinsicState for SystemFileResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        let fragment = self.fragment()?;
        let (current, metadata) = match self.current_content()? {
            CurrentFile::Missing => {
                return Ok(if self.entry.merge == MergeStrategy::Pam {
                    ResourceState::Invalid {
                        reason: format!(
                            "required PAM service file {} is missing",
                            self.entry.target.display()
                        ),
                    }
                } else {
                    ResourceState::Missing
                });
            }
            CurrentFile::NonRegular => {
                return Ok(ResourceState::Invalid {
                    reason: format!("{} is not a regular file", self.entry.target.display()),
                });
            }
            CurrentFile::Present(content, metadata) => (content, metadata),
        };
        let content_matches = match self.content_is_correct(&current, &fragment) {
            Ok(matches) => matches,
            Err(error) => {
                return Ok(ResourceState::Invalid {
                    reason: error.to_string(),
                });
            }
        };
        #[cfg(unix)]
        let metadata_matches = self.metadata_is_correct(&metadata);
        #[cfg(not(unix))]
        let metadata_matches = {
            let _ = metadata;
            true
        };
        Ok(if content_matches && metadata_matches {
            ResourceState::Correct
        } else {
            let mut differences = Vec::new();
            if !content_matches {
                differences.push("managed content differs");
            }
            if !metadata_matches {
                differences.push("ownership or mode differs");
            }
            ResourceState::Incorrect {
                current: differences.join("; "),
            }
        })
    }
}

fn parse_toml(content: &str) -> anyhow::Result<toml::Table> {
    if content.trim().is_empty() {
        Ok(toml::Table::new())
    } else {
        toml::from_str(content).context("parsing TOML system file")
    }
}

fn merge_table(current: &mut toml::Table, desired: &toml::Table) {
    for (key, desired_value) in desired {
        match (current.get_mut(key), desired_value) {
            (Some(toml::Value::Table(current_table)), toml::Value::Table(desired_table)) => {
                merge_table(current_table, desired_table);
            }
            _ => {
                current.insert(key.clone(), desired_value.clone());
            }
        }
    }
}

fn contains_table(current: &toml::Table, desired: &toml::Table) -> bool {
    desired.iter().all(
        |(key, desired_value)| match (current.get(key), desired_value) {
            (Some(toml::Value::Table(current_table)), toml::Value::Table(desired_table)) => {
                contains_table(current_table, desired_table)
            }
            (Some(current_value), desired_value) => current_value == desired_value,
            (None, _) => false,
        },
    )
}

fn contains_toml(current: &str, fragment: &str) -> anyhow::Result<bool> {
    Ok(contains_table(
        &parse_toml(current)?,
        &parse_toml(fragment)?,
    ))
}

fn merge_toml(current: &str, fragment: &str) -> anyhow::Result<String> {
    let mut current = parse_toml(current)?;
    merge_table(&mut current, &parse_toml(fragment)?);
    let mut rendered = toml::to_string_pretty(&current)?;
    if !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    Ok(rendered)
}

fn ini_settings(
    content: &str,
    strict: bool,
) -> anyhow::Result<Vec<(String, String, Option<String>)>> {
    let mut section = None::<String>;
    let mut settings = Vec::new();
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = Some(line.to_string());
            continue;
        }
        let (key, value) = if let Some((key, value)) = line.split_once('=') {
            (key.trim(), Some(value.trim().to_string()))
        } else if line.split_whitespace().count() == 1 {
            (line, None)
        } else {
            if strict {
                bail!("INI fragment contains an invalid bare setting: {line}");
            }
            continue;
        };
        let Some(section) = &section else {
            if strict {
                bail!("INI fragment contains a setting outside a section: {line}");
            }
            continue;
        };
        settings.push((section.clone(), key.to_string(), value));
    }
    Ok(settings)
}

fn contains_ini(current: &str, fragment: &str) -> anyhow::Result<bool> {
    let desired = ini_settings(fragment, true)?;
    let observed = ini_settings(current, false)?;
    Ok(desired.iter().all(|wanted| {
        observed
            .iter()
            .filter(|entry| entry.0 == wanted.0 && entry.1 == wanted.1)
            .count()
            == 1
            && observed.contains(wanted)
    }))
}

fn merge_ini(current: &str, fragment: &str) -> anyhow::Result<String> {
    let mut lines = current.lines().map(str::to_string).collect::<Vec<_>>();
    for (section, key, value) in ini_settings(fragment, true)? {
        ensure_ini_key(&mut lines, &section, &key, value.as_deref());
    }
    let mut rendered = lines.join("\n");
    if !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    Ok(rendered)
}

fn ini_line_key(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.is_empty()
        || line.starts_with(['#', ';'])
        || (line.starts_with('[') && line.ends_with(']'))
    {
        return None;
    }
    if let Some((key, _)) = line.split_once('=') {
        return Some(key.trim());
    }
    (line.split_whitespace().count() == 1).then_some(line)
}

fn ensure_ini_key(lines: &mut Vec<String>, section: &str, key: &str, value: Option<&str>) {
    let indices = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim() == section)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if let Some(&first) = indices.first() {
        let first_end = lines
            .iter()
            .enumerate()
            .skip(first.saturating_add(1))
            .find(|(_, line)| {
                let line = line.trim();
                line.starts_with('[') && line.ends_with(']')
            })
            .map_or(lines.len(), |(index, _)| index);
        let insertion_index = lines
            .iter()
            .enumerate()
            .take(first_end)
            .skip(first.saturating_add(1))
            .find(|(_, line)| ini_line_key(line) == Some(key))
            .map_or_else(
                || {
                    lines
                        .iter()
                        .enumerate()
                        .take(first_end)
                        .skip(first.saturating_add(1))
                        .rfind(|(_, line)| ini_line_key(line).is_some())
                        .map_or_else(
                            || first.saturating_add(1),
                            |(index, _)| index.saturating_add(1),
                        )
                },
                |(index, _)| index,
            );
        for &start in indices.iter().rev() {
            let content_start = start.saturating_add(1);
            let end = lines
                .iter()
                .enumerate()
                .skip(content_start)
                .find(|(_, line)| {
                    let line = line.trim();
                    line.starts_with('[') && line.ends_with(']')
                })
                .map_or(lines.len(), |(index, _)| index);
            for index in (content_start..end).rev() {
                if lines.get(index).and_then(|line| ini_line_key(line)) == Some(key) {
                    lines.remove(index);
                }
            }
        }
        lines.insert(
            insertion_index,
            value.map_or_else(|| key.to_string(), |value| format!("{key}={value}")),
        );
    } else {
        if lines.last().is_some_and(|line| !line.is_empty()) {
            lines.push(String::new());
        }
        lines.push(section.to_string());
        lines.push(value.map_or_else(|| key.to_string(), |value| format!("{key}={value}")));
    }
}

fn pam_rule_identity(line: &str) -> anyhow::Result<(String, String)> {
    let tokens = line.split_whitespace().collect::<Vec<_>>();
    let Some(facility) = tokens.first() else {
        bail!("PAM fragment contains an empty rule");
    };
    #[allow(
        clippy::case_sensitive_file_extension_comparisons,
        reason = "PAM module names and Linux file extensions are case-sensitive"
    )]
    let Some(module) = tokens.iter().find(|token| token.ends_with(".so")) else {
        bail!("PAM fragment rule has no module: {line}");
    };
    Ok(((*facility).to_string(), (*module).to_string()))
}

fn merge_pam(current: &str, fragment: &str) -> anyhow::Result<String> {
    let mut lines = current.lines().map(str::to_string).collect::<Vec<_>>();
    for rule in fragment
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let (facility, module) = pam_rule_identity(rule)?;
        lines.retain(|line| {
            let code = line.split_once('#').map_or(line.as_str(), |(code, _)| code);
            let tokens = code.split_whitespace().collect::<Vec<_>>();
            !(tokens.first().copied() == Some(facility.as_str())
                && tokens.contains(&module.as_str()))
        });
        let Some(index) = lines
            .iter()
            .rposition(|line| line.split_whitespace().next() == Some(facility.as_str()))
        else {
            bail!("PAM service has no {facility} stack; refusing to synthesize one");
        };
        lines.insert(index.saturating_add(1), rule.to_string());
    }
    let mut rendered = lines.join("\n");
    rendered.push('\n');
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::infra::exec::ExecResult;
    use crate::infra::exec::MockExecutor;
    use std::path::Path;

    fn entry(root: &Path, target: &Path, source: &str, merge: MergeStrategy) -> SystemFile {
        SystemFile {
            target: target.to_path_buf(),
            source: PathBuf::from(source),
            merge,
            origin: Some(root.to_path_buf()),
        }
    }

    #[test]
    fn description_is_the_target_path() {
        let root = tempfile::tempdir().unwrap();
        let target = Path::new("/etc/pacman.conf");
        let resource = SystemFileResource::new(
            entry(root.path(), target, "pacman.conf", MergeStrategy::Ini),
            Arc::new(MockExecutor::new()),
        );

        assert_eq!(resource.description(), target.display().to_string());
    }

    #[test]
    fn merge_strategies_preserve_unmanaged_content() {
        let toml = merge_toml(
            "other = true\n[browser]\nkeep = 1\n",
            "[browser]\nmanaged = 2\n",
        )
        .unwrap();
        assert!(toml.contains("other = true"));
        assert!(toml.contains("keep = 1"));
        assert!(contains_toml(&toml, "[browser]\nmanaged = 2\n").unwrap());

        let ini = merge_ini(
            "[boot]\ncommand=x\nsystemd=false\n",
            "[boot]\nsystemd=true\n",
        )
        .unwrap();
        assert!(ini.contains("command=x"));
        assert!(contains_ini(&ini, "[boot]\nsystemd=true\n").unwrap());

        let pam = merge_pam(
            "auth include system-auth\nauth optional old.so\n",
            "auth optional keyring.so\n",
        )
        .unwrap();
        assert!(pam.contains("auth optional old.so"));
        assert!(pam.ends_with("auth optional keyring.so\n"));
    }

    #[test]
    fn missing_pam_target_is_invalid() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("system")).unwrap();
        fs::write(
            root.path().join("system/login"),
            "auth optional keyring.so\n",
        )
        .unwrap();
        let target = root.path().join("missing-login");
        let resource = SystemFileResource::new(
            entry(root.path(), &target, "login", MergeStrategy::Pam),
            Arc::new(MockExecutor::new()),
        );

        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Invalid { reason } if reason.contains("is missing")
        ));
    }

    #[test]
    fn ini_merge_supports_bare_settings() {
        let current = "[options]\n#Color\nColor\nColor\nCheckSpace\nParallelDownloads = 2\n";
        let fragment = "[options]\nColor\nILoveCandy\nParallelDownloads = 5\n";

        let merged = merge_ini(current, fragment).unwrap();

        assert!(merged.contains("#Color"));
        assert!(merged.contains("CheckSpace"));
        assert_eq!(merged.lines().filter(|line| *line == "Color").count(), 1);
        assert!(merged.contains("ILoveCandy"));
        assert!(!merged.contains("ParallelDownloads = 2"));
        assert!(merged.contains("ParallelDownloads=5"));
        assert!(contains_ini(&merged, fragment).unwrap());
        assert!(!contains_ini("[options]\ncolor\n", "[options]\nColor\n").unwrap());
        assert!(merge_ini("[options]\nColor\n", "[options]\nColor enabled\n").is_err());
    }

    #[test]
    fn malformed_target_is_invalid_without_being_replaced() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("system")).unwrap();
        fs::write(root.path().join("system/fragment.toml"), "managed = true\n").unwrap();
        let target = root.path().join("target.toml");
        fs::write(&target, "not = [valid\n").unwrap();
        let resource = SystemFileResource::new(
            entry(root.path(), &target, "fragment.toml", MergeStrategy::Toml),
            Arc::new(MockExecutor::new()),
        );

        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Invalid { .. }
        ));
        assert_eq!(fs::read_to_string(target).unwrap(), "not = [valid\n");
    }

    #[cfg(unix)]
    #[test]
    fn apply_stages_merged_content_with_root_owned_install_arguments_and_converges() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("system")).unwrap();
        fs::write(
            root.path().join("system/fragment.toml"),
            "[managed]\nenabled = true\n",
        )
        .unwrap();
        let target = root.path().join("target.toml");
        fs::write(&target, "unmanaged = 'keep'\n").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
        let metadata = fs::metadata(&target).unwrap();
        let expected_target = target.clone();
        let copy_target = target.clone();
        let mut executor = MockExecutor::new();
        executor
            .expect_execute()
            .once()
            .withf(move |spec| {
                spec.program() == "sudo"
                    && spec.arguments().starts_with(&[
                        "install".into(),
                        "-D".into(),
                        "--owner=root".into(),
                        "--group=root".into(),
                        "--mode=0644".into(),
                        "--".into(),
                    ])
                    && spec
                        .arguments()
                        .last()
                        .is_some_and(|argument| argument == expected_target.as_os_str())
            })
            .returning(move |spec| {
                fs::copy(&spec.arguments()[6], &copy_target).unwrap();
                fs::set_permissions(&copy_target, fs::Permissions::from_mode(0o644)).unwrap();
                Ok(ExecResult::success(""))
            });
        let mut resource = SystemFileResource::new(
            entry(root.path(), &target, "fragment.toml", MergeStrategy::Toml),
            Arc::new(executor),
        );
        resource.temp_dir = root.path().to_path_buf();
        resource.expected_uid = metadata.uid();
        resource.expected_gid = metadata.gid();

        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
        let content = fs::read_to_string(target).unwrap();
        assert!(content.contains("unmanaged = \"keep\""));
        assert!(content.contains("enabled = true"));
    }
}
