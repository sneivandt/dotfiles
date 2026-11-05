# APM compatibility fixtures

These files contain synthetic public values, never machine exports.

`apm-0.33.0-lock.yaml` models the supported lock fields in
[APM v0.33.0 lockfile.py](https://github.com/microsoft/apm/blob/v0.33.0/src/apm_cli/deps/lockfile.py).
It exercises workflow ownership, legacy Cowork records, and preservation of
unknown metadata. Update the versioned fixture when adopting a different
upstream schema; do not treat this reduced example as a complete APM validator.

`apm-yaml-fragment.yaml` covers scalar types, aliases, literal merge fields,
multiline strings, and environment placeholders at the YAML parser boundary.
