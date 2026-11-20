//! Privileged system-file convergence driven by `conf/system-files.toml`.

use std::fs;
use std::io::ErrorKind;
#[cfg(unix)]
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context as _, anyhow, bail};

use crate::domains::system::config::system_files::{MergeStrategy, SystemFile};
use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::infra::exec::{CommandSpec, Executor, OutputLog};

// The privileged copy must finish on the destination filesystem before rename
// publishes it. A private directory prevents another user from replacing the
// staged file; the exit trap also removes a partial copy on command failure.
const ATOMIC_INSTALL: &str = r#"set -eu
helper=$1
fragment=$3
strategy=$5
fragment_hash=$6
case "$helper" in /*) ;; *) helper="$PWD/$helper" ;; esac
case "$fragment" in /*) ;; *) fragment="$PWD/$fragment" ;; esac
owner=$(id -u)
check_directory() {
    if [ "$(find . -maxdepth 0 -uid "$owner" ! -perm /022 -print)" != . ]; then
        printf 'System file directory must be owned by the installer and writable only by its owner: %s\n' "$PWD" >&2
        exit 1
    fi
}
cd -P -- "$4"
check_directory
boundary=$PWD
parent=${2%/*}
case "$parent" in
    "$boundary") remaining= ;;
    "$boundary"/*) remaining=${parent#"$boundary"/} ;;
    *) printf 'System file target is outside %s: %s\n' "$boundary" "$2" >&2; exit 1 ;;
esac
# Enter each directory relative to the verified parent. A link introduced
# after discovery must never redirect mkdir, staging, or publication.
while [ -n "$remaining" ]; do
    component=${remaining%%/*}
    case "$component" in ''|.|..) exit 1 ;; esac
    next="$PWD/$component"
    if [ -L "./$component" ]; then
        printf 'System file parent became a symlink: %s\n' "$next" >&2
        exit 1
    fi
    if [ ! -d "./$component" ]; then
        mkdir -m 0755 -- "./$component"
    fi
    cd -P -- "./$component"
    if [ "$PWD" != "$next" ]; then
        printf 'System file parent changed while entering %s\n' "$next" >&2
        exit 1
    fi
    check_directory
    case "$remaining" in */*) remaining=${remaining#*/} ;; *) remaining= ;; esac
done
test "$(pwd -P)" = "$parent"
target="./${2##*/}"
stage_dir=$(mktemp -d -- ./.dotfiles-system-file.XXXXXXXXXX)
trap 'rm -rf -- "$stage_dir"' EXIT
trap 'exit 1' HUP INT TERM
result=$("$helper" system-file-helper --target "$PWD/${target#./}" \
    --fragment "$fragment" "--fragment-hash=$fragment_hash" \
    --merge "$strategy" --stage "$PWD/${stage_dir#./}")
case "$result" in
    already-correct) printf 'already-correct\n'; exit 0 ;;
    prepared) ;;
    *) printf 'Invalid system file helper response\n' >&2; exit 1 ;;
esac
set -- "$stage_dir/input" "$target" "$stage_dir/original"
replacement="$stage_dir/replacement"
install -D --owner=root --group=root --mode=0644 -- "$1" "$replacement"
if [ -L "$2" ] || { [ -e "$2" ] && [ ! -f "$2" ]; }; then
    printf 'Refusing to replace a non-regular system file: %s\n' "$2" >&2
    exit 1
fi
if [ -f "$3" ]; then
    # ctime covers owner, mode and xattrs; include inode identity for replacements.
    signature='%d:%i:%u:%g:%f:%z'
    before=$(stat -c "$signature" -- "$2")
    cp --attributes-only --preserve=mode,ownership,xattr -- "$2" "$replacement"
    expected=$(sha256sum < "$3")
    if [ -L "$2" ] || [ ! -f "$2" ] ||
        ! actual=$(sha256sum < "$2") || [ "$actual" != "$expected" ] ||
        ! after=$(stat -c "$signature" -- "$2") || [ "$after" != "$before" ]; then
        printf 'System file changed or could not be verified while preparing update: %s; retry the update\n' "$2" >&2
        exit 1
    fi
    test "$(pwd -P)" = "$parent"
    mv -fT -- "$replacement" "$2"
else
    # Publishing a new file must also fail if the target appears after discovery.
    test "$(pwd -P)" = "$parent"
    ln -T -- "$replacement" "$2"
fi
printf 'applied\n'
"#;

/// Converges one system file from a tracked source fragment.
pub struct SystemFileResource {
    entry: SystemFile,
    target_root: PathBuf,
    executor: Arc<dyn Executor>,
}

enum CurrentFile {
    Missing,
    NonRegular,
    Present(String),
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
            target_root: PathBuf::from("/etc"),
            executor,
        }
    }

    fn fragment(&self) -> ResourceResult<String> {
        fs::read_to_string(self.entry.source_path())
            .with_context(|| format!("reading {}", self.entry.source_path().display()))
            .map_err(Into::into)
    }

    pub(in crate::domains::system) fn installation_state(&self) -> ResourceResult<ResourceState> {
        match self.current_state()? {
            // A later declaration may reference this dedicated file. Never publish
            // such a reference when its prerequisite belongs to the administrator.
            ResourceState::Invalid { reason } if self.entry.merge == MergeStrategy::CreateOnly => {
                Err(crate::engine::resource::ResourceError::conflicting_state(
                    self.description(),
                    "a missing or identical dedicated file",
                    reason,
                ))
            }
            // Read protected destinations only inside the scoped installer.
            ResourceState::Unknown { reason } => Ok(ResourceState::Incorrect { current: reason }),
            state @ (ResourceState::Correct
            | ResourceState::Missing
            | ResourceState::Incorrect { .. }
            | ResourceState::Invalid { .. }) => Ok(state),
        }
    }

    fn checked_target(&self) -> anyhow::Result<PathBuf> {
        let root = dunce::canonicalize(&self.target_root)?;
        let target = crate::infra::fs::resolve_target_entry(&self.entry.target)?;
        anyhow::ensure!(
            target != root && target.starts_with(&root),
            "system file target {} resolves outside {} to {}",
            self.entry.target.display(),
            self.target_root.display(),
            target.display()
        );
        Ok(target)
    }

    fn validate_installation_target(&self) -> anyhow::Result<()> {
        let target = &self.entry.target;
        // The private boundary is absolute, so this also excludes relative targets.
        anyhow::ensure!(
            target.starts_with(&self.target_root),
            "unsafe system file target"
        );
        anyhow::ensure!(target != &self.target_root, "unsafe system file target");
        anyhow::ensure!(
            !target.components().any(|component| matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )),
            "unsafe system file target"
        );
        Ok(())
    }

    fn assessment_with_target(
        &self,
        fragment: &str,
        resolved: anyhow::Result<PathBuf>,
    ) -> ResourceResult<(ResourceState, PathBuf)> {
        self.validate_installation_target()?;
        let target = match resolved {
            Ok(target) => target,
            Err(error) if permission_denied(&error) => {
                return Ok((
                    ResourceState::Unknown {
                        reason: "requires privileged assessment; destination was not read"
                            .to_string(),
                    },
                    self.entry.target.clone(),
                ));
            }
            Err(error) => return Err(error.into()),
        };
        let state = match Self::current_content(&target).map_err(anyhow::Error::from) {
            Ok(current) => self.state_for(&current, fragment),
            Err(error) if permission_denied(&error) => ResourceState::Unknown {
                reason: "requires privileged assessment; destination was not read".to_string(),
            },
            Err(error) => return Err(error.into()),
        };
        Ok((state, target))
    }

    fn current_content(target: &std::path::Path) -> ResourceResult<CurrentFile> {
        let metadata = match fs::symlink_metadata(target) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(CurrentFile::Missing),
            Err(error) => {
                return Err(anyhow!(error)
                    .context(format!("reading metadata for {}", target.display()))
                    .into());
            }
        };
        if !metadata.is_file() {
            return Ok(CurrentFile::NonRegular);
        }
        let content =
            fs::read_to_string(target).with_context(|| format!("reading {}", target.display()))?;
        Ok(CurrentFile::Present(content))
    }

    #[cfg(unix)]
    fn desired_content(&self, current: &str, fragment: &str) -> anyhow::Result<String> {
        match self.entry.merge {
            MergeStrategy::Toml => merge_toml(current, fragment),
            MergeStrategy::Ini => merge_ini(current, fragment),
            MergeStrategy::Pam => merge_pam(current, fragment),
            MergeStrategy::CreateOnly => Ok(fragment.to_owned()),
        }
    }

    fn content_is_correct(&self, current: &str, fragment: &str) -> anyhow::Result<bool> {
        match self.entry.merge {
            MergeStrategy::Toml => contains_toml(current, fragment),
            MergeStrategy::Ini => contains_ini(current, fragment),
            MergeStrategy::Pam => Ok(merge_pam(current, fragment)? == current),
            MergeStrategy::CreateOnly => Ok(current == fragment),
        }
    }

    fn state_for(&self, current: &CurrentFile, fragment: &str) -> ResourceState {
        let content = match current {
            CurrentFile::Missing if self.entry.merge == MergeStrategy::Pam => {
                return ResourceState::Invalid {
                    reason: format!(
                        "required PAM service file {} is missing",
                        self.entry.target.display()
                    ),
                };
            }
            CurrentFile::Missing => "",
            CurrentFile::NonRegular => {
                return ResourceState::Invalid {
                    reason: format!("{} is not a regular file", self.entry.target.display()),
                };
            }
            CurrentFile::Present(content) => content,
        };
        match self.content_is_correct(content, fragment) {
            Err(error) => ResourceState::Invalid {
                reason: error.to_string(),
            },
            Ok(_) if matches!(current, CurrentFile::Missing) => ResourceState::Missing,
            Ok(true) => ResourceState::Correct,
            Ok(false) if self.entry.merge == MergeStrategy::CreateOnly => ResourceState::Invalid {
                reason: format!(
                    "{} already exists with different content; review the existing file before retrying",
                    self.entry.target.display()
                ),
            },
            Ok(false) => ResourceState::Incorrect {
                current: "managed content differs".to_string(),
            },
        }
    }
}

