# Skill metadata validator

[check-skills.py](check-skills.py) validates the repository's deliberately small
frontmatter convention. The native checker and Linux docs stage run it and
[test-check-skills.py](test-check-skills.py).

Frontmatter accepts only a plain lowercase hyphenated `name` and a
two-space-indented YAML block `description` (`>` or `|`, optionally with
chomping). Names must match their directories and be unique. The validator
rejects unsupported forms, missing or duplicate keys, and empty descriptions;
it is not a general YAML parser.

Extend the convention and regression fixtures together when new metadata is
needed. Run the fixtures when changing the validator or its convention. Select
commands from [docs/README.md](../../../../docs/README.md#checks). Metadata
validation does not establish instruction accuracy or routing effectiveness.
