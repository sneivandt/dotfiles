---
name: creating-skills
description: >
  Guide for creating new GitHub Copilot Agent Skills in the dotfiles project.
  Use when adding new skills or modifying existing skill structure.
metadata:
  author: sneivandt
  version: "2.0"
---

# Creating GitHub Copilot Agent Skills

Skills are modular documentation units in `.github/skills/` that give AI assistants focused guidance on specific topics.

## Skill Structure

```
.github/skills/
├── skill-name/
│   └── SKILL.md
└── ...
```

Every `SKILL.md` starts with YAML frontmatter:

```markdown
---
name: skill-name
description: >
  Brief description of what the skill covers and when to use it.
metadata:
  author: sneivandt
  version: "1.0"
---

# Skill Title

Content goes here...
```

### Frontmatter Fields

- **`name`** (required): Kebab-case identifier matching the directory name
- **`description`** (required): Multi-line description using `>` for folding
- **`metadata.author`** (required): Author username
- **`metadata.version`** (required): Semantic version string

## When to Create a Skill

Create a skill when the topic is complex, repeated across files, or has common pitfalls. Good topics: Rust patterns, configuration formats, architecture concepts, development workflows.

## Content Guidelines

1. **Overview**: Brief intro
2. **Core Content**: Organized with clear headings
3. **Code Examples**: Rust, shell, or INI as appropriate
4. **Rules**: Clear dos and don'ts
5. **Cross-references**: Links to related skills

Keep skills **concise and focused** on patterns agents need for code generation. Most skills should be under 100 lines; complex topics (e.g., sparse checkout) may be longer when the detail is necessary.

## Creating a New Skill

```bash
mkdir -p .github/skills/my-new-skill
```

Include code examples from the actual codebase:

```rust
// Rust task example
pub struct MyTask;
impl Task for MyTask {
    fn name(&self) -> &str { "My task" }
    fn should_run(&self, ctx: &Context) -> bool { true }
    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        Ok(TaskResult::Ok)
    }
}
```

```ini
# INI config example
[base]
entry-one
entry-two
```

After creating, add the skill to `.github/copilot-instructions.md` and validate with `./dotfiles.sh test`.

## Rules

- Every skill must have valid YAML frontmatter
- Directory name must match the `name` field
- Skills should be self-contained and concise (aim for under 100 lines; longer is acceptable for complex topics)
- Examples must use actual project patterns (Task trait, Context, exec helpers)
- Cross-reference related skills where appropriate
- **Write in terms of the current state** — never describe something as "new", "changed", or "different from before"; document what exists, not how it evolved
- Validate with `./dotfiles.sh test` before committing
