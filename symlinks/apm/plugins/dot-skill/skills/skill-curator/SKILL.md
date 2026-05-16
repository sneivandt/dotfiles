---
name: skill-curator
description: Use when creating, reviewing, updating, or refactoring agent skills, plugins, prompts, or agent instructions. Prefer concise, composable skills with clear triggers and minimal overlap.
---

# Skill Curator

Use this skill for skill and plugin authoring, maintenance, cleanup, and refactoring.

## Preferences

- Keep skills concise, high-level, and focused on stable preferences or reusable workflows.
- Make the front matter description clearly explain when the skill should be used.
- Prefer composable skills with clear boundaries over broad skills that duplicate global or project instructions.
- Avoid over-specific skills unless the task recurs often and benefits from dedicated context.
- Check for overlap with existing skills before adding or expanding one.
- Prefer updating an existing related skill over creating a near-duplicate.
- Keep operational details, examples, and references only when they materially improve future agent behavior.
- Do not include secrets, private tokens, or sensitive data in skills or bundled assets.

## Maintenance Checks

- Confirm the skill folder, front matter `name`, and plugin references use consistent names.
- Confirm the skill is installed or referenced by the relevant plugin/config.
- Remove stale references, duplicated guidance, and instructions that are already covered elsewhere.
- When reviewing skills, call out whether each change is necessary, optional, or not worth doing.
