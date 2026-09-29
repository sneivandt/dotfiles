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
        if let Some(header) = ini_section(line) {
            section = Some(header.to_string());
            continue;
        }
        let Some((key, value)) = ini_setting(line) else {
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
        settings.push((section.clone(), key.to_string(), value.map(str::to_string)));
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

fn ini_section(line: &str) -> Option<&str> {
    let line = line.trim();
    (line.starts_with('[') && line.ends_with(']')).then_some(line)
}

fn ini_setting(line: &str) -> Option<(&str, Option<&str>)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with(['#', ';']) || ini_section(line).is_some() {
        return None;
    }
    if let Some((key, value)) = line.split_once('=') {
        return Some((key.trim(), Some(value.trim())));
    }
    (line.split_whitespace().count() == 1).then_some((line, None))
}

fn ini_line_key(line: &str) -> Option<&str> {
    ini_setting(line).map(|(key, _)| key)
}

fn ensure_ini_key(lines: &mut Vec<String>, section: &str, key: &str, value: Option<&str>) {
    let setting = value.map_or_else(|| key.to_string(), |value| format!("{key}={value}"));
    let Some(first) = lines.iter().position(|line| line.trim() == section) else {
        if lines.last().is_some_and(|line| !line.is_empty()) {
            lines.push(String::new());
        }
        lines.push(section.to_string());
        lines.push(setting);
        return;
    };

    let mut insertion_index = first.saturating_add(1);
    for (index, line) in lines
        .iter()
        .enumerate()
        .skip(insertion_index)
        .take_while(|(_, line)| ini_section(line).is_none())
    {
        if let Some(current_key) = ini_line_key(line) {
            if current_key == key {
                insertion_index = index;
                break;
            }
            insertion_index = index.saturating_add(1);
        }
    }

    // No matching key precedes the insertion point, so removing duplicates
    // across all occurrences of this section leaves that index unchanged.
    let mut in_section = false;
    lines.retain(|line| {
        if let Some(header) = ini_section(line) {
            in_section = header == section;
        }
        !in_section || ini_line_key(line) != Some(key)
    });
    lines.insert(insertion_index, setting);
}

fn pam_rule_identity(line: &str) -> anyhow::Result<(&str, &str)> {
    let mut tokens = line.split_whitespace();
    let Some(facility) = tokens.next() else {
        bail!("PAM fragment contains an empty rule");
    };
    let Some(control) = tokens.next() else {
        bail!("PAM fragment rule has no control field: {line}");
    };
    if matches!(control, "include" | "substack") {
        bail!("PAM fragment rule includes a stack rather than a module: {line}");
    }
    if control.starts_with('[')
        && !control.ends_with(']')
        && !tokens.by_ref().any(|token| token.ends_with(']'))
    {
        bail!("PAM fragment rule has an unterminated control field: {line}");
    }
    #[allow(
        clippy::case_sensitive_file_extension_comparisons,
        reason = "PAM module names and Linux file extensions are case-sensitive"
    )]
    let Some(module) = tokens.next().filter(|token| token.ends_with(".so")) else {
        bail!("PAM fragment rule has no module: {line}");
    };
    Ok((facility, module))
}

