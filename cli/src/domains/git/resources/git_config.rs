//! Git configuration resource.
use anyhow::{Context as _, Result};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::domains::git::config::git_config::target_key;

/// Stage the complete user configuration privately before editing it with libgit2.
fn stage_config(
    parent: &Path,
    contents: impl AsRef<[u8]>,
) -> Result<(crate::infra::fs::PrivateTempDir, PathBuf)> {
    // libgit2 also creates its own lock file during edits. Keep every such
    // intermediate copy inside a directory private from its first instant.
    let directory = crate::infra::fs::PrivateTempDir::create(parent, ".dotfiles-git")
        .context("creating private Git config staging directory")?;
    let (mut stage, mut file) = directory
        .create_file("config")
        .context("creating private staged Git config")?;
    file.write_all(contents.as_ref())?;
    let path = stage.path().to_path_buf();
    stage.persist();
    Ok((directory, path))
}

fn open_file(path: &Path) -> Result<git2::Config> {
    require_regular_config(path)?;
    // libgit2's relative-include resolver recognizes '/' separators, including
    // on Windows. Native backslashes can otherwise make an include appear absent.
    #[cfg(windows)]
    {
        let normalized = path
            .to_str()
            .context("Git config path is not valid UTF-8")?
            .replace('\\', "/");
        Ok(git2::Config::open(Path::new(&normalized))?)
    }
    #[cfg(not(windows))]
    Ok(git2::Config::open(path)?)
}

fn require_regular_config(path: &Path) -> Result<()> {
    match std::fs::metadata(path) {
        Ok(metadata) => anyhow::ensure!(
            metadata.is_file(),
            "Git config is not a regular file: {}",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("inspecting Git config {}", path.display()));
        }
    }
    Ok(())
}

fn is_include(key: &str) -> bool {
    key.rsplit_once('.').is_some_and(|(section, name)| {
        name == "path" && (section == "include" || section.starts_with("includeif."))
    })
}

use crate::engine::{
    AccessClaim, IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState,
};

/// A git config entry resource that can be checked and applied.
///
/// Uses the `git2` crate to read and write global git configuration natively,
/// without shelling out to `git config`.
#[derive(Debug, Clone)]
pub struct GitConfigResource {
    /// Config key (e.g., "core.autocrlf").
    pub key: String,
    desired: Option<String>,
    config_path: Option<PathBuf>,
}

impl GitConfigResource {
    /// Create a new git config resource.
    #[must_use]
    pub const fn new(key: String, desired_value: String) -> Self {
        Self {
            key,
            desired: Some(desired_value),
            config_path: None,
        }
    }

    /// Create a resource that removes a global Git setting.
    #[must_use]
    pub const fn absent(key: String) -> Self {
        Self {
            key,
            desired: None,
            config_path: None,
        }
    }

    /// Create a resource backed by one explicit config file.
    #[must_use]
    pub const fn with_config_path(
        key: String,
        desired_value: String,
        config_path: PathBuf,
    ) -> Self {
        Self {
            key,
            desired: Some(desired_value),
            config_path: Some(config_path),
        }
    }

    /// Scope either setting or removal to an explicit file.
    #[must_use]
    #[cfg(test)]
    fn using_config_path(mut self, path: PathBuf) -> Self {
        self.config_path = Some(path);
        self
    }

    fn open_config(&self) -> Result<git2::Config> {
        self.config_path.as_deref().map_or_else(
            || {
                let config = git2::Config::open_default().context("opening git config")?;
                config
                    .open_level(git2::ConfigLevel::Global)
                    .context("opening global git config")
            },
            |path| {
                open_file(path).with_context(|| format!("opening git config {}", path.display()))
            },
        )
    }

    fn values_from_config(&self, config: &git2::Config) -> Result<Vec<Option<String>>> {
        (|| -> Result<Vec<Option<String>>> {
            let mut entries = config.multivar(&self.key, None)?;
            let mut values = Vec::new();
            while let Some(entry) = entries.next() {
                let entry = entry?;
                if entry.include_depth() != 0 {
                    continue;
                }
                values.push(if entry.has_value() {
                    Some(entry.value()?.to_string())
                } else {
                    None
                });
            }
            Ok(values)
        })()
        .with_context(|| format!("reading git config {}", self.key))
    }

