//! YAML boundary for APM documents with string-keyed mappings.
//!
//! Keep numeric-looking identifiers as strings, reject ambiguous duplicate keys
//! and unsupported tags, and preserve literal merge fields for native APM.

use anyhow::{Context as _, Result};
use serde::de::DeserializeOwned;
pub(crate) use serde_json::Value;
use std::borrow::Cow;
pub(crate) type Mapping = serde_json::Map<String, Value>;

pub(crate) fn from_str<T: DeserializeOwned>(text: &str) -> Result<T> {
    let compatible = preserve_numeric_strings(text)?;
    Ok(serde_saphyr::from_str_with_options(
        &compatible,
        serde_saphyr::options! {
            strict_booleans: true,
            merge_keys: serde_saphyr::MergeKeyPolicy::AsOrdinary,
            reject_unsupported_tags: true,
        },
    )?)
}

// Version 2.0 lacks the numeric-spelling options introduced in the yanked 2.1.
// Quote only implicit plain scalar tokens identified by the YAML parser. Byte
// spans keep comments, Unicode, anchors, explicit tags and block strings intact.
fn preserve_numeric_strings(text: &str) -> Result<Cow<'_, str>> {
    use serde_saphyr::granit_parser::{Event, Parser, ScalarStyle};
    let mut replacements = Vec::new();
    for event in Parser::new_from_str(text) {
        let (event, span) = event?;
        if let Event::Scalar(value, ScalarStyle::Plain, _, None) = event
            && numeric_string(&value)
        {
            let range = span.byte_range().context("YAML scalar has no byte range")?;
            anyhow::ensure!(
                text.get(range.clone()) == Some(value.as_ref()),
                "YAML scalar source range does not match its value"
            );
            replacements.push((range, serde_json::to_string(value.as_ref())?));
        }
    }
    if replacements.is_empty() {
        return Ok(Cow::Borrowed(text));
    }
    let mut compatible = text.to_owned();
    for (range, replacement) in replacements.into_iter().rev() {
        compatible.replace_range(range, &replacement);
    }
    Ok(Cow::Owned(compatible))
}

fn numeric_string(value: &str) -> bool {
    let digits = value
        .strip_prefix('-')
        .or_else(|| value.strip_prefix('+'))
        .unwrap_or(value);
    let leading_zero = digits.len() > 1
        && digits.starts_with('0')
        && digits.bytes().all(|byte| byte.is_ascii_digit());
    let underscored = digits.starts_with(|ch: char| ch.is_ascii_digit())
        && digits.contains('_')
        && digits
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '+' | '-'));
    leading_zero || underscored
}

pub(crate) fn from_slice<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    from_str(std::str::from_utf8(bytes)?)
}

pub(crate) fn to_string(value: &impl serde::Serialize) -> Result<String, serde_saphyr::ser::Error> {
    serde_saphyr::to_string(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apm_documents_preserve_unknown_fields_and_scalar_types_on_round_trip() {
        for document in [
            include_str!("../../tests/fixtures/apm-0.33.0-lock.yaml"),
            include_str!("../../tests/fixtures/apm-yaml-fragment.yaml"),
        ] {
            let value: Value = from_slice(document.as_bytes()).unwrap();
            let serialized = to_string(&value).unwrap();
            assert_eq!(from_str::<Value>(&serialized).unwrap(), value);
        }
        let value: Value =
            from_str(include_str!("../../tests/fixtures/apm-yaml-fragment.yaml")).unwrap();
        assert_eq!(value["numeric_identifier"], "0755");
        assert_eq!(value["underscored_identifier"], "1_000");
        assert_eq!(value["dependencies"]["mcp"][0]["<<"], value["defaults"]);
        assert_eq!(
            value["dependencies"]["mcp"][0]["env"]["LITERAL"],
            "${KEEP_THIS_LITERAL}"
        );
        assert_eq!(value["notes"], "first line\nsecond line\n");
    }

    #[test]
    fn ambiguous_or_unrepresentable_yaml_fails_without_silent_coercion() {
        for document in [
            "key: 1\nkey: 2",
            "key: .nan",
            "key: !foreign value",
            "? [a, b]\n: value",
            "[unclosed",
        ] {
            assert!(from_str::<Value>(document).is_err(), "{document}");
        }
        assert!(from_slice::<Value>(&[0xff]).is_err());
    }

    #[test]
    fn scalar_compatibility_preserves_source_boundaries_and_explicit_types() {
        let document = "label: café\nplain: &id 0755 # preserve comment\nalias: *id\nexplicit: !!int 755\nfloat: 0.5\nzero: 0\nsigned: -007\nscientific: 1_000.5\nflag: on\nblock: |\n  0755\n";
        let value: Value = from_str(document).unwrap();
        assert_eq!(value["plain"], "0755");
        assert_eq!(value["alias"], "0755");
        assert_eq!(value["signed"], "-007");
        assert_eq!(value["scientific"], "1_000.5");
        assert_eq!(value["explicit"], 755);
        assert_eq!(value["float"], 0.5);
        assert_eq!(value["zero"], 0);
        assert_eq!(value["flag"], "on");
        assert_eq!(value["block"], "0755\n");
        assert_eq!(
            preserve_numeric_strings("ordinary: true\n").unwrap(),
            "ordinary: true\n"
        );
        assert!(numeric_string("+00"));
        assert!(numeric_string("0xFF_FF"));
        assert!(!numeric_string("ordinary"));
        assert!(!numeric_string("file_1"));
        assert!(!numeric_string("1_2:3"));
        assert!(!numeric_string(""));
    }
}
