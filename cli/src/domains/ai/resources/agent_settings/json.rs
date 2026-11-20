//! Edit only objects on the managed path. Unmanaged values must not make a
//! round trip through `serde_json::Number`, which can approximate them as f64.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use anyhow::{Context as _, Result};
use serde::de::IgnoredAny;
use serde_json::{Deserializer, Map, Value};

use super::{AgentSettingResource, toml_value_to_json};

/// Read immediate members of a validated object. Let the JSON parser find
/// each value's end (including escaped strings and nested containers).
fn members(object: &str) -> Result<BTreeMap<String, &str>> {
    let mut fields = BTreeMap::new();
    let mut rest = object
        .strip_prefix('{')
        .context("expected JSON object")?
        .trim_start();
    while !rest.starts_with('}') {
        let mut keys = Deserializer::from_str(rest).into_iter::<String>();
        let key = keys.next().context("missing JSON member name")??;
        rest = rest
            .split_at(keys.byte_offset())
            .1
            .trim_start()
            .strip_prefix(':')
            .context("missing JSON member separator")?
            .trim_start();
        let mut values = Deserializer::from_str(rest).into_iter::<IgnoredAny>();
        let _value = values.next().context("missing JSON member value")??;
        let (raw, tail) = rest.split_at(values.byte_offset());
        fields.insert(key, raw);
        let Some(next) = tail.trim_start().strip_prefix(',') else {
            break;
        };
        rest = next.trim_start();
    }
    Ok(fields)
}

fn edit(contents: &str, setting: &AgentSettingResource) -> Result<Option<String>> {
    let desired = toml_value_to_json(&setting.desired_value);
    let mut parents = Vec::new();
    let mut object = contents;
    let mut segments = setting.key.split('.').peekable();
    while let Some(key) = segments.next() {
        let fields = members(object)?;
        let current = fields.get(key).copied();
        parents.push((fields, key));
        if segments.peek().is_none() {
            if current
                .map(serde_json::from_str::<Value>)
                .transpose()?
                .as_ref()
                == Some(&desired)
            {
                return Ok(None);
            }
        } else {
            object = current.unwrap_or("{}");
            anyhow::ensure!(
                object.starts_with('{'),
                "cannot set '{}': '{key}' is not a JSON object",
                setting.key
            );
        }
    }

    let mut indent = "  ".repeat(parents.len());
    let mut updated = serde_json::to_string_pretty(&desired)?.replace('\n', &format!("\n{indent}"));
    while let Some((mut fields, key)) = parents.pop() {
        fields.insert(key.to_owned(), &updated);
        indent.truncate(parents.len() * 2);
        let mut rendered = String::from("{");
        for (index, (name, raw)) in fields.into_iter().enumerate() {
            if index != 0 {
                rendered.push(',');
            }
            write!(
                rendered,
                "\n{indent}  {}: {raw}",
                serde_json::to_string(&name)?
            )?;
        }
        write!(rendered, "\n{indent}}}")?;
        updated = rendered;
    }
    Ok(Some(updated))
}

pub(super) fn plan(contents: &str, settings: &[AgentSettingResource]) -> Result<Option<String>> {
    let contents = if contents.trim().is_empty() {
        "{}"
    } else {
        // Keep the existing whole-document validation, including the root type.
        serde_json::from_str::<Map<String, Value>>(contents)?;
        contents.trim()
    };
    let mut updated = None;
    for setting in settings {
        if let Some(change) = edit(updated.as_deref().unwrap_or(contents), setting)? {
            updated = Some(change);
        }
    }
    Ok(updated)
}
