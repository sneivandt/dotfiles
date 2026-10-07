//! Source cron metadata for the local workflows recorded in APM's lockfile.

use std::collections::{BTreeMap, BTreeSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::Deserialize;
use serde_yaml_ng::Value;

use super::lockfile::COPILOT_APP_WORKFLOW_URI_PREFIX;

#[derive(Deserialize)]
struct WorkflowLock {
    #[serde(default)]
    dependencies: Vec<WorkflowDependency>,
}

#[derive(Deserialize)]
struct WorkflowDependency {
    local_path: Option<String>,
    #[serde(default)]
    deployed_files: Vec<String>,
}

/// Read source schedules only for workflows owned by the current global lock.
///
/// A missing cron field is recorded as `None` so removing a source schedule
/// also removes an old database schedule. Remote dependencies without a local
/// source path retain native APM's scheduling behavior.
pub(super) fn read_source_cron_schedules(
    home: &Path,
    ids: &[String],
) -> Result<BTreeMap<String, Option<String>>> {
    let lock_path = home.join(".apm").join("apm.lock.yaml");
    let content = match std::fs::read_to_string(&lock_path) {
        Ok(content) => content,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("reading {}", lock_path.display()));
        }
    };
    let lock: WorkflowLock =
        serde_yaml_ng::from_str(&content).context("reading APM workflow sources")?;
    let mut schedules = BTreeMap::new();
    for dependency in lock.dependencies {
        let managed: BTreeSet<&str> = dependency
            .deployed_files
            .iter()
            .filter_map(|file| file.strip_prefix(COPILOT_APP_WORKFLOW_URI_PREFIX))
            .filter(|id| ids.iter().any(|managed| managed == id))
            .collect();
        if managed.is_empty() {
            continue;
        }
        let Some(local_path) = dependency.local_path else {
            continue;
        };
        let root = local_source_path(home, &local_path)
            .join(".apm")
            .join("prompts");
        let mut prompts = BTreeMap::new();
        collect_prompts(&root, &mut prompts)?;
        for id in managed {
            let stem = id
                .splitn(4, "--")
                .nth(3)
                .with_context(|| format!("invalid managed workflow id {id}"))?;
            let path = prompts
                .get(stem)
                .with_context(|| format!("source prompt for managed workflow {id} is missing"))?;
            let cron = read_prompt_cron(path)?;
            if let Some(previous) = schedules.insert(id.to_owned(), cron.clone()) {
                anyhow::ensure!(
                    previous == cron,
                    "conflicting source schedules for managed workflow {id}"
                );
            }
        }
    }
    Ok(schedules)
}

fn local_source_path(home: &Path, source: &str) -> PathBuf {
    if source == "~" {
        return home.to_path_buf();
    }
    if let Some(relative) = source
        .strip_prefix("~/")
        .or_else(|| source.strip_prefix("~\\"))
    {
        return home.join(relative);
    }
    let path = Path::new(source);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        home.join(".apm").join(path)
    }
}

fn collect_prompts(root: &Path, prompts: &mut BTreeMap<String, PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(root)
        .with_context(|| format!("reading workflow prompt directory {}", root.display()))?
    {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect_prompts(&entry.path(), prompts)?;
        } else if kind.is_file()
            && let Some(stem) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.strip_suffix(".prompt.md"))
        {
            let slug = slugify(stem);
            anyhow::ensure!(
                prompts.insert(slug.clone(), entry.path()).is_none(),
                "ambiguous workflow prompt stem {slug} in {}",
                root.display()
            );
        }
    }
    Ok(())
}

fn slugify(stem: &str) -> String {
    let slug = stem
        .split(|character: char| {
            !character.is_ascii_alphanumeric() && !matches!(character, '-' | '_')
        })
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .to_ascii_lowercase();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "unknown".to_string()
    } else {
        slug.to_string()
    }
}

