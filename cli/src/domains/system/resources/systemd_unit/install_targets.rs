use super::{ResourceError, ResourceResult, safe_unit_name};

pub(super) fn install_targets(content: &str) -> ResourceResult<Vec<String>> {
    let mut in_install = false;
    let mut targets: Vec<String> = Vec::new();
    let mut lines = content.lines();
    while let Some(raw_line) = lines.next() {
        let mut line = raw_line.to_string();
        if line.trim().is_empty() || line.trim_start().starts_with(['#', ';']) {
            continue;
        }
        // Comments are skipped during continuation, but blank lines end it.
        while line.bytes().rev().take_while(|byte| *byte == b'\\').count() % 2 == 1 {
            line.pop();
            line.push(' ');
            let Some(next) = lines.find(|next| !next.trim_start().starts_with(['#', ';'])) else {
                break;
            };
            line.push_str(next);
        }
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_install = line == "[Install]";
            continue;
        }
        if !in_install {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let suffix = match key.trim() {
            "WantedBy" => ".wants",
            "RequiredBy" => ".requires",
            _ if value.trim().is_empty() => continue,
            key => {
                return Err(ResourceError::not_supported(format!(
                    "offline enablement cannot interpret [Install] {key}; use a live user systemd manager"
                )));
            }
        };
        if value.trim().is_empty() {
            targets.retain(|target| !target.ends_with(suffix));
            continue;
        }
        for target in value.split_whitespace() {
            if !safe_unit_name(target) {
                return Err(ResourceError::not_supported(format!(
                    "invalid [Install] target {target:?}"
                )));
            }
            targets.push(format!("{target}{suffix}"));
        }
    }
    Ok(targets)
}

pub(super) fn has_install_directives(content: &str) -> bool {
    let mut in_install = false;
    content.lines().any(|line| {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_install = line == "[Install]";
        }
        in_install && !line.starts_with(['#', ';']) && line.contains('=')
    })
}