fn merge_pam(current: &str, fragment: &str) -> anyhow::Result<String> {
    let mut lines = current.lines().map(str::to_string).collect::<Vec<_>>();
    for rule in fragment
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let (facility, module) = pam_rule_identity(rule)?;
        let Some(index) = lines
            .iter()
            .rposition(|line| line.split_whitespace().next() == Some(facility))
        else {
            bail!("PAM service has no {facility} stack; refusing to synthesize one");
        };
        let mut trailing = lines.split_off(index.saturating_add(1));
        lines.retain(|line| {
            let code = line.split_once('#').map_or(line.as_str(), |(code, _)| code);
            !pam_rule_identity(code).is_ok_and(|identity| identity == (facility, module))
        });
        lines.push(rule.to_string());
        lines.append(&mut trailing);
    }
    let mut rendered = lines.join("\n");
    rendered.push('\n');
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::exec::{ExecError, ExecResult, MockExecutor};
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
    fn pam_merge_preserves_a_stack_containing_only_the_managed_module() {
        let fragment = "auth optional keyring.so\n";
        for current in [
            fragment,
            "# retained\nauth required keyring.so old_option\n",
            "auth optional keyring.so\nauth optional keyring.so\n",
        ] {
            let merged = merge_pam(current, fragment).unwrap();
            assert!(merged.ends_with(fragment), "{merged}");
            assert_eq!(merged.matches("keyring.so").count(), 1, "{merged}");
            assert_eq!(merge_pam(&merged, fragment).unwrap(), merged);
            if current.starts_with('#') {
                assert!(merged.starts_with("# retained\n"));
            }
        }
        assert!(
            merge_pam("session optional other.so\n", fragment).is_err(),
            "a genuinely absent facility must still be rejected"
        );
    }

    #[test]
    fn pam_merge_does_not_claim_module_arguments_or_stack_names() {
        let unmanaged = "auth include system-auth\n\
            auth required pam_exec.so keyring.so\n\
            auth [success=1 default=ignore] pam_debug.so keyring.so\n\
            auth include keyring.so\n";
        let current = format!(
            "{unmanaged}auth optional keyring.so old_option\nsession include system-login\n"
        );
        let fragment = "auth optional keyring.so\n";
        let expected = format!("{unmanaged}{fragment}session include system-login\n");

        let merged = merge_pam(&current, fragment).unwrap();

        assert_eq!(merged, expected, "unmanaged PAM rules must be preserved");
        assert_eq!(merge_pam(&merged, fragment).unwrap(), merged);
    }

    #[test]
    fn pam_rule_identity_uses_the_module_field_after_the_control_field() {
        for rule in [
            "auth optional keyring.so argument",
            "auth [success=1 default=ignore] keyring.so argument",
            "auth [success=done] keyring.so argument",
        ] {
            assert_eq!(pam_rule_identity(rule).unwrap(), ("auth", "keyring.so"));
        }
        for rule in [
            "auth optional script keyring.so",
            "auth include keyring.so",
            "auth substack keyring.so",
            "auth [success=1 keyring.so",
        ] {
            assert!(
                pam_rule_identity(rule).is_err(),
                "a stack name or argument is not a module field: {rule}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn pam_resource_accepts_an_already_converged_single_module_stack() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        let root = tempfile::tempdir_in(".").unwrap();
        fs::create_dir(root.path().join("system")).unwrap();
        let content = "password optional keyring.so\n";
        fs::write(root.path().join("system/passwd"), content).unwrap();
        let target = root.path().join("passwd");
        fs::write(&target, content).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
        let metadata = fs::metadata(&target).unwrap();
        let mut resource = SystemFileResource::new(
            entry(root.path(), &target, "passwd", MergeStrategy::Pam),
            Arc::new(MockExecutor::new()),
        );
        resource.expected_uid = metadata.uid();
        resource.expected_gid = metadata.gid();

        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
        assert_eq!(resource.apply().unwrap(), ResourceChange::AlreadyCorrect);
        assert_eq!(resource.apply().unwrap(), ResourceChange::AlreadyCorrect);
        assert_eq!(fs::read_to_string(target).unwrap(), content);
    }

    #[test]
    fn ini_setting_recognition_preserves_lexical_edge_cases() {
        for (label, line, expected) in [
            (
                "only the first equals sign separates the value",
                " \tkey \t= first = second \t",
                Some(("key", Some("first = second"))),
            ),
            ("empty key", " = value ", Some(("", Some("value")))),
            ("empty value", "key \t= \t", Some(("key", Some("")))),
            ("empty key and value", "\t= \t", Some(("", Some("")))),
            ("bare flag with tabs", "\tFlag\t", Some(("Flag", None))),
            (
                "whitespace inside an assigned key is retained",
                " first\tsecond = value ",
                Some(("first\tsecond", Some("value"))),
            ),
            (
                "inline comment characters are literal",
                "key=value # hash ; semicolon",
                Some(("key", Some("value # hash ; semicolon"))),
            ),
            (
                "malformed section-like line remains a bare setting",
                "\t[not-a-section\t",
                Some(("[not-a-section", None)),
            ),
            ("empty line", "", None),
            ("whitespace", " \t ", None),
            ("hash comment", "\t# key=value", None),
            ("semicolon comment", "\t; Flag", None),
            ("section header", " \t[other]\t ", None),
            ("equals sign inside a section header", "[other=value]", None),
        ] {
            assert_eq!(ini_line_key(line), expected.map(|(key, _)| key), "{label}");
            let fragment = format!("[options]\n{line}\n");
            let expected_settings = expected
                .map(|(key, value)| {
                    (
                        "[options]".to_string(),
                        key.to_string(),
                        value.map(str::to_string),
                    )
                })
                .into_iter()
                .collect::<Vec<_>>();
            for strict in [false, true] {
                assert_eq!(
                    ini_settings(&fragment, strict).unwrap(),
                    expected_settings,
                    "{label}, strict={strict}"
                );
            }

            if let Some((key, value)) = expected {
                let current = format!("[options]\n{line}\n{line}\n");
                let setting =
                    value.map_or_else(|| key.to_string(), |value| format!("{key}={value}"));
                let merged = merge_ini(&current, &fragment).unwrap();
                assert_eq!(merged, format!("[options]\n{setting}\n"), "{label}");
                assert!(contains_ini(&merged, &fragment).unwrap(), "{label}");
                assert_eq!(merge_ini(&merged, &fragment).unwrap(), merged, "{label}");
            }
        }
    }

    #[test]
    fn ini_settings_preserve_strict_diagnostics_and_permissive_skips() {
        for (label, content, expected_error) in [
            (
                "invalid bare setting inside a section",
                "[options]\n\tbad setting\t\n",
                "INI fragment contains an invalid bare setting: bad setting",
            ),
            (
                "invalid bare setting precedes the missing-section diagnostic",
                "\tbad setting\t\n",
                "INI fragment contains an invalid bare setting: bad setting",
            ),
            (
                "assignment outside a section",
                "\tkey = value\t\n",
                "INI fragment contains a setting outside a section: key = value",
            ),
            (
                "bare flag outside a section",
                "\tFlag\t\n",
                "INI fragment contains a setting outside a section: Flag",
            ),
            (
                "malformed section-like flag outside a section",
                "\t[not-a-section\t\n",
                "INI fragment contains a setting outside a section: [not-a-section",
            ),
            (
                "first invalid line wins",
                "bad setting\nkey=value\n",
                "INI fragment contains an invalid bare setting: bad setting",
            ),
            (
                "first out-of-section setting wins",
                "key=value\nbad setting\n",
                "INI fragment contains a setting outside a section: key=value",
            ),
        ] {
            assert_eq!(
                ini_settings(content, true).unwrap_err().to_string(),
                expected_error,
                "{label}"
            );
            assert!(ini_settings(content, false).unwrap().is_empty(), "{label}");
            let continued = format!("{content}[options]\nkeep=yes\n");
            assert_eq!(
                ini_settings(&continued, false).unwrap(),
                [(
                    "[options]".to_string(),
                    "keep".to_string(),
                    Some("yes".to_string()),
                )],
                "{label}"
            );
        }
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
    fn ini_merge_preserves_setting_positions_and_removes_only_managed_duplicates() {
        for (label, current, expected) in [
            ("empty file", "", "[options]\nmanaged=new\n"),
            (
                "missing section",
                "[other]\nmanaged=keep\n",
                "[other]\nmanaged=keep\n\n[options]\nmanaged=new\n",
            ),
            (
                "empty section",
                "[options]\n# retained comment\n\n[other]\nmanaged=keep\n",
                "[options]\nmanaged=new\n# retained comment\n\n[other]\nmanaged=keep\n",
            ),
            (
                "missing key follows last setting, not trailing comments",
                "[options]\n# heading\nkeep=yes\n# trailing\n\n",
                "[options]\n# heading\nkeep=yes\nmanaged=new\n# trailing\n",
            ),
            (
                "existing key stays before following settings",
                "[options]\n# heading\nmanaged=old\nkeep=yes\nmanaged=duplicate\n",
                "[options]\n# heading\nmanaged=new\nkeep=yes\n",
            ),
            (
                "repeated sections retain unmanaged keys",
                "[options]\nmanaged=old\n[other]\nmanaged=keep\n [options] \nmanaged\nkeep=yes\n",
                "[options]\nmanaged=new\n[other]\nmanaged=keep\n [options] \nkeep=yes\n",
            ),
            (
                "key moves from later occurrence into first section",
                "[options]\nkeep=yes\n# trailing\n[options]\nmanaged=old\n",
                "[options]\nkeep=yes\nmanaged=new\n# trailing\n[options]\n",
            ),
            (
                "bare settings and malformed section-like lines keep their positions",
                "[options]\n# managed=comment\nBareFlag\n[not-a-section\n; trailing\n",
                "[options]\n# managed=comment\nBareFlag\n[not-a-section\nmanaged=new\n; trailing\n",
            ),
        ] {
            let fragment = "[options]\nmanaged=new\n";
            let merged = merge_ini(current, fragment).unwrap();
            assert_eq!(merged, expected, "{label}");
            assert!(contains_ini(&merged, fragment).unwrap(), "{label}");
            assert_eq!(merge_ini(&merged, fragment).unwrap(), merged, "{label}");
        }
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
        assert!(
            resource.apply().is_err(),
            "malformed content must not be installed"
        );
        assert_eq!(fs::read_to_string(target).unwrap(), "not = [valid\n");
    }

    #[test]
    fn failed_install_cleans_staging_preserves_target_and_can_be_retried() {
        #[cfg(unix)]
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        let root = tempfile::tempdir_in(".").unwrap();
        fs::create_dir(root.path().join("system")).unwrap();
        let staging = root.path().join("staging");
        fs::create_dir(&staging).unwrap();
        fs::write(
            root.path().join("system/fragment.toml"),
            "[managed]\nenabled = true\n",
        )
        .unwrap();
        let target = root.path().join("target.toml");
        fs::write(&target, "unmanaged = 'keep'\n").unwrap();
        #[cfg(unix)]
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
        #[cfg(unix)]
        let metadata = fs::metadata(&target).unwrap();
        let mut executor = MockExecutor::new();
        let mut sequence = mockall::Sequence::new();
        for fail in [true, false] {
            let expected_target = target.clone();
            let expected_staging = staging.clone();
            executor
                .expect_execute()
                .once()
                .in_sequence(&mut sequence)
                .returning(move |spec| {
                    assert_eq!(spec.program(), "sudo");
                    assert_eq!(spec.arguments().len(), 8);
                    assert_eq!(
                        &spec.arguments()[..6],
                        [
                            "install",
                            "-D",
                            "--owner=root",
                            "--group=root",
                            "--mode=0644",
                            "--",
                        ]
                    );
                    assert_eq!(spec.arguments()[7], expected_target.as_os_str());
                    assert_eq!(spec.working_dir(), None);
                    assert!(spec.is_checked());
                    let staged = Path::new(&spec.arguments()[6]);
                    assert_eq!(staged.parent(), Some(expected_staging.as_path()));
                    let content = fs::read_to_string(staged).unwrap();
                    let table: toml::Table = toml::from_str(&content).unwrap();
                    assert_eq!(table["unmanaged"].as_str(), Some("keep"));
                    assert_eq!(table["managed"]["enabled"].as_bool(), Some(true));
                    #[cfg(unix)]
                    assert_eq!(
                        fs::metadata(staged).unwrap().permissions().mode() & 0o777,
                        0o600
                    );
                    if fail {
                        Err(ExecError::non_zero(
                            "sudo install",
                            ExecResult::failure("", "fixture install denied", Some(1)),
                        ))
                    } else {
                        fs::copy(staged, &expected_target).unwrap();
                        #[cfg(unix)]
                        fs::set_permissions(&expected_target, fs::Permissions::from_mode(0o644))
                            .unwrap();
                        Ok(ExecResult::success(""))
                    }
                });
        }
        let mut resource = SystemFileResource::new(
            entry(root.path(), &target, "fragment.toml", MergeStrategy::Toml),
            Arc::new(executor),
        );
        resource.temp_dir = staging.clone();
        #[cfg(unix)]
        {
            resource.expected_uid = metadata.uid();
            resource.expected_gid = metadata.gid();
        }

        let error = resource.apply().unwrap_err();
        assert!(
            format!("{error:#}").contains("fixture install denied"),
            "{error:#}"
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "unmanaged = 'keep'\n");
        assert_eq!(fs::read_dir(&staging).unwrap().count(), 0);
        assert!(matches!(
            resource.current_state().unwrap(),
            ResourceState::Incorrect { .. }
        ));
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
        assert_eq!(resource.apply().unwrap(), ResourceChange::AlreadyCorrect);
        assert_eq!(fs::read_dir(&staging).unwrap().count(), 0);
        let content = fs::read_to_string(target).unwrap();
        assert!(content.contains("unmanaged = \"keep\""));
        assert!(content.contains("enabled = true"));
    }
}