    /// Check resource state against a pre-opened config snapshot.
    ///
    /// This enables unit testing without touching the real global git config.
    fn state_from_config(&self, config: &git2::Config) -> ResourceResult<ResourceState> {
        let values = self.values_from_config(config)?;
        if values.is_empty() {
            return Ok(if self.desired.is_some() {
                ResourceState::Missing
            } else {
                ResourceState::Correct
            });
        }
        let after_includes = is_include(&target_key(&self.key)) || self.include_order(config)?.1;
        Ok(
            if after_includes
                && self.desired.as_ref().is_some_and(|desired| {
                    values
                        .iter()
                        .all(|value| value.as_deref() == Some(desired.as_str()))
                })
            {
                ResourceState::Correct
            } else {
                ResourceState::Incorrect {
                    current: values
                        .iter()
                        .map(|value| value.as_deref().unwrap_or("<implicit true>"))
                        .collect::<Vec<_>>()
                        .join(", "),
                }
            },
        )
    }

    fn entry_count_across_includes(&self, config: &git2::Config) -> Result<usize> {
        let mut count = 0_usize;
        let mut entries = config.multivar(&self.key, None)?;
        while let Some(entry) = entries.next() {
            entry?;
            count = count.saturating_add(1);
        }
        Ok(count)
    }

    /// Apply config change to a mutable config handle.
    ///
    /// This enables unit testing without touching the real global git config.
    fn apply_to_config(&self, config: &mut git2::Config) -> Result<ResourceChange> {
        let values = self.values_from_config(config)?;
        let total_entries = self.entry_count_across_includes(config)?;
        // libgit2's single-value `set_str`/`remove` checks uniqueness across
        // expanded includes, so any included entry for the same key requires
        // multivar mutation (which edits only the depth-0 file in place).
        let needs_multivar = values.len() > 1 || total_entries > values.len();
        // libgit2's regex mutation can dereference a null implicit value.
        if needs_multivar && values.iter().any(Option::is_none) {
            anyhow::bail!(
                "cannot safely replace repeated implicit values for git config {}; give each entry an explicit value first",
                self.key
            );
        }
        if let Some(desired) = &self.desired {
            if needs_multivar {
                let escaped = escape_multivar_value(desired);
                config.set_multivar(&self.key, ".*", &escaped)
            } else {
                config.set_str(&self.key, desired)
            }
            .with_context(|| format!("setting {} = {desired}", self.key))?;
        } else {
            if values.is_empty() {
                return Ok(ResourceChange::AlreadyCorrect);
            }
            if needs_multivar {
                config.remove_multivar(&self.key, ".*")
            } else {
                config.remove(&self.key)
            }
            .with_context(|| format!("removing git config {}", self.key))?;
        }
        Ok(ResourceChange::Applied)
    }

    // A conditional include can be active in Git even when opening the global
    // file by itself leaves it inactive. Treat the directive as an ordering
    // boundary regardless of which included values libgit2 can see here.
    fn include_order(&self, config: &git2::Config) -> Result<(bool, bool)> {
        let managed = target_key(&self.key);
        let mut entries = config.entries(None)?;
        let mut includes = false;
        let mut after_includes = true;
        while let Some(entry) = entries.next() {
            let entry = entry?;
            if entry.include_depth() != 0 {
                continue;
            }
            let name = entry.name()?;
            if name == managed {
                after_includes = true;
            } else if is_include(name) {
                includes = true;
                after_includes = false;
            }
        }
        Ok((includes, after_includes))
    }

    fn apply_transaction(&self) -> Result<ResourceChange> {
        let configured = self
            .config_path
            .clone()
            .map_or_else(git2::Config::find_global, Ok)?;
        let settings = std::slice::from_ref(self);
        let plan = GitConfigPlan::inspect(&configured, settings)?;
        plan.apply(&configured)
    }
}

pub(crate) fn config_access_claim() -> AccessClaim {
    AccessClaim::exclusive("git-config", "configuration")
}

fn resolve_config(path: &Path) -> Result<PathBuf> {
    if crate::infra::fs::symlink_metadata_optional(path, "inspect Git config")?.is_some() {
        crate::infra::fs::canonicalize(path)
    } else {
        crate::infra::fs::resolve_target_entry(path)
    }
}

pub(crate) struct GitConfigPlan {
    path: PathBuf,
    before: Option<Vec<u8>>,
    changed: Vec<GitConfigResource>,
}

impl GitConfigPlan {
    pub(crate) fn inspect(configured: &Path, settings: &[GitConfigResource]) -> Result<Self> {
        let path = resolve_config(configured)?;
        let before = read_config_bytes(&path)?;
        let config = open_file(&path)?;
        let mut changed = Vec::new();
        let mut include_changes = Vec::new();
        let mut unchanged_values = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for setting in settings {
            let key = target_key(&setting.key);
            let include = is_include(&key);
            if !seen.insert(key) {
                continue;
            }
            if setting.state_from_config(&config)? != ResourceState::Correct {
                if include {
                    include_changes.push(setting);
                } else {
                    changed.push(setting);
                }
            } else if !include && setting.desired.is_some() {
                unchanged_values.push(setting);
            }
        }
        if !include_changes.is_empty() {
            // A managed include can change precedence for the other keys.
            // Stage include edits first and ensure managed values follow them.
            include_changes.extend(changed);
            include_changes.extend(unchanged_values);
            changed = include_changes;
        }
        let plan = Self {
            path,
            before,
            changed: changed.into_iter().cloned().collect(),
        };
        plan.ensure_current(configured)?;
        Ok(plan)
    }

