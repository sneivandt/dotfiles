# Skill metadata validator

[check-skills.py](check-skills.py) validates repository-local skills under
`.agents/skills/*/SKILL.md` only (not APM plugin skills under `symlinks/apm/`).
The native checker and Linux docs stage run it alongside
[test-check-skills.py](test-check-skills.py).

Frontmatter accepts only a plain lowercase hyphenated `name` and a
two-space-indented YAML block `description` (`>` or `|`, optionally with
chomping). Names must match their directories and be unique. The validator
rejects unsupported forms, missing or duplicate keys, and empty descriptions;
it is not a general YAML parser.

Extend the convention and regression fixtures together when new metadata is
needed. Select commands from [docs/README.md](../../../../docs/README.md#checks).