impl Resource for SystemFileResource {
    fn description(&self) -> String {
        self.entry.target.display().to_string()
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        let fragment = self.fragment()?;
        if let Err(error) = validate_fragment(self.entry.merge, &fragment) {
            return Err(crate::engine::resource::ResourceError::conflicting_state(
                self.entry.target.display().to_string(),
                "a valid system file and fragment",
                error.to_string(),
            ));
        }
        let (state, target) = self.assessment_with_target(&fragment, self.checked_target())?;
        match state {
            ResourceState::Correct => return Ok(ResourceChange::AlreadyCorrect),
            ResourceState::Invalid { reason } => {
                return Err(crate::engine::resource::ResourceError::conflicting_state(
                    self.entry.target.display().to_string(),
                    "a valid system file and fragment",
                    reason,
                ));
            }
            ResourceState::Missing
            | ResourceState::Incorrect { .. }
            | ResourceState::Unknown { .. } => {}
        }
        let result = self
            .executor
            .execute(
                CommandSpec::new("sudo")
                    .args(&[
                        "-n",
                        "--",
                        "sh",
                        "-c",
                        ATOMIC_INSTALL,
                        "dotfiles-system-file",
                    ])
                    .arg(std::env::current_exe()?)
                    .arg(&target)
                    .arg(dunce::canonicalize(self.entry.source_path())?)
                    .arg(&self.target_root)
                    .arg(match self.entry.merge {
                        MergeStrategy::Toml => "toml",
                        MergeStrategy::Ini => "ini",
                        MergeStrategy::Pam => "pam",
                        MergeStrategy::CreateOnly => "create-only",
                    })
                    .arg(crate::infra::fs::content_hash(fragment.as_bytes()))
                    .redact_arguments()
                    .output_log(OutputLog::Omit),
            )
            .with_context(|| format!("installing {}", self.entry.target.display()))?;
        match result.stdout.trim() {
            "applied" => Ok(ResourceChange::Applied),
            "already-correct" => Ok(ResourceChange::AlreadyCorrect),
            _ => Err(anyhow!("invalid system file installation response").into()),
        }
    }
}

impl IntrinsicState for SystemFileResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        let fragment = self.fragment()?;
        if let Err(error) = validate_fragment(self.entry.merge, &fragment) {
            return Ok(ResourceState::Invalid {
                reason: error.to_string(),
            });
        }
        self.assessment_with_target(&fragment, self.checked_target())
            .map(|(state, _)| state)
    }
}

fn permission_denied(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io_error| io_error.kind() == ErrorKind::PermissionDenied)
    })
}

/// Run only in the scoped installer, without exposing destination content or diagnostics.
pub(crate) fn privileged_prepare(
    target: &std::path::Path,
    fragment_path: &std::path::Path,
    fragment_hash: &str,
    merge: &str,
    stage: &std::path::Path,
) -> anyhow::Result<&'static str> {
    #[cfg(unix)]
    {
        anyhow::ensure!(
            nix::unistd::Uid::effective().is_root(),
            "system file helper requires root"
        );
        anyhow::ensure!(
            target.starts_with("/etc"),
            "system file helper target must be below /etc"
        );
        let fragment = verified_fragment(fragment_path, fragment_hash)?;
        prepare_in_private_stage(target, &fragment, merge, stage, 0)
    }
    #[cfg(not(unix))]
    {
        let _ = (target, fragment_path, fragment_hash, merge, stage);
        bail!("system file helper requires Unix");
    }
}

#[cfg(unix)]
fn verified_fragment(path: &std::path::Path, expected_hash: &str) -> anyhow::Result<String> {
    let bytes = fs::read(path).map_err(|_| anyhow!("could not read system file fragment"))?;
    anyhow::ensure!(
        crate::infra::fs::content_hash(&bytes) == expected_hash,
        "system file fragment changed while preparing update; retry the update"
    );
    String::from_utf8(bytes).map_err(|_| anyhow!("system file fragment is not UTF-8"))
}

#[cfg(unix)]
fn prepare_in_private_stage(
    target: &std::path::Path,
    fragment: &str,
    merge: &str,
    stage: &std::path::Path,
    owner: u32,
) -> anyhow::Result<&'static str> {
    prepare_contents(target, fragment, merge, stage, owner)
        .map_err(|_| anyhow!("system file preparation failed; destination was not published"))
}

#[cfg(unix)]
fn prepare_contents(
    target: &std::path::Path,
    fragment: &str,
    merge: &str,
    stage: &std::path::Path,
    owner: u32,
) -> anyhow::Result<&'static str> {
    use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};

    let parent = target.parent().context("target has no parent")?;
    anyhow::ensure!(
        target.is_absolute()
            && dunce::canonicalize(parent)? == parent
            && stage.parent() == Some(parent),
        "unsafe helper paths"
    );
    for (path, private) in [(parent, false), (stage, true)] {
        let metadata = fs::symlink_metadata(path)?;
        anyhow::ensure!(
            metadata.is_dir()
                && metadata.uid() == owner
                && metadata.mode() & if private { 0o077 } else { 0o022 } == 0,
            "unsafe staging directory"
        );
    }
    let merge = match merge {
        "toml" => MergeStrategy::Toml,
        "ini" => MergeStrategy::Ini,
        "pam" => MergeStrategy::Pam,
        "create-only" => MergeStrategy::CreateOnly,
        _ => bail!("unknown merge strategy"),
    };
    let resource = SystemFileResource::new(
        SystemFile {
            target: target.to_path_buf(),
            source: PathBuf::new(),
            merge,
            origin: None,
        },
        Arc::new(crate::infra::exec::ProcessExecutor::system()),
    );
    let current = SystemFileResource::current_content(target)?;
    match resource.state_for(&current, fragment) {
        ResourceState::Correct => return Ok("already-correct"),
        ResourceState::Invalid { reason } => bail!("{reason}"),
        ResourceState::Missing | ResourceState::Incorrect { .. } => {}
        ResourceState::Unknown { .. } => bail!("unknown system file state"),
    }
    let text = match &current {
        CurrentFile::Missing => "",
        CurrentFile::Present(text) => text,
        CurrentFile::NonRegular => bail!("not a regular file"),
    };
    let desired = resource.desired_content(text, fragment)?;
    for (name, content) in [
        ("input", Some(desired.as_str())),
        (
            "original",
            matches!(current, CurrentFile::Present(_)).then_some(text),
        ),
    ] {
        if let Some(content) = content {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(stage.join(name))?;
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
        }
    }
    Ok("prepared")
}

fn parse_toml(content: &str) -> anyhow::Result<toml::Table> {
    if content.trim().is_empty() {
        Ok(toml::Table::new())
    } else {
        toml::from_str(content).context("parsing TOML system file")
    }
}