    pub(crate) fn changed(&self) -> &[GitConfigResource] {
        &self.changed
    }

    fn ensure_current(&self, configured: &Path) -> Result<()> {
        anyhow::ensure!(
            resolve_config(configured)? == self.path
                && read_config_bytes(&self.path)? == self.before,
            "global Git config changed during update; retry"
        );
        Ok(())
    }

    pub(crate) fn apply(&self, configured: &Path) -> Result<ResourceChange> {
        if self.changed.is_empty() {
            return Ok(ResourceChange::AlreadyCorrect);
        }
        let path = &self.path;
        self.ensure_current(configured)?;
        crate::infra::fs::ensure_parent_dir(path)?;
        // Git uses exclusive creation of this path, not an advisory file lock.
        // This is only an empty reservation. No config bytes go into this lock.
        let mut lock_path = path.as_os_str().to_os_string();
        lock_path.push(".lock");
        let lock_path = PathBuf::from(lock_path);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let lock_file = options.open(&lock_path)
            .with_context(|| format!("acquiring Git config lock {}; wait for the other Git writer or inspect a stale lock before retrying", lock_path.display()))?;
        let _lock = crate::infra::fs::TempGuard::file(lock_path);
        drop(lock_file);
        self.ensure_current(configured)?;
        let config = open_file(path)?;
        let parent = path.parent().context("global Git config has no parent")?;
        let (stage, stage_path) = stage_config(parent, self.before.as_deref().unwrap_or_default())?;
        let stanza_path = stage_path.with_file_name("stanza");
        for setting in &self.changed {
            stage_setting(setting, &stage_path, &stanza_path, &config)
                .with_context(|| format!("staging git config {}", setting.key))?;
        }
        let replacement = std::fs::read(&stage_path)?;
        crate::infra::fs::write_atomic_checked(path, replacement, || {
            self.ensure_current(configured)
        })?;
        drop(stage);
        Ok(ResourceChange::Applied)
    }
}

fn stage_setting(
    setting: &GitConfigResource,
    stage_path: &Path,
    stanza_path: &Path,
    original: &git2::Config,
) -> Result<()> {
    let mut staged = open_file(stage_path)?;
    if is_include(&target_key(&setting.key)) || !setting.include_order(&staged)?.0 {
        setting.apply_to_config(&mut staged)?;
        return Ok(());
    }

    let values = setting.values_from_config(original)?;
    anyhow::ensure!(
        values.iter().all(Option::is_some),
        "cannot safely replace repeated implicit values for git config {}; give each entry an explicit value first",
        setting.key
    );
    if !values.is_empty() {
        staged.remove_multivar(&setting.key, ".*")?;
    }
    drop(staged);
    if let Some(desired) = &setting.desired {
        // Let libgit2 validate and quote the value before appending its stanza.
        std::fs::write(stanza_path, [])?;
        let mut stanza = open_file(stanza_path)?;
        stanza.set_str(&setting.key, desired)?;
        drop(stanza);
        let mut stage = std::fs::OpenOptions::new().append(true).open(stage_path)?;
        stage.write_all(b"\n")?;
        stage.write_all(&std::fs::read(stanza_path)?)?;
    }
    Ok(())
}

impl Resource for GitConfigResource {
    fn access_claims(&self) -> Vec<AccessClaim> {
        // Global configuration may include multiple files; keep one conservative
        // domain-wide key, including explicit paths that could alias those files.
        vec![config_access_claim()]
    }

    fn description(&self) -> String {
        self.desired.as_ref().map_or_else(
            || format!("{} is unset", self.key),
            |desired| format!("{} = {desired}", self.key),
        )
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        Ok(self
            .apply_transaction()
            .with_context(|| format!("setting git config {}", self.key))?)
    }
}

fn escape_multivar_value(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\t' => escaped.push_str("\\t"),
            '\x08' => escaped.push_str("\\b"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn read_config_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    require_regular_config(path)?;
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading Git config {}", path.display())),
    }
}

impl IntrinsicState for GitConfigResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        let config = self
            .open_config()
            .with_context(|| format!("reading git config {}", self.key))?;
        self.state_from_config(&config)
    }
}

#[cfg(test)]
#[path = "tests/git_config.rs"]
mod tests;
