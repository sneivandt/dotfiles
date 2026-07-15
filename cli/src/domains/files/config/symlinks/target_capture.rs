use anyhow::{Result, bail};

use super::path_segments;

pub(super) fn apply_target_captures(target: &str, captures: &[String]) -> Result<String> {
    let mut captures = captures.iter();
    let mut segments = Vec::new();
    for segment in path_segments(target) {
        if segment == "*" {
            let Some(capture) = captures.next() else {
                bail!("target pattern '{target}' has more '*' wildcards than the source pattern");
            };
            segments.push(capture.clone());
        } else {
            segments.push(segment);
        }
    }
    if captures.next().is_some() {
        bail!("target pattern '{target}' has fewer '*' wildcards than the source pattern");
    }
    Ok(segments.join("/"))
}
