use anyhow::{Context as _, bail};

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

pub(super) fn pam_rule_identity(line: &str) -> anyhow::Result<(&str, &str)> {
    let (facility, fields) = line
        .split_once(char::is_whitespace)
        .context("PAM fragment rule has no control field")?;
    let (control, module_fields) = pam_control_and_module(fields)
        .context("PAM fragment rule has an unterminated control field")?;
    if control.eq_ignore_ascii_case("include") || control.eq_ignore_ascii_case("substack") {
        bail!("PAM fragment rule includes a stack rather than a module: {line}");
    }
    #[expect(
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
pub(super) fn same_pam_rule(left: &str, right: &str) -> bool {
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

pub(super) fn merge_pam(current: &str, fragment: &str) -> anyhow::Result<String> {
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

pub(super) fn validate_pam_rules(fragment: &str) -> anyhow::Result<()> {
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