fn read_prompt_cron(path: &Path) -> Result<Option<String>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading workflow prompt {}", path.display()))?;
    let mut lines = content.lines();
    anyhow::ensure!(
        lines.next() == Some("---"),
        "workflow prompt {} has no frontmatter",
        path.display()
    );
    let mut frontmatter = String::new();
    let mut closed = false;
    for line in lines {
        if matches!(line, "---" | "...") {
            closed = true;
            break;
        }
        frontmatter.push_str(line);
        frontmatter.push('\n');
    }
    anyhow::ensure!(closed, "unclosed frontmatter in {}", path.display());
    let metadata: Value = serde_yaml_ng::from_str(&frontmatter)
        .with_context(|| format!("reading workflow frontmatter {}", path.display()))?;
    anyhow::ensure!(
        metadata.is_mapping(),
        "workflow frontmatter must be a mapping in {}",
        path.display()
    );
    match metadata.get("cron_expression") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(cron)) => {
            Ok(Some(cron.trim().to_string()).filter(|cron| !cron.is_empty()))
        }
        Some(_) => anyhow::bail!("cron_expression must be a string in {}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_sources(home: &Path, source: &str) -> PathBuf {
        let root = home.join(".apm").join("plugins").join("fixture");
        let prompts = root.join(".apm").join("prompts");
        std::fs::create_dir_all(prompts.join("nested")).unwrap();
        std::fs::write(
            prompts.join("nested").join("Morning Review.prompt.md"),
            source,
        )
        .unwrap();
        std::fs::write(
            prompts.join("manual.prompt.md"),
            "---\ninterval: manual\n---\nManual prompt\n",
        )
        .unwrap();
        std::fs::write(
            prompts.join("foreign.prompt.md"),
            "---\ncron_expression: 42\n---\nUnmanaged prompt\n",
        )
        .unwrap();
        std::fs::write(
            home.join(".apm").join("apm.lock.yaml"),
            "dependencies:\n- local_path: ~/.apm/plugins/fixture\n  deployed_files:\n  \
             - copilot-app-db://workflows/apm--_local--fixture--morning-review\n  \
             - copilot-app-db://workflows/apm--_local--fixture--manual\n  \
             - .agents/skills/foreign\n- repo_url: remote/package\n  deployed_files:\n  \
             - copilot-app-db://workflows/apm--remote--package--review\n",
        )
        .unwrap();
        prompts
    }

    #[test]
    fn source_cron_metadata_is_scoped_to_deployed_ids() {
        let dir = tempfile::tempdir().unwrap();
        let prompts = write_sources(
            dir.path(),
            "---\r\ninterval: manual\r\ncron_expression: \"15 7,11,15 * * 1-5\"\r\n---\r\nPrompt",
        );
        let ids = [
            "apm--_local--fixture--morning-review",
            "apm--_local--fixture--manual",
            "apm--remote--package--review",
            "apm--foreign--fixture--foreign",
        ]
        .map(str::to_string);
        let expected = BTreeMap::from([
            (ids[0].clone(), Some("15 7,11,15 * * 1-5".to_string())),
            (ids[1].clone(), None),
        ]);
        assert_eq!(
            read_source_cron_schedules(dir.path(), &ids).unwrap(),
            expected
        );

        std::fs::write(
            prompts.join("nested").join("Morning Review.prompt.md"),
            "---\ninterval: manual\n---\nPrompt",
        )
        .unwrap();
        assert_eq!(
            read_source_cron_schedules(dir.path(), &ids).unwrap()[&ids[0]],
            None,
            "removing cron from the source must be represented explicitly"
        );
        std::fs::rename(
            prompts.join("nested").join("Morning Review.prompt.md"),
            prompts.join("nested").join("Morning--Review.prompt.md"),
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".apm").join("apm.lock.yaml"),
            "dependencies:\n- local_path: ~/.apm/plugins/fixture\n  deployed_files:\n  \
             - copilot-app-db://workflows/apm--_local--fixture--morning--review\n",
        )
        .unwrap();
        let double_hyphen_id = "apm--_local--fixture--morning--review".to_string();
        assert_eq!(
            read_source_cron_schedules(dir.path(), std::slice::from_ref(&double_hyphen_id))
                .unwrap(),
            BTreeMap::from([(double_hyphen_id, None)]),
            "prompt stems may contain the namespace separator"
        );
    }

    #[test]
    fn source_cron_metadata_reports_missing_and_invalid_sources() {
        for (source, reason) in [
            ("no frontmatter", "has no frontmatter"),
            ("---\ncron_expression: foo", "unclosed frontmatter"),
            (
                "---\ncron_expression: [\n---",
                "reading workflow frontmatter",
            ),
            ("---\ncron_expression: 42\n---", "must be a string"),
            ("---\n42\n---", "must be a mapping"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let prompts = write_sources(dir.path(), source);
            let ids = ["apm--_local--fixture--morning-review".to_string()];
            let error = read_source_cron_schedules(dir.path(), &ids).unwrap_err();
            assert!(format!("{error:#}").contains(reason), "{error:#}");

            std::fs::remove_file(prompts.join("nested").join("Morning Review.prompt.md")).unwrap();
            let missing = read_source_cron_schedules(dir.path(), &ids).unwrap_err();
            assert!(missing.to_string().contains("is missing"), "{missing:#}");
        }
    }

    #[test]
    fn source_paths_and_apm_prompt_slugs_are_portable() {
        let home = Path::new("fixture-home");
        for source in ["~/.apm/plugins/fixture", "~\\.apm\\plugins\\fixture"] {
            assert_eq!(
                local_source_path(home, source),
                home.join(source.get(2..).unwrap())
            );
        }
        assert_eq!(local_source_path(home, "~"), home);
        assert_eq!(
            local_source_path(home, "plugins/fixture"),
            home.join(".apm").join("plugins/fixture")
        );
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            local_source_path(home, dir.path().to_str().unwrap()),
            dir.path()
        );
        for (stem, expected) in [
            ("Morning Review", "morning-review"),
            ("Review-!Now", "review--now"),
            ("a__b", "a__b"),
            ("!!", "unknown"),
        ] {
            assert_eq!(slugify(stem), expected, "{stem}");
        }
    }
}
