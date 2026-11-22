use super::AgentSettingResource;
use anyhow::{Context as _, Result};
use serde_json::Value;

pub(super) fn set_editable_toml(
    document: &mut toml_edit::DocumentMut,
    setting: &AgentSettingResource,
) -> Result<()> {
    let segments: Vec<_> = setting.key.split('.').collect();
    let (last, parents) = segments.split_last().context("empty settings path")?;
    let mut table: &mut dyn toml_edit::TableLike = document.as_table_mut();
    let mut inline_parent = false;
    for segment in parents {
        if !table.contains_key(segment) {
            table.insert(segment, toml_edit::Item::Table(toml_edit::Table::new()));
        }
        table = table
            .get_mut(segment)
            .and_then(|item| {
                inline_parent = item.is_inline_table();
                item.as_table_like_mut()
            })
            .with_context(|| {
                format!(
                    "cannot set '{}': '{segment}' is not a TOML table",
                    setting.key
                )
            })?;
    }
    let serialized = toml::to_string(&toml::Table::from_iter([(
        "value".into(),
        setting.desired_value.clone(),
    )]))?;
    let mut value = serialized
        .parse::<toml_edit::DocumentMut>()?
        .remove("value")
        .context("serialized setting missing")?;
    // Replacing an entry directly bypasses InlineTable's conversion on insert.
    if inline_parent {
        value.make_value();
    }
    if let (Some(old), Some(new)) = (
        table.get(last).and_then(toml_edit::Item::as_value),
        value.as_value_mut(),
    ) {
        *new.decor_mut() = old.decor().clone();
    }
    if let Some(item) = table.get_mut(last) {
        *item = value;
    } else {
        table.insert(last, value);
    }
    Ok(())
}

pub(super) fn toml_value_to_json(value: &toml::Value) -> Value {
    match value {
        toml::Value::String(value) => Value::String(value.clone()),
        toml::Value::Integer(value) => Value::Number((*value).into()),
        toml::Value::Float(value) => {
            serde_json::Number::from_f64(*value).map_or(Value::Null, Value::Number)
        }
        toml::Value::Boolean(value) => Value::Bool(*value),
        toml::Value::Datetime(value) => Value::String(value.to_string()),
        toml::Value::Array(values) => Value::Array(values.iter().map(toml_value_to_json).collect()),
        toml::Value::Table(table) => Value::Object(
            table
                .iter()
                .map(|(key, nested)| (key.clone(), toml_value_to_json(nested)))
                .collect(),
        ),
    }
}
