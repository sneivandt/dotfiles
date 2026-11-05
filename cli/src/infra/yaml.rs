//! YAML boundary for APM documents with string-keyed mappings.
//!
//! Keep numeric-looking identifiers as strings, reject ambiguous duplicate keys
//! and unsupported tags, and preserve literal merge fields for native APM.

use serde::de::DeserializeOwned;
pub(crate) use serde_json::Value;
pub(crate) type Mapping = serde_json::Map<String, Value>;

pub(crate) fn from_str<T: DeserializeOwned>(text: &str) -> Result<T, serde_saphyr::Error> {
    serde_saphyr::from_str_with_options(
        text,
        serde_saphyr::options! {
            leading_zero_numbers_as_strings: true,
            underscored_numbers_as_strings: true,
            merge_keys: serde_saphyr::MergeKeyPolicy::AsOrdinary,
            reject_unsupported_tags: true,
        },
    )
}

pub(crate) fn from_slice<T: DeserializeOwned>(bytes: &[u8]) -> anyhow::Result<T> {
    Ok(from_str(std::str::from_utf8(bytes)?)?)
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
}
