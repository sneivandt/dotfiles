use anyhow::bail;

pub(super) fn ini_settings(
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

pub(super) fn contains_ini(current: &str, fragment: &str) -> anyhow::Result<bool> {
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
pub(super) fn merge_ini(current: &str, fragment: &str) -> anyhow::Result<String> {
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
pub(super) fn ini_line_key(line: &str) -> Option<&str> {
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

#[cfg(test)]
mod ini_section_tests {
    use super::ini_section;

    #[test]
    fn ini_section_requires_both_brackets() {
        assert_eq!(ini_section("[Section]"), Some("[Section]"));
        assert_eq!(ini_section("[unclosed"), None);
        assert_eq!(ini_section("key=val]"), None);
    }
}