#[cfg(any(unix, test))]
fn merge_table(current: &mut dyn toml_edit::TableLike, desired: &dyn toml_edit::TableLike) {
    for (key, desired_value) in desired.iter() {
        if let (Some(current_table), Some(desired_table)) = (
            current
                .get_mut(key)
                .and_then(toml_edit::Item::as_table_like_mut),
            desired_value.as_table_like(),
        ) {
            merge_table(current_table, desired_table);
        } else {
            let mut replacement = desired_value.clone();
            if let (Some(existing), Some(value)) = (
                current.get(key).and_then(toml_edit::Item::as_value),
                replacement.as_value_mut(),
            ) {
                *value.decor_mut() = existing.decor().clone();
            }
            if let Some(existing) = current.get_mut(key) {
                *existing = replacement;
            } else {
                current.insert(key, replacement);
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

#[cfg(any(unix, test))]
fn merge_toml(current: &str, fragment: &str) -> anyhow::Result<String> {
    let mut current = current
        .parse::<toml_edit::DocumentMut>()
        .context("parsing TOML system file")?;
    let fragment = fragment
        .parse::<toml_edit::DocumentMut>()
        .context("parsing TOML system file")?;
    merge_table(current.as_table_mut(), fragment.as_table());
    let mut rendered = current.to_string();
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
    let mut desired_values = std::collections::BTreeMap::new();
    for (index, raw) in content.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[')
            && !rest
                .strip_suffix(']')
                .is_some_and(|name| !name.trim().is_empty() && !name.contains(['[', ']']))
        {
            if strict {
                bail!("INI fragment contains an invalid section header: {line}");
            }
            bail!(
                "invalid section header in installed INI file at line {}",
                index + 1
            );
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
        if strict && key.is_empty() {
            bail!("INI fragment contains an empty key: {line}");
        }
        let Some(section) = &section else {
            if strict {
                bail!("INI fragment contains a setting outside a section: {line}");
            }
            continue;
        };
        if strict
            && let Some(previous) = desired_values.insert((section.clone(), key), value)
            && previous != value
        {
            bail!("INI fragment contains conflicting values for {key} in {section}");
        }
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

#[cfg(any(unix, test))]
fn merge_ini(current: &str, fragment: &str) -> anyhow::Result<String> {
    ini_settings(current, false)?;
    let newline = if current
        .split_once('\n')
        .is_some_and(|(first, _)| first.ends_with('\r'))
    {
        "\r\n"
    } else {
        "\n"
    };
    let mut lines = current
        .split_inclusive('\n')
        .map(str::to_string)
        .collect::<Vec<_>>();
    for (section, key, value) in ini_settings(fragment, true)? {
        ensure_ini_key(&mut lines, &section, &key, value.as_deref(), newline);
    }
    let mut rendered = String::new();
    for line in lines {
        if !rendered.is_empty() && !rendered.ends_with('\n') {
            rendered.push_str(newline);
        }
        rendered.push_str(&line);
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

#[cfg(any(unix, test))]
fn ini_line_key(line: &str) -> Option<&str> {
    ini_setting(line).map(|(key, _)| key)
}

#[cfg(any(unix, test))]
fn ensure_ini_key(
    lines: &mut Vec<String>,
    section: &str,
    key: &str,
    value: Option<&str>,
    newline: &str,
) {
    let mut setting = value.map_or_else(|| key.to_string(), |value| format!("{key}={value}"));
    let Some(first) = lines.iter().position(|line| line.trim() == section) else {
        if lines
            .last()
            .is_some_and(|line| !line.trim_end_matches(['\r', '\n']).is_empty())
        {
            lines.push(newline.to_owned());
        }
        lines.push(format!("{section}{newline}"));
        setting.push_str(newline);
        lines.push(setting);
        return;
    };

    let mut insertion_index = first.saturating_add(1);
    let mut ending = newline;
    for (index, line) in lines
        .iter()
        .enumerate()
        .skip(insertion_index)
        .take_while(|(_, line)| ini_section(line).is_none())
    {
        if let Some(current_key) = ini_line_key(line) {
            if current_key == key {
                insertion_index = index;
                ending = if line.ends_with("\r\n") {
                    "\r\n"
                } else if line.ends_with('\n') {
                    "\n"
                } else {
                    ""
                };
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
    setting.push_str(ending);
    lines.insert(insertion_index, setting);
}

fn same_pam_facility(current: &str, desired: &str) -> bool {
    // A leading '-' suppresses missing-module logging, not stack membership.
    current
        .strip_prefix('-')
        .unwrap_or(current)
        .eq_ignore_ascii_case(desired.strip_prefix('-').unwrap_or(desired))
}

fn pam_control_and_module(fields: &str) -> Option<(&str, &str)> {
    let fields = fields.trim_start();
    fields.strip_prefix('[').map_or_else(
        || {
            Some(
                fields
                    .split_once(char::is_whitespace)
                    .unwrap_or((fields, "")),
            )
        },
        |extended| extended.split_once(']'),
    )
}

fn pam_rule_identity(line: &str) -> anyhow::Result<(&str, &str)> {
    let (facility, fields) = line
        .split_once(char::is_whitespace)
        .context("PAM fragment rule has no control field")?;
    let (control, module_fields) = pam_control_and_module(fields)
        .context("PAM fragment rule has an unterminated control field")?;
    if control.eq_ignore_ascii_case("include") || control.eq_ignore_ascii_case("substack") {
        bail!("PAM fragment rule includes a stack rather than a module: {line}");
    }
    #[allow(
        clippy::case_sensitive_file_extension_comparisons,
        reason = "PAM module names and Linux file extensions are case-sensitive"
    )]
    let Some(module) = module_fields
        .split_whitespace()
        .next()
        .filter(|token| token.ends_with(".so"))
    else {
        bail!("PAM fragment rule has no module: {line}");
    };
    Ok((facility, module))
}

pub(crate) fn validate_fragment(merge: MergeStrategy, fragment: &str) -> anyhow::Result<()> {
    match merge {
        MergeStrategy::Toml => parse_toml(fragment).map(drop),
        MergeStrategy::Ini => ini_settings(fragment, true).map(drop),
        MergeStrategy::Pam => validate_pam_rules(fragment),
        MergeStrategy::CreateOnly => Ok(()),
    }
}

fn pam_rules(fragment: &str) -> impl Iterator<Item = anyhow::Result<(&str, &str, &str)>> {
    fragment
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|rule| {
            anyhow::ensure!(
                pam_continued(rule).is_none(),
                "PAM fragment rules must occupy one line"
            );
            let code = pam_code(rule);
            let (facility, module) = pam_rule_identity(code)?;
            anyhow::ensure!(
                ["auth", "account", "password", "session"]
                    .iter()
                    .any(|known| same_pam_facility(facility, known)),
                "unknown PAM fragment facility: {facility}"
            );
            let fields = code
                .strip_prefix(facility)
                .context("missing PAM facility")?
                .trim_start();
            if !fields.starts_with('[') {
                let (control, _) = pam_control_and_module(fields).context("missing PAM control")?;
                anyhow::ensure!(
                    ["required", "requisite", "sufficient", "optional"]
                        .iter()
                        .any(|known| control.eq_ignore_ascii_case(known)),
                    "unknown PAM fragment control: {control}"
                );
            }
            Ok((rule, facility, module))
        })
}

struct PamLine<'a> {
    text: &'a str,
    code: String,
}

fn pam_code(line: &str) -> &str {
    line.split_once('#').map_or(line, |(code, _)| code).trim()
}

fn pam_continued(line: &str) -> Option<&str> {
    if line.contains('#') {
        return None;
    }
    line.trim_end_matches([' ', '\t', '\n']).strip_suffix('\\')
}

fn pam_service_lines(content: &str) -> anyhow::Result<Vec<PamLine<'_>>> {
    let mut lines = Vec::new();
    let mut pending = content;
    let mut length = 0;
    let mut code = String::new();
    for physical in content.split_inclusive('\n') {
        length += physical.len();
        if let Some(continued) = pam_continued(physical) {
            code.push_str(continued.trim_start());
            code.push(' ');
        } else {
            code.push_str(pam_code(physical));
            let (text, rest) = pending.split_at(length);
            lines.push(PamLine {
                text,
                code: std::mem::take(&mut code),
            });
            pending = rest;
            length = 0;
        }
    }
    anyhow::ensure!(
        pending.is_empty(),
        "PAM service file ends with an unfinished rule"
    );
    Ok(lines)
}

fn pam_has_jump(control: &str) -> bool {
    control.split('=').skip(1).any(|action| {
        action
            .trim_start()
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_digit)
    })
}

// Ignore alignment between PAM fields, not whitespace inside an extended
// control or module argument (e.g. a bracketed path containing spaces).
fn same_pam_rule(left: &str, right: &str) -> bool {
    fn fields(line: &str) -> Option<(&str, bool, &str, &str, &str)> {
        let (facility, fields) = line.trim().split_once(char::is_whitespace)?;
        let fields = fields.trim_start();
        let (control, rest) = pam_control_and_module(fields)?;
        let rest = rest.trim_start();
        let (module, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        Some((
            facility,
            fields.starts_with('['),
            control,
            module,
            args.trim_start(),
        ))
    }
    fields(left).is_some_and(|parsed| fields(right) == Some(parsed))
}

fn merge_pam(current: &str, fragment: &str) -> anyhow::Result<String> {
    validate_pam_rules(fragment)?;
    let mut lines = pam_service_lines(current)?;
    for rule in pam_rules(fragment) {
        let (rule, facility, module) = rule?;
        let mut last_in_stack = None;
        let mut matching = Vec::new();
        let mut has_jump = false;
        for (index, line) in lines.iter().enumerate() {
            let code = line.code.trim();
            let Some((current_facility, fields)) = code.split_once(char::is_whitespace) else {
                continue;
            };
            if !same_pam_facility(current_facility, facility) {
                continue;
            }
            last_in_stack = Some(index);
            if let Some((control, _)) = pam_control_and_module(fields) {
                has_jump |= pam_has_jump(control);
            }
            if let Ok((_, current_module)) = pam_rule_identity(code) {
                if current_module == module {
                    matching.push(index);
                } else if std::path::Path::new(current_module).file_name()
                    == std::path::Path::new(module).file_name()
                {
                    bail!(
                        "ambiguous PAM {facility} module paths {current_module} and {module}. Review the installed rule and the fragment before retrying"
                    );
                }
            }
        }
        let Some(last_in_stack) = last_in_stack else {
            bail!("PAM service has no {facility} stack; refusing to synthesize one");
        };
        let replacement = PamLine {
            text: rule,
            code: pam_code(rule).to_string(),
        };
        match matching.as_slice() {
            [] => {
                // Adding a rule can change where a relative jump lands.
                anyhow::ensure!(
                    !has_jump,
                    "PAM {facility} stack has a jump that could be affected by adding {module}. Review the installed stack before retrying"
                );
                lines.insert(last_in_stack.saturating_add(1), replacement);
            }
            [index] => {
                let installed = lines.get_mut(*index).context("missing matched PAM rule")?;
                if !same_pam_rule(&installed.code, &replacement.code) {
                    *installed = replacement;
                }
            }
            _ => {
                // Separate invocations of the same module can be intentional.
                anyhow::ensure!(
                    matching.iter().any(|index| lines
                        .get(*index)
                        .is_some_and(|line| same_pam_rule(&line.code, &replacement.code))),
                    "multiple PAM {facility} rules use {module}, and none matches the fragment. Review the installed rules and the fragment before retrying"
                );
            }
        }
    }
    let mut rendered = String::new();
    for line in lines {
        if !rendered.is_empty() && !rendered.ends_with('\n') {
            rendered.push('\n');
        }
        rendered.push_str(line.text);
    }
    if !rendered.is_empty() && !rendered.ends_with('\n') && rendered != current {
        rendered.push('\n');
    }
    Ok(rendered)
}

fn validate_pam_rules(fragment: &str) -> anyhow::Result<()> {
    let mut identities = std::collections::BTreeMap::new();
    for rule in pam_rules(fragment) {
        let (rule, facility, module) = rule?;
        let facility = facility
            .strip_prefix('-')
            .unwrap_or(facility)
            .to_ascii_lowercase();
        if let Some(previous) = identities.insert((facility, module), rule) {
            anyhow::ensure!(
                previous == rule,
                "PAM fragment contains conflicting rules for module {module}: {previous}; {rule}"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_merge_preserves_comments_and_layout_for_changed_and_unmanaged_keys() {
        let original = "# administrator heading\n\
            unmanaged = 'keep' # secret description\n\
            managed  = false # why this setting exists\n\n\
            # nested settings\n[service]\n\
            keep = { value = 1, other = 'retained' } # inline comment\n";
        let fragment = "managed = true\n[service]\nkeep = { value = 2 }\nnew = 'added'\n";
        let merged = merge_toml(original, fragment).unwrap();
        assert!(merged.starts_with("# administrator heading\n"));
        assert!(merged.contains("unmanaged = 'keep' # secret description\n"));
        assert!(merged.contains("managed  = true # why this setting exists\n"));
        assert!(merged.contains("# nested settings\n[service]\n"));
        assert!(merged.contains("other = 'retained'"));
        assert!(merged.contains("# inline comment"));
        assert!(contains_toml(&merged, fragment).unwrap());
        assert_eq!(merge_toml(&merged, fragment).unwrap(), merged);
    }

    #[cfg(unix)]
    #[test]
    fn privileged_fragment_read_rejects_changed_content_without_exposing_it() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let path = fixture.path().join("fragment");
        let fragment = "managed = true\n";
        let fingerprint = crate::infra::fs::content_hash(fragment);
        fs::write(&path, fragment).unwrap();
        assert_eq!(verified_fragment(&path, &fingerprint).unwrap(), fragment);
        fs::write(&path, "private = 'substituted fixture secret'\n").unwrap();
        let error = verified_fragment(&path, &fingerprint).unwrap_err();
        assert!(error.to_string().contains("fragment changed"));
        assert!(!format!("{error:#}").contains("substituted fixture secret"));
    }

    #[cfg(unix)]
    #[test]
    fn standalone_helper_rejects_unprivileged_calls_and_out_of_scope_targets_before_reading() {
        let error = privileged_prepare(
            Path::new("/outside/etc/target"),
            Path::new("source-must-not-be-read"),
            "fingerprint",
            "toml",
            Path::new("stage-must-not-be-created"),
        )
        .unwrap_err();
        let reason = if nix::unistd::Uid::effective().is_root() {
            "target must be below /etc"
        } else {
            "helper requires root"
        };
        assert!(error.to_string().contains(reason), "{error}");
    }

    #[test]
    fn assessment_defers_only_permission_failures_without_reading_unresolved_targets() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let target = root.join("unresolved");
        let resource = fixture_resource(
            entry(&root, &target, "fragment", MergeStrategy::Toml),
            Arc::new(MockExecutor::new()),
        );
        for kind in [
            ErrorKind::PermissionDenied,
            ErrorKind::NotFound,
            ErrorKind::Other,
        ] {
            let failure = anyhow!(std::io::Error::new(kind, "fixture resolution failed"))
                .context("resolve target parent");
            let result = resource.assessment_with_target("managed = true\n", Err(failure));
            if kind == ErrorKind::PermissionDenied {
                let (state, pending_target) = result.unwrap();
                assert!(matches!(state, ResourceState::Unknown { .. }), "{state:?}");
                assert_eq!(pending_target, target);
            } else {
                let error = result.unwrap_err();
                assert!(
                    format!("{error:#}").contains("fixture resolution failed"),
                    "{error}"
                );
            }
            assert!(!target.exists());
            assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        }
    }

    #[test]
    fn unsafe_configured_targets_cannot_be_assessed_or_delegated_after_permission_failures() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = dunce::canonicalize(fixture.path()).unwrap();
        fs::create_dir(root.join("system")).unwrap();
        fs::write(root.join("system/fragment"), "managed = true\n").unwrap();
        for target in [
            PathBuf::from("relative/target"),
            root.clone(),
            root.parent().unwrap().join("outside"),
            root.join("missing/../target"),
        ] {
            let resource = fixture_resource(
                entry(&root, &target, "fragment", MergeStrategy::Toml),
                Arc::new(MockExecutor::new()),
            );
            let denied = anyhow!(std::io::Error::from(ErrorKind::PermissionDenied));
            assert!(
                resource
                    .assessment_with_target("managed = true\n", Err(denied))
                    .is_err(),
                "{target:?}"
            );
            assert!(resource.apply().is_err(), "{target:?}");
            assert!(!root.join("target").exists());
            assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        }
    }

    #[cfg(unix)]
    #[test]
    fn private_preparation_merges_ini_and_pam_then_converges_without_new_staged_contents() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        for (strategy, original, fragment, expected) in [
            (
                "ini",
                "# administrator\r\n[boot]\r\nsystemd=false\r\n\n[network]\r\n# retained",
                "[boot]\nsystemd=true\n",
                "# administrator\r\n[boot]\r\nsystemd=true\r\n\n[network]\r\n# retained",
            ),
            (
                "ini",
                "# retained\n[options]\nkeep=private\nmanaged=no\n",
                "[options]\nmanaged=yes\n",
                "# retained\n[options]\nkeep=private\nmanaged=yes\n",
            ),
            (
                "pam",
                "# retained\nauth include system-auth\nsession include system-login\n",
                "auth optional fixture.so\n",
                "# retained\nauth include system-auth\nauth optional fixture.so\nsession include system-login\n",
            ),
        ] {
            let fixture = tempfile::tempdir_in(".").unwrap();
            let root = fixture.path().canonicalize().unwrap();
            let target = root.join("target");
            let stage = root.join("stage");
            fs::write(&target, original).unwrap();
            fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
            fs::create_dir(&stage).unwrap();
            fs::set_permissions(&stage, fs::Permissions::from_mode(0o700)).unwrap();
            let owner = fs::metadata(&root).unwrap().uid();
            assert_eq!(
                prepare_in_private_stage(&target, fragment, strategy, &stage, owner).unwrap(),
                "prepared",
                "{strategy}"
            );
            assert_eq!(
                fs::read_to_string(stage.join("input")).unwrap(),
                expected,
                "{strategy}"
            );
            assert_eq!(
                fs::read_to_string(stage.join("original")).unwrap(),
                original,
                "{strategy}"
            );
            assert_eq!(fs::read_to_string(&target).unwrap(), original, "{strategy}");
            fs::write(&target, expected).unwrap();
            fs::remove_file(stage.join("input")).unwrap();
            fs::remove_file(stage.join("original")).unwrap();
            assert_eq!(
                prepare_in_private_stage(&target, fragment, strategy, &stage, owner).unwrap(),
                "already-correct",
                "{strategy}"
            );
            assert_eq!(fs::read_dir(&stage).unwrap().count(), 0, "{strategy}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn protected_targets_and_parents_remain_unknown_without_executor_calls_or_preview_writes() {
        use crate::engine::{ProcessOpts, process_resources_with_cache};
        use crate::test_helpers::{ContextBuilder, task_batch};
        use std::os::unix::fs::PermissionsExt as _;

        if nix::unistd::Uid::effective().is_root() {
            return; // Root is not constrained by Unix file permission bits.
        }
        for protect_parent in [false, true] {
            let fixture = tempfile::tempdir_in(".").unwrap();
            let root = fixture.path().canonicalize().unwrap();
            fs::create_dir(root.join("system")).unwrap();
            fs::write(root.join("system/fragment"), "managed = true\n").unwrap();
            let parent = root.join("protected");
            fs::create_dir(&parent).unwrap();
            let nested = parent.join("nested");
            fs::create_dir(&nested).unwrap();
            let target = nested.join("target");
            let original = "private = 'fixture secret'\nmanaged = true\n";
            fs::write(&target, original).unwrap();
            let restricted = if protect_parent { &parent } else { &target };
            let permissions = fs::metadata(restricted).unwrap().permissions();
            fs::set_permissions(restricted, fs::Permissions::from_mode(0o0)).unwrap();
            let resource = fixture_resource(
                entry(&root, &target, "fragment", MergeStrategy::Toml),
                Arc::new(MockExecutor::new()),
            );
            let observed = resource.current_state().unwrap();
            assert!(
                matches!(observed, ResourceState::Unknown { .. }),
                "{observed:?}"
            );
            let ctx = ContextBuilder::new(root.clone())
                .overlay(None)
                .build()
                .with_dry_run(true);
            let result = process_resources_with_cache(
                &ctx,
                [resource],
                &(),
                |resource, ()| {
                    resource.current_state().map(|state| match state {
                        ResourceState::Unknown { reason } => {
                            ResourceState::Incorrect { current: reason }
                        }
                        unchanged @ (ResourceState::Missing
                        | ResourceState::Correct
                        | ResourceState::Incorrect { .. }
                        | ResourceState::Invalid { .. }) => unchanged,
                    })
                },
                &ProcessOpts::strict("configure"),
            )
            .unwrap();
            assert_eq!(task_batch(&result).changed_count(), 1);
            let mut executor = MockExecutor::new();
            executor.expect_execute().once().returning(|spec| {
                assert_eq!(spec.program(), "sudo");
                assert_eq!(spec.arguments()[0], "-n");
                assert!(spec.is_checked());
                Ok(ExecResult::success("already-correct\n"))
            });
            let pending = fixture_resource(
                entry(&root, &target, "fragment", MergeStrategy::Toml),
                Arc::new(executor),
            );
            assert_eq!(pending.apply().unwrap(), ResourceChange::AlreadyCorrect);
            fs::set_permissions(restricted, permissions).unwrap();
            assert_eq!(fs::read_to_string(&target).unwrap(), original);
            assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
        }
    }

    #[cfg(unix)]
    #[test]
    fn preparation_keeps_original_and_merged_contents_in_verified_private_stage() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let target = root.join("target");
        let stage = root.join("private-stage");
        fs::create_dir(&stage).unwrap();
        fs::set_permissions(&stage, fs::Permissions::from_mode(0o700)).unwrap();
        let original = "# confidential\nprivate = 'fixture secret'\nmanaged = false\n";
        fs::write(&target, original).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        let owner = fs::metadata(&root).unwrap().uid();
        assert_eq!(
            prepare_in_private_stage(&target, "managed = true\n", "toml", &stage, owner).unwrap(),
            "prepared"
        );
        assert_eq!(
            fs::read_to_string(stage.join("original")).unwrap(),
            original
        );
        let merged = fs::read_to_string(stage.join("input")).unwrap();
        assert!(merged.contains("private = 'fixture secret'"));
        assert!(merged.contains("# confidential"));
        for name in ["input", "original"] {
            let metadata = fs::metadata(stage.join(name)).unwrap();
            assert_eq!(metadata.uid(), owner);
            assert_eq!(metadata.mode() & 0o777, 0o600);
            fs::remove_file(stage.join(name)).unwrap();
        }
        assert_eq!(fs::read_to_string(&target).unwrap(), original);
        fs::write(&target, merged).unwrap();
        assert_eq!(
            prepare_in_private_stage(&target, "managed = true\n", "toml", &stage, owner).unwrap(),
            "already-correct"
        );
        assert_eq!(fs::read_dir(stage).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn preparation_refuses_public_or_linked_staging_and_sanitizes_target_parse_errors() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let target = root.join("target");
        let stage = root.join("stage");
        fs::create_dir(&stage).unwrap();
        fs::write(&target, "private = 'fixture secret'\nmanaged = false\n").unwrap();
        let owner = fs::metadata(&root).unwrap().uid();
        fs::set_permissions(&stage, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            prepare_in_private_stage(&target, "managed = true\n", "toml", &stage, owner).is_err()
        );
        assert_eq!(fs::read_dir(&stage).unwrap().count(), 0);
        fs::set_permissions(&stage, fs::Permissions::from_mode(0o700)).unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink(&stage, &link).unwrap();
        assert!(
            prepare_in_private_stage(&target, "managed = true\n", "toml", &link, owner).is_err()
        );
        let invalid = "private = [fixture secret\n";
        fs::write(&target, invalid).unwrap();
        let error = prepare_in_private_stage(&target, "managed = true\n", "toml", &stage, owner)
            .unwrap_err();
        assert!(!format!("{error:#}").contains("fixture secret"));
        assert_eq!(fs::read_to_string(&target).unwrap(), invalid);
        assert_eq!(fs::read_dir(stage).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn privileged_assessment_refuses_missing_pam_and_nonregular_targets_without_staging_contents() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let target = root.join("target");
        let stage = root.join("stage");
        fs::create_dir(&stage).unwrap();
        fs::set_permissions(&stage, fs::Permissions::from_mode(0o700)).unwrap();
        let owner = fs::metadata(&root).unwrap().uid();
        assert!(
            prepare_in_private_stage(&target, "auth optional fixture.so\n", "pam", &stage, owner,)
                .is_err()
        );
        assert!(!target.exists());
        assert_eq!(fs::read_dir(&stage).unwrap().count(), 0);
        let unmanaged = root.join("unmanaged");
        fs::write(&unmanaged, "private = 'fixture secret'\n").unwrap();
        std::os::unix::fs::symlink(&unmanaged, &target).unwrap();
        assert!(
            prepare_in_private_stage(&target, "managed = true\n", "toml", &stage, owner).is_err()
        );
        assert!(fs::symlink_metadata(&target).unwrap().is_symlink());
        assert_eq!(
            fs::read_to_string(&unmanaged).unwrap(),
            "private = 'fixture secret'\n"
        );
        assert_eq!(fs::read_dir(stage).unwrap().count(), 0);
    }

    #[test]
    fn pam_conflicts_fail_validation_and_merge_without_silently_dropping_rules() {
        let original = "auth required pam_unix.so\n";
        for facility in ["auth", "AUTH", "-auth"] {
            let fragment = format!(
                "auth optional pam_example.so first\n{facility} required pam_example.so second\n"
            );
            assert!(
                validate_fragment(MergeStrategy::Pam, &fragment)
                    .unwrap_err()
                    .to_string()
                    .contains("conflicting rules")
            );
            assert!(merge_pam(original, &fragment).is_err());
        }
        let rule = "auth optional pam_example.so first\n";
        let duplicate = format!("{rule}{rule}");
        validate_fragment(MergeStrategy::Pam, &duplicate).unwrap();
        let merged = merge_pam(original, &duplicate).unwrap();
        assert!(merged.contains(original));
        assert_eq!(merged.matches("pam_example.so").count(), 1);
        assert_eq!(merge_pam(&merged, &duplicate).unwrap(), merged);
    }
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

    fn fixture_resource(entry: SystemFile, executor: Arc<dyn Executor>) -> SystemFileResource {
        let root = entry.origin.clone().unwrap();
        let mut resource = SystemFileResource::new(entry, executor);
        resource.target_root = root;
        resource
    }

    #[test]
    fn description_is_the_target_path() {
        let root = tempfile::tempdir_in(".").unwrap();
        let target = Path::new("/etc/pacman.conf");
        let resource = fixture_resource(
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
    fn pam_refuses_conflicting_module_paths_in_the_same_facility() {
        let installed = "auth required /usr/lib/security/pam_gnome_keyring.so\n";
        let desired = "auth optional pam_gnome_keyring.so\n";
        let error = merge_pam(installed, desired).unwrap_err().to_string();
        assert!(error.contains("ambiguous PAM auth module paths"), "{error}");
        assert!(
            error.contains("/usr/lib/security/pam_gnome_keyring.so"),
            "{error}"
        );
        assert!(merge_pam(
            "session required /usr/lib/security/pam_gnome_keyring.so\nauth include system-auth\n",
            desired
        )
        .is_ok());
    }

    #[test]
    fn missing_pam_target_cannot_be_created_by_direct_apply() {
        for fragment in ["", "# Managed login rules\n", "auth optional keyring.so\n"] {
            let root = tempfile::tempdir_in(".").unwrap();
            fs::create_dir(root.path().join("system")).unwrap();
            fs::write(root.path().join("system/login"), fragment).unwrap();
            let target = root.path().join("missing-login");
            let resource = fixture_resource(
                entry(root.path(), &target, "login", MergeStrategy::Pam),
                Arc::new(MockExecutor::new()),
            );

            assert!(matches!(
                resource.current_state().unwrap(),
                ResourceState::Invalid { reason } if reason.contains("is missing")
            ));
            let error = resource.apply().unwrap_err();

            assert!(error.to_string().contains("is missing"), "{error}");
            assert!(!target.exists(), "{fragment:?}");
            assert!(!root.path().join("staging-must-not-be-created").exists());
        }
    }

    #[test]
    fn pam_fragment_comments_do_not_become_rules_or_target_content() {
        let current = "# Preserve this comment\nauth include system-auth\n";
        let fragment = "# Managed rules\n\n  # Ignore indented comments\n\
            auth optional keyring.so\n";
        let merged = merge_pam(current, fragment).unwrap();

        assert_eq!(merged, format!("{current}auth optional keyring.so\n"));
        assert_eq!(merge_pam(&merged, fragment).unwrap(), merged);
        assert_eq!(merge_pam(current, "# No rules\n").unwrap(), current);
    }

    #[test]
    fn pam_merge_preserves_a_stack_containing_only_the_managed_module() {
        let fragment = "auth optional keyring.so\n";
        for current in [
            fragment,
            "# retained\nauth required keyring.so old_option\n",
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
    fn pam_merge_preserves_every_invocation_when_one_rule_matches() {
        let fragment =
            "auth sufficient pam_listfile.so item=user sense=allow onerr=fail file=/etc/allow-a\n";
        let other =
            "auth sufficient pam_listfile.so item=user sense=allow onerr=fail file=/etc/allow-b\n";
        let fallback = "auth required pam_deny.so\n";
        for current in [
            format!("{fragment}{other}{fallback}"),
            format!("{other}{fragment}{fallback}"),
            format!("{fragment}{fragment}{fallback}"),
            format!("auth [success=1 default=ignore] pam_permit.so\n{other}{fragment}{fallback}"),
            format!("auth include system-auth\n{other}{fragment}{fallback}"),
        ] {
            assert_eq!(merge_pam(&current, fragment).unwrap(), current);
        }
        let current = format!("{other}{fragment}{fallback}");
        let changed = fragment.replace("allow-a", "allow-c");
        let error = merge_pam(&current, &changed).unwrap_err().to_string();
        assert!(error.contains("none matches the fragment"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn pam_retry_preserves_equivalent_site_layout_and_adds_only_missing_session_rule() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        const LOGIN: &str = include_str!("../../../../../system/pam.d/login");
        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let target = root.join("login");
        let stage = root.join("stage");
        fs::create_dir(&stage).unwrap();
        fs::set_permissions(&stage, fs::Permissions::from_mode(0o700)).unwrap();
        let owner = fs::metadata(&root).unwrap().uid();
        let legacy = "auth\toptional\tpam_gnome_keyring.so # local tty policy\n";
        let site = "auth optional pam_gnome_keyring.so only_if=gdm # graphical login\n\
                    session include system-login\n";
        let ambiguous = format!("auth optional pam_gnome_keyring.so only_if=login\n{site}");
        fs::write(&target, &ambiguous).unwrap();
        let prepare = || prepare_in_private_stage(&target, LOGIN, "pam", &stage, owner);
        assert!(prepare().is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), ambiguous);
        assert_eq!(fs::read_dir(&stage).unwrap().count(), 0);

        let reviewed = format!("{legacy}{site}");
        fs::write(&target, &reviewed).unwrap();
        assert_eq!(prepare().unwrap(), "prepared");
        let session = LOGIN
            .lines()
            .find(|line| line.starts_with("session"))
            .unwrap();
        let expected = format!("{reviewed}{session}\n");
        assert_eq!(fs::read_to_string(stage.join("input")).unwrap(), expected);
        assert_eq!(
            fs::read_to_string(stage.join("original")).unwrap(),
            reviewed
        );
        fs::rename(stage.join("input"), &target).unwrap();
        fs::remove_file(stage.join("original")).unwrap();
        assert_eq!(prepare().unwrap(), "already-correct");
        assert_eq!(fs::read_dir(&stage).unwrap().count(), 0);
        assert_eq!(fs::read_to_string(&target).unwrap(), expected);

        let argument = "auth optional site.so [config=/etc/kiosk  site]";
        assert!(!same_pam_rule(
            argument,
            &argument.replace("kiosk  site", "kiosk site")
        ));
        assert_eq!(
            merge_pam(legacy, LOGIN.lines().next().unwrap()).unwrap(),
            legacy
        );
    }

    #[test]
    fn pam_merge_preserves_the_rule_a_numeric_jump_skips() {
        let before = "# retain layout and comments\n\
            auth [success=1 default=die] pam_permit.so\n\
            auth optional pam_gnome_keyring.so # existing rule\n\
            auth sufficient pam_permit.so\n\
            auth requisite pam_deny.so\n";
        let fragment = "auth optional pam_gnome_keyring.so\n";
        assert_eq!(merge_pam(before, fragment).unwrap(), before);

        let updated = "auth optional pam_gnome_keyring.so debug\n";
        let expected = before.replace(
            "auth optional pam_gnome_keyring.so # existing rule\n",
            updated,
        );
        assert_eq!(merge_pam(before, updated).unwrap(), expected);
        assert_eq!(merge_pam(&expected, updated).unwrap(), expected);
    }

    #[test]
    fn pam_merge_refuses_to_change_stack_length_around_jumps() {
        let fragment = "auth optional keyring.so\n";
        let jump = "-AUTH [success = \\\n    1 default=die] pam_permit.so\n";
        let error = merge_pam(jump, fragment).unwrap_err().to_string();
        assert!(error.contains("could be affected"), "{error}");

        let other_facility = format!("{jump}session required pam_permit.so\n");
        let session = "session optional keyring.so\n";
        assert_eq!(
            merge_pam(&other_facility, session).unwrap(),
            format!("{other_facility}{session}")
        );
    }

    #[test]
    fn pam_merge_replaces_a_continued_rule_as_one_entry() {
        let current = "auth optional keyring.so \\\n    old_option\n\
            auth required pam_permit.so\n";
        let fragment = "auth optional keyring.so new_option\n";
        assert_eq!(
            merge_pam(current, fragment).unwrap(),
            format!("{fragment}auth required pam_permit.so\n")
        );
        assert!(
            validate_fragment(MergeStrategy::Pam, "auth optional keyring.so \\\n").is_err(),
            "a fragment must not continue into the next installed rule"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unsafe_pam_stack_edit_does_not_stage_any_files() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let target = root.join("login");
        let stage = root.join("stage");
        fs::create_dir(&stage).unwrap();
        fs::set_permissions(&stage, fs::Permissions::from_mode(0o700)).unwrap();
        let owner = fs::metadata(&root).unwrap().uid();
        let fragment = "auth optional keyring.so\n";
        for (installed, expected) in [
            (
                "auth [success=1 default=die] pam_permit.so\nauth required pam_permit.so\n",
                None,
            ),
            (
                "auth optional keyring.so first\nauth optional keyring.so second\n",
                None,
            ),
            (
                "auth optional keyring.so first\nauth optional keyring.so\n",
                Some("already-correct"),
            ),
        ] {
            fs::write(&target, installed).unwrap();
            let result = prepare_in_private_stage(&target, fragment, "pam", &stage, owner);
            assert_eq!(result.ok(), expected, "{installed}");
            assert_eq!(fs::read_to_string(&target).unwrap(), installed);
            assert_eq!(fs::read_dir(&stage).unwrap().count(), 0);
        }
    }

    #[test]
    fn pam_merge_does_not_claim_module_arguments_or_stack_names() {
        let unmanaged = "auth include system-auth\n\
            auth required pam_exec.so keyring.so\n\
            auth [success=1 default=ignore] pam_debug.so keyring.so\n\
            -AUTH INCLUDE keyring.so\n\
            AUTH SUBSTACK keyring.so\n\
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
    fn pam_merge_recognizes_prefixed_and_case_insensitive_facilities() {
        for (current, fragment, expected) in [
            (
                "-auth include system-auth\nsession include system-login\n",
                "auth optional keyring.so\n",
                "-auth include system-auth\nauth optional keyring.so\nsession include system-login\n",
            ),
            (
                "auth include system-auth\n-auth optional keyring.so old_option\n",
                "auth optional keyring.so\n",
                "auth include system-auth\nauth optional keyring.so\n",
            ),
            (
                "session include system-login\nSESSION optional keyring.so old_option\n",
                "-session optional keyring.so\n",
                "session include system-login\n-session optional keyring.so\n",
            ),
            (
                "-session optional keyring.so\n",
                "SESSION optional keyring.so\n",
                "SESSION optional keyring.so\n",
            ),
        ] {
            let merged = merge_pam(current, fragment).unwrap();
            assert_eq!(merged, expected, "{current:?} with {fragment:?}");
            assert_eq!(merge_pam(&merged, fragment).unwrap(), merged);
        }
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
            "-AUTH INCLUDE keyring.so",
            "AUTH SUBSTACK keyring.so",
            "auth [success=1 keyring.so",
        ] {
            assert!(
                pam_rule_identity(rule).is_err(),
                "a stack name or argument is not a module field: {rule}"
            );
        }
    }

    #[test]
    fn typo_in_fragment_control_cannot_replace_a_working_login_rule() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let target = root.join("login");
        let installed = "# kiosk login policy\nauth required pam_permit.so\n";
        fs::write(&target, installed).unwrap();
        fs::create_dir(root.join("system")).unwrap();
        let fragment = root.join("system/login");
        let resource = fixture_resource(
            entry(&root, &target, "login", MergeStrategy::Pam),
            Arc::new(MockExecutor::new()),
        );
        for rule in [
            "auth requried pam_permit.so\n",
            "aut required pam_permit.so\n",
        ] {
            fs::write(&fragment, rule).unwrap();
            assert!(validate_fragment(MergeStrategy::Pam, rule).is_err());
            assert!(matches!(
                resource.current_state().unwrap(),
                ResourceState::Invalid { .. }
            ));
            assert!(resource.apply().is_err());
            assert_eq!(fs::read_to_string(&target).unwrap(), installed);
        }
        fs::write(fragment, "auth required pam_permit.so\n").unwrap();
        assert_eq!(resource.apply().unwrap(), ResourceChange::AlreadyCorrect);
    }

    #[cfg(unix)]
    #[test]
    fn paccache_drop_in_never_replaces_custom_environment_lists() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        const DROP_IN: &str =
            include_str!("../../../../../system/systemd/system/paccache.service.d/retention.conf");
        const ENV_FILE: &str =
            include_str!("../../../../../system/systemd/system/paccache.service.d/retention.env");
        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let stage = root.join("stage");
        fs::create_dir(&stage).unwrap();
        fs::set_permissions(&stage, fs::Permissions::from_mode(0o700)).unwrap();
        let owner = fs::metadata(&root).unwrap().uid();
        let previous = "[Service]\nEnvironmentFile=\nEnvironment=PACCACHE_ARGS=-k2\n";
        let custom = "[Service]\nEnvironment=SITE=blue\nEnvironmentFile=/srv/site/cache.env\n\
                      EnvironmentFile=-/srv/mirror.env\nNice=7\n";
        let directives: Vec<_> = DROP_IN
            .lines()
            .filter(|line| line.starts_with("EnvironmentFile="))
            .collect();
        assert_eq!(
            directives,
            ["EnvironmentFile=/etc/systemd/system/paccache.service.d/retention.env"]
        );
        assert!(DROP_IN.contains(
            "\nConditionPathExists=/etc/systemd/system/paccache.service.d/retention.env\n"
        ));
        assert_eq!(ENV_FILE.trim(), "PACCACHE_ARGS=-k2");
        for (name, desired, conflicts) in [
            ("retention.conf", DROP_IN, [previous, custom]),
            (
                "retention.env",
                ENV_FILE,
                ["", "# operator override\nCACHE=/srv/private\n"],
            ),
        ] {
            let target = root.join(name);
            for existing in conflicts {
                fs::write(&target, existing).unwrap();
                assert!(
                    prepare_in_private_stage(&target, desired, "create-only", &stage, owner)
                        .is_err()
                );
                assert_eq!(fs::read_to_string(&target).unwrap(), existing);
                assert_eq!(fs::read_dir(&stage).unwrap().count(), 0);
            }
            let backup = root.join(format!("{name}.before-review"));
            fs::rename(&target, &backup).unwrap();
            assert_eq!(
                prepare_in_private_stage(&target, desired, "create-only", &stage, owner).unwrap(),
                "prepared"
            );
            assert!(!target.exists());
            fs::rename(stage.join("input"), &target).unwrap();
            assert_eq!(fs::read_to_string(&target).unwrap(), desired);
            assert_eq!(
                prepare_in_private_stage(&target, desired, "create-only", &stage, owner).unwrap(),
                "already-correct"
            );
            assert_eq!(fs::read_dir(&stage).unwrap().count(), 0);
            assert_eq!(fs::read_to_string(backup).unwrap(), conflicts[1]);
        }
    }

    #[test]
    fn paccache_policy_file_is_required_before_publishing_its_drop_in() {
        use crate::engine::{ProcessOpts, process_resources_with_cache};
        use crate::test_helpers::make_linux_context;

        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let mut entries = super::super::super::config::system_files::load_all(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../conf/system-files.toml"),
        )
        .unwrap();
        entries.retain(|entry| {
            entry
                .source
                .starts_with("systemd/system/paccache.service.d")
        });
        assert_eq!(entries.len(), 2);
        for entry in &mut entries {
            entry.origin = Some(root.clone());
            entry.target = root.join(entry.source.file_name().unwrap());
            let src = entry.source_path();
            fs::create_dir_all(src.parent().unwrap()).unwrap();
            fs::copy(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../system")
                    .join(&entry.source),
                src,
            )
            .unwrap();
        }
        let env_path = root.join("retention.env");
        let drop_in = root.join("retention.conf");
        let customized = "# site-specific mirror cache\nPACCACHE_ARGS=-k10\n";
        fs::write(&env_path, customized).unwrap();
        let run = || {
            process_resources_with_cache(
                &make_linux_context(root.clone(), None),
                entries
                    .iter()
                    .cloned()
                    .map(|entry| fixture_resource(entry, Arc::new(MockExecutor::new()))),
                &(),
                |resource, ()| resource.installation_state(),
                &ProcessOpts::strict("configure").sequential(),
            )
        };
        assert!(run().is_err());
        assert!(
            !drop_in.exists(),
            "must not activate custom cleanup arguments"
        );
        assert_eq!(fs::read_to_string(&env_path).unwrap(), customized);
        // After the first installation step is interrupted the old service still
        // has no reference to the new env file, and retry need only add the drop-in.
        fs::write(&env_path, "PACCACHE_ARGS=-k2\n").unwrap();
        let states: Vec<_> = entries
            .iter()
            .cloned()
            .map(|entry| {
                fixture_resource(entry, Arc::new(MockExecutor::new()))
                    .installation_state()
                    .unwrap()
            })
            .collect();
        assert_eq!(states, [ResourceState::Correct, ResourceState::Missing]);
        let previous = "[Service]\nEnvironmentFile=\nEnvironment=PACCACHE_ARGS=-k2\n";
        fs::write(&drop_in, previous).unwrap();
        assert!(run().is_err());
        assert_eq!(fs::read_to_string(&drop_in).unwrap(), previous);
        // Operator review: keep the complete old file outside the active drop-ins.
        fs::rename(&drop_in, root.join("reviewed-old.conf.bak")).unwrap();
        fs::copy(entries.last().unwrap().source_path(), &drop_in).unwrap();
        assert!(run().is_ok());
        fs::write(&drop_in, "# post-install operator revision\n").unwrap();
        assert!(run().is_err());
        assert_eq!(
            fs::read_to_string(&drop_in).unwrap(),
            "# post-install operator revision\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("reviewed-old.conf.bak")).unwrap(),
            previous
        );
    }

    #[cfg(unix)]
    #[test]
    fn pam_resource_accepts_an_already_converged_single_module_stack() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = tempfile::tempdir_in(".").unwrap();
        fs::create_dir(root.path().join("system")).unwrap();
        let content = "password optional keyring.so\n";
        fs::write(root.path().join("system/passwd"), content).unwrap();
        let target = root.path().join("passwd");
        fs::write(&target, content).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        let resource = fixture_resource(
            entry(root.path(), &target, "passwd", MergeStrategy::Pam),
            Arc::new(MockExecutor::new()),
        );

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
            ("empty value", "key \t= \t", Some(("key", Some("")))),
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
    fn malformed_ini_section_cannot_claim_a_key_from_another_section() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let root = dunce::canonicalize(fixture.path()).unwrap();
        fs::create_dir(root.join("system")).unwrap();
        fs::write(root.join("system/fragment"), "[boot]\ncommand=managed\n").unwrap();
        let target = root.join("wsl.conf");
        let mut executor = MockExecutor::new();
        executor.expect_execute().never();
        let resource = fixture_resource(
            entry(&root, &target, "fragment", MergeStrategy::Ini),
            Arc::new(executor),
        );
        for value in ["managed", "site-local"] {
            let original = format!("[boot]\n[other\ncommand={value}\n");
            fs::write(&target, &original).unwrap();
            assert!(merge_ini(&original, "[boot]\ncommand=managed\n").is_err());
            assert!(matches!(resource.current_state().unwrap(),
                ResourceState::Invalid { reason } if reason.contains("line 2")));
            assert!(resource.apply().is_err());
            assert_eq!(fs::read_to_string(&target).unwrap(), original);
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
    fn ini_fragments_reject_conflicting_values_including_repeated_sections() {
        for fragment in [
            "[options]\nmanaged=first\nmanaged=second\n",
            "[options]\nmanaged\nmanaged=value\n",
            "[options]\nmanaged=first\n[other]\nkeep=yes\n[options]\nmanaged=second\n",
        ] {
            let error = ini_settings(fragment, true).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("conflicting values for managed in [options]"),
                "{fragment}: {error}"
            );
            assert!(merge_ini("", fragment).is_err(), "{fragment}");
            assert!(contains_ini("", fragment).is_err(), "{fragment}");
            assert!(
                ini_settings(fragment, false).is_ok(),
                "existing target duplicates remain repairable"
            );
        }
    }

    #[test]
    fn identical_ini_fragment_duplicates_and_keys_in_other_sections_converge() {
        let fragment = "[options]\nmanaged=value\nmanaged=value\nFlag\nFlag\n\
            [other]\nmanaged=other\n[options]\nmanaged=value\n";
        let current = "[options]\nmanaged=old\nmanaged=duplicate\nFlag=old\n\
            [unmanaged]\nmanaged=keep\n";

        let merged = merge_ini(current, fragment).unwrap();

        assert!(contains_ini(&merged, fragment).unwrap());
        assert_eq!(merge_ini(&merged, fragment).unwrap(), merged);
        assert!(merged.contains("[unmanaged]\nmanaged=keep\n"));
        assert_eq!(merged.matches("managed=value").count(), 1);
        assert_eq!(merged.lines().filter(|line| *line == "Flag").count(), 1);
        assert!(merged.contains("[other]\nmanaged=other\n"));
    }

    #[test]
    fn malformed_fragments_are_invalid_for_missing_and_existing_targets() {
        for (merge, fragment, reason) in [
            (MergeStrategy::Toml, "managed = [\n", "parsing TOML"),
            (MergeStrategy::Ini, "managed=true\n", "outside a section"),
            (MergeStrategy::Ini, "[options]\n = value\n", "empty key"),
            (MergeStrategy::Ini, "[options]\n = \n", "empty key"),
            (
                MergeStrategy::Ini,
                "[options]\n[broken\n",
                "invalid section header",
            ),
            (
                MergeStrategy::Ini,
                "[]\nkey=value\n",
                "invalid section header",
            ),
            (MergeStrategy::Ini, "[ \t]\n", "invalid section header"),
            (MergeStrategy::Ini, "[[nested]]\n", "invalid section header"),
            (
                MergeStrategy::Ini,
                "[broken]extra]\n",
                "invalid section header",
            ),
            (
                MergeStrategy::Ini,
                "[options]\nmanaged=true\nmanaged=false\n",
                "conflicting values",
            ),
        ] {
            for existing in [false, true] {
                let root = tempfile::tempdir_in(".").unwrap();
                fs::create_dir(root.path().join("system")).unwrap();
                fs::write(root.path().join("system/fragment"), fragment).unwrap();
                let target = root.path().join("target");
                let original = "# Keep existing content\n";
                if existing {
                    fs::write(&target, original).unwrap();
                }
                let resource = fixture_resource(
                    entry(root.path(), &target, "fragment", merge),
                    Arc::new(MockExecutor::new()),
                );

                assert!(
                    matches!(
                        resource.current_state().unwrap(),
                        ResourceState::Invalid { reason: detail } if detail.contains(reason)
                    ),
                    "{merge:?}, existing={existing}"
                );
                let error = resource.apply().unwrap_err();
                assert!(error.to_string().contains(reason), "{error}");
                assert!(!root.path().join("staging-must-not-be-created").exists());
                if existing {
                    assert_eq!(fs::read_to_string(&target).unwrap(), original);
                } else {
                    assert!(!target.exists());
                }
            }
        }
    }

    #[test]
    fn system_file_dry_runs_preserve_missing_and_existing_targets() {
        use crate::engine::{ProcessOpts, process_resources};
        use crate::test_helpers::{ContextBuilder, task_batch};

        for (merge, fragment, current) in [
            (MergeStrategy::Toml, "managed = true\n", None),
            (
                MergeStrategy::Toml,
                "managed = true\n",
                Some("managed = false\nunmanaged = 'keep'\n"),
            ),
            (MergeStrategy::Ini, "[options]\nmanaged=true\n", None),
            (
                MergeStrategy::Ini,
                "[options]\nmanaged=true\n",
                Some("[options]\nmanaged=false\n# retain comment\nunmanaged=keep\n"),
            ),
            (
                MergeStrategy::Pam,
                "auth optional keyring.so\n",
                Some("# retain comment\nauth include system-auth\n"),
            ),
        ] {
            let root = tempfile::tempdir_in(".").unwrap();
            fs::create_dir(root.path().join("system")).unwrap();
            fs::write(root.path().join("system/fragment"), fragment).unwrap();
            let parent = root.path().join("target-parent");
            let target = parent.join("target");
            if let Some(current) = current {
                fs::create_dir(&parent).unwrap();
                fs::write(&target, current).unwrap();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt as _;
                    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
                }
            }
            let original_metadata = fs::metadata(&target).ok();
            let staging = root.path().join("staging-must-not-be-created");
            let resource = fixture_resource(
                entry(root.path(), &target, "fragment", merge),
                Arc::new(MockExecutor::new()),
            );
            let initial_state = resource.current_state().unwrap();
            if current.is_some() {
                assert!(
                    matches!(
                        initial_state,
                        ResourceState::Incorrect { current: difference }
                            if difference.contains("managed content differs")
                    ),
                    "{merge:?}"
                );
            } else {
                assert_eq!(initial_state, ResourceState::Missing);
            }
            let ctx = ContextBuilder::new(root.path().to_path_buf())
                .overlay(None)
                .build()
                .with_dry_run(true);

            let result =
                process_resources(&ctx, [resource], &ProcessOpts::strict("configure")).unwrap();

            let stats = task_batch(&result);
            assert_eq!(stats.changed_count(), 1, "{merge:?}, current={current:?}");
            assert_eq!(stats.already_ok_count(), 0);
            assert_eq!(stats.failed_count(), 0);
            assert_eq!(stats.skipped_count(), 0);
            if let Some(current) = current {
                assert_eq!(fs::read_to_string(&target).unwrap(), current);
                let before = original_metadata.unwrap();
                let after = fs::metadata(&target).unwrap();
                assert_eq!(after.permissions(), before.permissions());
                assert_eq!(after.modified().unwrap(), before.modified().unwrap());
                assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
            } else {
                assert!(!target.exists());
                assert!(!parent.exists(), "preview must not create target parents");
            }
            assert!(!staging.exists());
            assert_eq!(
                fs::read_to_string(root.path().join("system/fragment")).unwrap(),
                fragment
            );
        }
    }

    #[test]
    fn nonregular_system_file_targets_are_never_followed_or_replaced() {
        for kind in ["directory", "file link", "directory link", "dangling link"] {
            let fixture = tempfile::tempdir_in(".").unwrap();
            let root = fixture.path().canonicalize().unwrap();
            fs::create_dir(root.join("system")).unwrap();
            fs::write(root.join("system/fragment"), "managed = true\n").unwrap();
            let target = root.join("target");
            let user = root.join("user");
            fs::create_dir(&user).unwrap();
            let user_file = user.join("content");
            fs::write(&user_file, "user content").unwrap();
            match kind {
                "directory" => {
                    fs::create_dir(&target).unwrap();
                    fs::write(target.join("content"), "target content").unwrap();
                }
                "file link" => {
                    crate::infra::fs::create_native_symlink(&user_file, &target, false).unwrap();
                }
                "directory link" => {
                    crate::infra::fs::create_native_symlink(&user, &target, true).unwrap();
                }
                "dangling link" => {
                    crate::infra::fs::create_native_symlink(&root.join("absent"), &target, false)
                        .unwrap();
                }
                _ => panic!("unexpected target kind: {kind}"),
            }
            let original_link = fs::read_link(&target).ok();
            let resource = fixture_resource(
                entry(&root, &target, "fragment", MergeStrategy::Toml),
                Arc::new(MockExecutor::new()),
            );

            assert!(
                matches!(
                    resource.current_state().unwrap(),
                    ResourceState::Invalid { reason } if reason.contains("not a regular file")
                ),
                "{kind}"
            );
            let error = resource.apply().unwrap_err();
            assert!(
                error.to_string().contains("not a regular file"),
                "{kind}: {error}"
            );
            assert_eq!(fs::read_link(&target).ok(), original_link, "{kind}");
            if kind == "directory" {
                assert!(target.symlink_metadata().unwrap().is_dir());
                assert_eq!(
                    fs::read_to_string(target.join("content")).unwrap(),
                    "target content"
                );
            } else {
                assert!(target.symlink_metadata().unwrap().is_symlink(), "{kind}");
            }
            assert_eq!(fs::read_to_string(&user_file).unwrap(), "user content");
            assert!(!root.join("staging-must-not-be-created").exists());
            assert_eq!(fs::read_dir(&root).unwrap().count(), 3);
        }
    }

    #[test]
    fn system_file_discovery_errors_never_become_missing_or_mutate_files() {
        for failure in [
            "missing fragment",
            "fragment encoding",
            "target encoding",
            "target metadata",
        ] {
            let root = tempfile::tempdir_in(".").unwrap();
            fs::create_dir(root.path().join("system")).unwrap();
            let source = root.path().join("system").join("fragment");
            let target = root.path().join("target");
            let fragment = if failure == "fragment encoding" {
                &b"\xff"[..]
            } else {
                &b"managed = true\n"[..]
            };
            if failure != "missing fragment" {
                fs::write(&source, fragment).unwrap();
            }
            let current = if failure == "target encoding" {
                &b"\xff"[..]
            } else {
                &b"unmanaged = 'keep'\n"[..]
            };
            fs::write(&target, current).unwrap();
            let resource_target = if failure == "target metadata" {
                root.path().join("invalid\0target")
            } else {
                target.clone()
            };
            let resource = fixture_resource(
                entry(
                    root.path(),
                    &resource_target,
                    "fragment",
                    MergeStrategy::Toml,
                ),
                Arc::new(MockExecutor::new()),
            );

            for error in [
                resource.current_state().unwrap_err(),
                resource.apply().unwrap_err(),
            ] {
                let message = format!("{error:#}");
                let expected_path = if failure.contains("fragment") {
                    source.clone()
                } else {
                    crate::infra::fs::resolve_target_entry(&resource_target).unwrap()
                };
                assert!(
                    message.contains(&format!("reading {}", expected_path.display()))
                        || message
                            .contains(&format!("reading metadata for {}", expected_path.display())),
                    "{failure}: {message}"
                );
            }
            assert_eq!(fs::read(&target).unwrap(), current, "{failure}");
            if failure == "missing fragment" {
                assert!(!source.exists());
            } else {
                assert_eq!(fs::read(&source).unwrap(), fragment, "{failure}");
            }
            assert!(!root.path().join("staging-must-not-be-created").exists());
            assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
        }
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
                "[options]\n# heading\nkeep=yes\nmanaged=new\n# trailing\n\n",
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
                "bare settings keep their positions",
                "[options]\n# managed=comment\nBareFlag\n; trailing\n",
                "[options]\n# managed=comment\nBareFlag\nmanaged=new\n; trailing\n",
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
        let root = tempfile::tempdir_in(".").unwrap();
        fs::create_dir(root.path().join("system")).unwrap();
        fs::write(root.path().join("system/fragment.toml"), "managed = true\n").unwrap();
        let target = root.path().join("target.toml");
        fs::write(&target, "not = [valid\n").unwrap();
        let resource = fixture_resource(
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

    #[cfg(target_os = "linux")]
    const PREPARE_FIXTURE: &str = r#"#!/bin/sh
set -eu
test "$1" = system-file-helper && test "$2" = --target
target=$3
test "$4" = --fragment
case "$6" in --fragment-hash=*) ;; *) exit 1 ;; esac
test "$7" = --merge && test "$8" = toml && test "$9" = --stage
stage=${10}
test "$(stat -c %a -- "$stage")" = 700
printf 'replacement content\n' > "$stage/input"
if [ -f "$target" ]; then cp -- "$target" "$stage/original"; fi
printf 'prepared\n'
"#;

    #[cfg(target_os = "linux")]
    const INSTALL_FIXTURE: &str = r#"#!/bin/sh
set -eu
test "$1" = -D && test "$2" = --owner=root
test "$3" = --group=root && test "$4" = --mode=0644
test "$5" = --
shift 5
test "$(stat -c %a -- "$(dirname -- "$2")")" = 700
if [ "$FAILURE" = copy ]; then
    printf 'partial' > "$2"
    exit 1
fi
if [ "$FAILURE" = conflict ]; then
    printf 'intervening edit\n' > "$TARGET"
fi
# Exercise the real copy without requiring root in the fixture.
exec "$REAL_INSTALL" --mode=0644 -- "$1" "$2"
"#;

    #[cfg(target_os = "linux")]
    const PUBLISH_FIXTURE: &str = r#"#!/bin/sh
set -eu
if [ "$FAILURE" = publish ]; then
    exit 1
fi
case "${0##*/}" in
    mv) exec "$REAL_MV" "$@" ;;
    ln) exec "$REAL_LN" "$@" ;;
esac
"#;

    #[test]
    #[cfg(target_os = "linux")]
    #[allow(
        clippy::too_many_lines,
        reason = "shared setup for publication and preservation cases"
    )]
    fn atomic_install_preserves_destination_on_failures_and_intervening_edits() {
        use std::os::unix::fs::PermissionsExt as _;

        for existing in [false, true] {
            for failure in ["copy", "publish", "conflict", "chmod", "none"] {
                if !existing && failure == "chmod" {
                    continue;
                }
                let root = tempfile::tempdir_in(".").unwrap();
                let root_path = root.path().canonicalize().unwrap();
                let tools = root.path().join("tools");
                fs::create_dir(&tools).unwrap();
                // Quotes and shell syntax must be passed as data, even through sudo sh.
                let parent = root_path.join("parent ' $(false)");
                let target = parent.join("target ' $(false).toml");
                let helper = root_path.join("helper ' $(false)");
                fs::write(&helper, PREPARE_FIXTURE).unwrap();
                fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
                if existing {
                    fs::create_dir(&parent).unwrap();
                    fs::write(&target, "original content\n").unwrap();
                    rustix::fs::setxattr(
                        &target,
                        "user.dotfiles-fixture",
                        b"preserve me",
                        rustix::fs::XattrFlags::empty(),
                    )
                    .unwrap();
                    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
                }
                let original = fs::metadata(&target).ok();
                let install = tools.join("install");
                fs::write(&install, INSTALL_FIXTURE).unwrap();
                let cp = tools.join("cp");
                fs::write(
                    &cp,
                    r#"#!/bin/sh
set -eu
"$REAL_CP" "$@"
if [ "$1" = --attributes-only ] && [ "$FAILURE" = chmod ]; then chmod 0400 -- "$TARGET"; fi
"#,
                )
                .unwrap();
                fs::set_permissions(cp, fs::Permissions::from_mode(0o755)).unwrap();
                for tool in ["mv", "ln"] {
                    let publish = tools.join(tool);
                    fs::write(&publish, PUBLISH_FIXTURE).unwrap();
                    fs::set_permissions(publish, fs::Permissions::from_mode(0o755)).unwrap();
                }
                fs::set_permissions(install, fs::Permissions::from_mode(0o755)).unwrap();
                let path = std::env::join_paths(
                    std::iter::once(tools)
                        .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
                )
                .unwrap();
                let result = std::process::Command::new("sh")
                    .args(["-c", ATOMIC_INSTALL, "dotfiles-system-file"])
                    .arg(&helper)
                    .arg(&target)
                    .arg("public fragment")
                    .arg(&root_path)
                    .arg("toml")
                    .arg("fragment snapshot")
                    .env("PATH", path)
                    .env("FAILURE", failure)
                    .env("TARGET", &target)
                    .env("REAL_CP", which::which("cp").unwrap())
                    .env("REAL_INSTALL", which::which("install").unwrap())
                    .env("REAL_MV", which::which("mv").unwrap())
                    .env("REAL_LN", which::which("ln").unwrap())
                    .output()
                    .unwrap();

                assert_eq!(
                    result.status.success(),
                    failure == "none",
                    "existing={existing}, failure={failure}: {result:?}"
                );
                if failure == "none" {
                    assert_eq!(fs::read(&target).unwrap(), b"replacement content\n");
                    if existing {
                        let mut value = [0_u8; 32];
                        let count =
                            rustix::fs::getxattr(&target, "user.dotfiles-fixture", &mut value[..])
                                .unwrap();
                        assert_eq!(&value[..count], b"preserve me");
                    }
                    assert_eq!(
                        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                        if existing { 0o600 } else { 0o644 }
                    );
                } else if failure == "chmod" {
                    assert!(String::from_utf8_lossy(&result.stderr).contains("retry the update"));
                    assert_eq!(fs::read(&target).unwrap(), b"original content\n");
                    assert_eq!(
                        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                        0o400
                    );
                } else if failure == "conflict" {
                    assert_eq!(fs::read(&target).unwrap(), b"intervening edit\n");
                    if existing {
                        assert!(
                            String::from_utf8_lossy(&result.stderr).contains("retry the update")
                        );
                    }
                } else if let Some(original) = original {
                    assert_eq!(fs::read(&target).unwrap(), b"original content\n");
                    let after = fs::metadata(&target).unwrap();
                    assert_eq!(after.permissions().mode(), original.permissions().mode());
                    assert_eq!(after.modified().unwrap(), original.modified().unwrap());
                } else {
                    assert!(
                        !target.exists(),
                        "failed creation must leave the target absent"
                    );
                }
                assert!(
                    fs::read_dir(&parent).unwrap().all(|entry| {
                        !entry
                            .unwrap()
                            .file_name()
                            .to_string_lossy()
                            .starts_with(".dotfiles-system-file.")
                    }),
                    "replacement staging must be cleaned after {failure}"
                );
            }
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn system_file_publication_refuses_escaping_and_changed_parents() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let fixture_path = fixture.path().canonicalize().unwrap();
        let root = fixture_path.join("etc");
        let outside = fixture_path.join("outside");
        let actual = root.join("actual");
        for directory in [&root, &outside, &actual] {
            fs::create_dir(directory).unwrap();
        }
        let target = root.join("alias/subdir/config.toml");
        let alias = root.join("alias");
        std::os::unix::fs::symlink(&outside, &alias).unwrap();
        let resource = fixture_resource(
            entry(&root, &target, "fragment.toml", MergeStrategy::Toml),
            Arc::new(MockExecutor::new()),
        );
        assert!(
            resource
                .checked_target()
                .unwrap_err()
                .to_string()
                .contains("resolves outside")
        );
        fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&actual, &alias).unwrap();
        let resolved = resource.checked_target().unwrap();
        assert_eq!(resolved, actual.join("subdir/config.toml"));

        // Simulate a parent replaced after discovery. The helper must reject it
        // before it creates either the missing subdirectory or staging files.
        fs::remove_dir(&actual).unwrap();
        std::os::unix::fs::symlink(&outside, &actual).unwrap();
        let result = std::process::Command::new("sh")
            .args(["-c", ATOMIC_INSTALL, "dotfiles-system-file", "unused-input"])
            .arg(&resolved)
            .arg("")
            .arg(&root)
            .arg("toml")
            .arg("fragment snapshot")
            .output()
            .unwrap();
        assert!(!result.status.success(), "{result:?}");
        assert!(String::from_utf8_lossy(&result.stderr).contains("became a symlink"));
        assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
    }

    #[test]
    fn failed_install_preserves_target_without_user_staging_and_can_be_retried() {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt as _;

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
        let mut executor = MockExecutor::new();
        let mut sequence = mockall::Sequence::new();
        for fail in [true, false] {
            let expected_target = crate::infra::fs::resolve_target_entry(&target).unwrap();
            let expected_fragment =
                dunce::canonicalize(root.path().join("system/fragment.toml")).unwrap();
            let expected_staging = staging.clone();
            executor
                .expect_execute()
                .once()
                .in_sequence(&mut sequence)
                .returning(move |spec| {
                    assert_eq!(spec.program(), "sudo");
                    assert_eq!(spec.arguments().len(), 12);
                    assert_eq!(
                        &spec.arguments()[..6],
                        [
                            "-n",
                            "--",
                            "sh",
                            "-c",
                            ATOMIC_INSTALL,
                            "dotfiles-system-file"
                        ]
                    );
                    assert_eq!(spec.arguments()[7], expected_target.as_os_str());
                    assert_eq!(spec.arguments()[8], expected_fragment.as_os_str());
                    assert_eq!(spec.arguments()[10], "toml");
                    assert_eq!(
                        spec.arguments()[11],
                        crate::infra::fs::content_hash("[managed]\nenabled = true\n").as_str()
                    );
                    assert_eq!(
                        Path::new(&spec.arguments()[6]),
                        std::env::current_exe().unwrap()
                    );
                    assert_eq!(fs::read_dir(&expected_staging).unwrap().count(), 0);
                    assert_eq!(spec.working_dir(), None);
                    assert!(spec.is_checked());
                    if fail {
                        Err(ExecError::non_zero(
                            "sudo sh",
                            ExecResult::failure("", "fixture install denied", Some(1)),
                        ))
                    } else {
                        fs::write(
                            &expected_target,
                            "unmanaged = 'keep'\n[managed]\nenabled = true\n",
                        )
                        .unwrap();
                        #[cfg(unix)]
                        fs::set_permissions(&expected_target, fs::Permissions::from_mode(0o644))
                            .unwrap();
                        Ok(ExecResult::success("applied\n"))
                    }
                });
        }
        let resource = fixture_resource(
            entry(root.path(), &target, "fragment.toml", MergeStrategy::Toml),
            Arc::new(executor),
        );

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
        assert!(content.contains("unmanaged = 'keep'"));
        assert!(content.contains("enabled = true"));
    }
}
