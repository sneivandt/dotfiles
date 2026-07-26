//! Config entries that accept either a bare string or an explicit table.
//!
//! Several sections let an entry be written in a shorthand string form or a
//! structured table form, for example in `conf/symlinks.toml`:
//!
//! ```toml
//! symlinks = [
//!   "bashrc",                                  # shorthand
//!   { source = "foo", target = ".bar" },       # explicit table
//! ]
//! ```
//!
//! The obvious encoding is `#[serde(untagged)]`, but untagged enums ignore
//! `deny_unknown_fields`: serde buffers the input and silently falls through to
//! the next variant, so a misspelled key such as `targett` is accepted and the
//! entry is parsed as if the key were absent. Silently discarding a mistyped
//! key produces a machine state that does not match the file on disk, which is
//! exactly the failure mode declarative configuration is meant to prevent.
//!
//! [`StringOrTable`] dispatches on the TOML value kind instead. A table is
//! always deserialized as `T`, so `T`'s `deny_unknown_fields` is enforced and
//! the reported error names the offending key.

use serde::{Deserialize, Deserializer, de::DeserializeOwned, de::Error as _};

/// A config entry written either as a bare string or as an explicit table.
///
/// `T` is the table form and should derive `Deserialize` with
/// `#[serde(deny_unknown_fields)]` so unknown keys are rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StringOrTable<T> {
    /// Shorthand form: a bare string.
    Bare(String),
    /// Explicit form: a table deserialized as `T`.
    Table(T),
}

impl<'de, T> Deserialize<'de> for StringOrTable<T>
where
    T: DeserializeOwned,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = toml::Value::deserialize(deserializer)?;
        #[allow(
            clippy::wildcard_enum_match_arm,
            reason = "any value kind other than string or table is invalid, including future ones"
        )]
        match value {
            toml::Value::String(bare) => Ok(Self::Bare(bare)),
            table @ toml::Value::Table(_) => T::deserialize(table)
                .map(Self::Table)
                .map_err(D::Error::custom),
            other => Err(D::Error::custom(format!(
                "expected a string or a table, found {}",
                other.type_str()
            ))),
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers and direct indexing"
)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    struct Pair {
        source: String,
        target: String,
    }

    #[derive(Debug, Deserialize)]
    struct Doc {
        entries: Vec<StringOrTable<Pair>>,
    }

    fn parse(toml_str: &str) -> Result<Doc, toml::de::Error> {
        toml::from_str(toml_str)
    }

    #[test]
    fn bare_string_uses_shorthand_variant() {
        let doc = parse(r#"entries = ["bashrc"]"#).unwrap();
        assert_eq!(
            doc.entries[0],
            StringOrTable::Bare("bashrc".to_string()),
            "a bare string should parse as the shorthand variant"
        );
    }

    #[test]
    fn table_uses_explicit_variant() {
        let doc = parse(r#"entries = [{ source = "a", target = "b" }]"#).unwrap();
        assert_eq!(
            doc.entries[0],
            StringOrTable::Table(Pair {
                source: "a".to_string(),
                target: "b".to_string(),
            }),
            "a table should parse as the explicit variant"
        );
    }

    #[test]
    fn unknown_key_in_table_is_rejected() {
        let err = parse(r#"entries = [{ source = "a", target = "b", targett = "c" }]"#)
            .expect_err("an unknown key must not be silently ignored");
        let message = err.to_string();
        assert!(
            message.contains("targett"),
            "error should name the unknown key, got: {message}"
        );
    }

    #[test]
    fn missing_required_key_in_table_is_rejected() {
        let err = parse(r#"entries = [{ source = "a" }]"#)
            .expect_err("a missing required key must be rejected");
        let message = err.to_string();
        assert!(
            message.contains("target"),
            "error should name the missing key, got: {message}"
        );
    }

    #[test]
    fn non_string_non_table_is_rejected_with_type_name() {
        let err = parse("entries = [42]").expect_err("an integer entry must be rejected");
        let message = err.to_string();
        assert!(
            message.contains("expected a string or a table"),
            "error should explain the accepted forms, got: {message}"
        );
        assert!(
            message.contains("integer"),
            "error should name the offending type, got: {message}"
        );
    }
}
