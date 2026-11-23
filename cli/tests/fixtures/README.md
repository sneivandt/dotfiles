# Integration test fixtures

These files contain synthetic public values, never machine exports.

## APM compatibility fixtures

`apm-0.33.0-lock.yaml` models the supported lock fields in
[APM v0.33.0 lockfile.py](https://github.com/microsoft/apm/blob/v0.33.0/src/apm_cli/deps/lockfile.py).
It exercises workflow ownership, legacy Cowork records, and preservation of
unknown metadata. Update the versioned fixture when adopting a different
upstream schema; do not treat this reduced example as a complete APM validator.

`apm-yaml-fragment.yaml` covers scalar types, aliases, literal merge fields,
multiline strings, and environment placeholders at the YAML parser boundary.

## Profile symlink fixtures

`base_profile.toml` is a minimal `symlinks.toml` for base-profile integration
tests, declaring a single `bashrc` symlink under `[base]`.

`desktop_profile.toml` is a minimal `symlinks.toml` for desktop-profile
integration tests, declaring entries under both `[base]` (`bashrc`) and
`[desktop]` (`config/Code/User/settings.json`).
