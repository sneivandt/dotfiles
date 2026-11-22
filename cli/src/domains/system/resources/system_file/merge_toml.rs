use anyhow::Context as _;

pub(super) fn parse_toml(content: &str) -> anyhow::Result<toml::Table> {
    if content.trim().is_empty() {
        Ok(toml::Table::new())
    } else {
        toml::from_str(content).context("parsing TOML system file")
    }
}

#[cfg(any(unix, test))]
fn merge_table(
    current: &mut dyn toml_edit::TableLike,
    desired: &dyn toml_edit::TableLike,
    inline: bool,
) {
    for (key, desired_value) in desired.iter() {
        if let (Some((current_table, current_inline)), Some(desired_table)) = (
            current.get_mut(key).and_then(|item| {
                let is_inline = item.is_inline_table();
                item.as_table_like_mut().map(|table| (table, is_inline))
            }),
            desired_value.as_table_like(),
        ) {
            merge_table(current_table, desired_table, current_inline);
        } else {
            let mut replacement = desired_value.clone();
            // Direct assignment bypasses InlineTable::insert's value conversion.
            if inline {
                replacement.make_value();
            }
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

fn toml_table_matches(
    current: &toml::Table,
    desired: &toml::Table,
    allow_extra_keys: bool,
) -> bool {
    (allow_extra_keys || current.len() == desired.len())
        && desired.iter().all(|(key, desired_value)| {
            current
                .get(key)
                .is_some_and(|value| toml_value_matches(value, desired_value, allow_extra_keys))
        })
}

fn toml_value_matches(
    current: &toml::Value,
    desired: &toml::Value,
    allow_extra_keys: bool,
) -> bool {
    match (current, desired) {
        (toml::Value::Table(current), toml::Value::Table(desired)) => {
            toml_table_matches(current, desired, allow_extra_keys)
        }
        (toml::Value::Array(current), toml::Value::Array(desired)) => {
            // A fragment owns the whole array, including tables inside it.
            current.len() == desired.len()
                && current
                    .iter()
                    .zip(desired)
                    .all(|(current, desired)| toml_value_matches(current, desired, false))
        }
        (toml::Value::Float(current), toml::Value::Float(desired))
            if current.is_nan() && desired.is_nan() =>
        {
            // TOML distinguishes signs even though NaN cannot compare equal.
            current.is_sign_negative() == desired.is_sign_negative()
        }
        _ => current == desired,
    }
}

pub(super) fn contains_toml(current: &str, fragment: &str) -> anyhow::Result<bool> {
    Ok(toml_table_matches(
        &parse_toml(current)?,
        &parse_toml(fragment)?,
        true,
    ))
}

#[cfg(any(unix, test))]
pub(super) fn merge_toml(current: &str, fragment: &str) -> anyhow::Result<String> {
    let mut current = current
        .parse::<toml_edit::DocumentMut>()
        .context("parsing TOML system file")?;
    let fragment = fragment
        .parse::<toml_edit::DocumentMut>()
        .context("parsing TOML system file")?;
    merge_table(current.as_table_mut(), fragment.as_table(), false);
    let mut rendered = current.to_string();
    if !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    Ok(rendered)
}
