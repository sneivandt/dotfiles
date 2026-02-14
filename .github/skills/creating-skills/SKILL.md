---
name: creating-skills
description: >
  Guide for creating new GitHub Copilot Agent Skills in the dotfiles project.
  Use when adding new skills or modifying existing skill structure.
metadata:
  author: sneivandt
  version: "1.0"
---

# Creating GitHub Copilot Agent Skills

This skill provides guidance for creating and managing GitHub Copilot Agent Skills in the dotfiles project.

## What Are GitHub Copilot Agent Skills?

GitHub Copilot Agent Skills are modular, reusable documentation units that provide AI code assistants with detailed guidance on specific topics. They help maintain consistency and provide focused expertise for particular areas of the codebase.

## Skill Structure

Each skill is stored in its own directory under `.github/skills/` and contains a `SKILL.md` file.

### Directory Layout
```
.github/skills/
├── skill-name/
│   └── SKILL.md
├── another-skill/
│   └── SKILL.md
└── ...
```

### File Format

Every `SKILL.md` file must start with YAML frontmatter followed by markdown content:

```markdown
---
name: skill-name
description: >
  Brief description of what the skill covers and when to use it.
  Can span multiple lines.
metadata:
  author: sneivandt
  version: "1.0"
---

# Skill Title

Content goes here...
```

### YAML Frontmatter Fields

- **`name`** (required): Kebab-case identifier matching the directory name
  - Example: `creating-skills`, `shell-patterns`, `ini-configuration`
- **`description`** (required): Single or multi-line description using `>` for folding
  - Explain what the skill covers
  - Describe when to use it
- **`metadata.author`** (required): Author username
- **`metadata.version`** (required): Version string (use semantic versioning)

## When to Create a Skill

Create a new skill when:
- **Topic is complex**: The topic requires detailed explanation beyond a few sentences
- **Pattern is repeated**: The same guidance applies across multiple files or scenarios
- **Topic is self-contained**: The subject can be explained independently
- **Documentation exists**: There's already documentation that could be condensed into skill format
- **Common mistakes occur**: The topic has pitfalls that need careful explanation

Examples of good skill topics:
- Coding patterns and conventions (shell-patterns, powershell-patterns)
- Configuration formats (ini-configuration)
- System architecture (profile-system)
- Development workflows (testing-patterns, git-hooks-patterns)

## Skill Content Guidelines

### Structure
1. **Overview Section**: Brief introduction explaining what the skill covers
2. **Core Content**: Main guidance organized with clear headings
3. **Examples**: Code examples showing correct usage
4. **Rules/Guidelines**: Clear dos and don'ts
5. **Cross-references**: Links to related skills or documentation

### Writing Style
- **Be concise**: Skills are focused documentation, not comprehensive guides
- **Use examples**: Show code patterns rather than just describing them
- **Be specific**: Provide actionable guidance, not generic advice
- **Use consistent formatting**: Match the style of existing skills
- **Include context**: Explain *why* patterns exist, not just *what* they are

### Length Guidelines
- **Minimum**: ~100 lines for focused topics
- **Typical**: 130-170 lines for most skills
- **Maximum**: Keep under 300 lines; split into multiple skills if longer

Existing skills for reference:
- `profile-system`: 121 lines
- `ini-configuration`: 129 lines
- `shell-patterns`: 156 lines
- `powershell-patterns`: 171 lines

## Creating a New Skill: Step by Step

### 1. Create Directory
```bash
mkdir -p .github/skills/my-new-skill
```

### 2. Create SKILL.md File
```bash
cat > .github/skills/my-new-skill/SKILL.md << 'EOF'
---
name: my-new-skill
description: >
  Brief description here.
metadata:
  author: sneivandt
  version: "1.0"
---

# Skill Title

Content...
EOF
```

### 3. Structure the Content
- Start with overview section
- Add main content with clear headings
- Include code examples
- Add rules and guidelines
- Cross-reference related skills

### 4. Update References
Add the skill to `.github/copilot-instructions.md` in the skills list:
```markdown
> **Note**: This project uses GitHub Copilot Agent Skills for detailed technical guidance. See `.github/skills/` for:
> - `ini-configuration` - Working with INI configuration files
> - `shell-patterns` - Shell scripting patterns and conventions
> - `powershell-patterns` - PowerShell scripting patterns and conventions
> - `profile-system` - Understanding the profile system
> - `my-new-skill` - Description of my new skill
```

### 5. Test the Skill
- Ensure YAML frontmatter is valid
- Check markdown formatting
- Verify examples are correct
- Run tests: `./dotfiles.sh -T`

## Naming Conventions

### Skill Names (Kebab-Case)
- Use lowercase letters, numbers, and hyphens only
- Be descriptive but concise
- Examples: `creating-skills`, `shell-patterns`, `git-hooks-patterns`

### Section Headers (Title Case)
- Use standard markdown headings (`#`, `##`, `###`)
- Be descriptive and scannable
- Examples: "Creating a New Skill", "Code Examples", "Rules and Guidelines"

## Code Examples

Always include code examples to illustrate concepts:

### Shell Examples
```sh
# Show real patterns from the codebase
my_task()
{(
  log_stage "Task name"
  # Task implementation
)}
```

### PowerShell Examples
```powershell
# Show real patterns from the codebase
function Install-Something {
  [CmdletBinding()]
  param([switch]$DryRun)
  # Function implementation
}
```

### Configuration Examples
```ini
# Show real INI patterns
[profile-name]
entry-one
entry-two
```

## Skill Maintenance

### Updating Existing Skills
1. Increment version number in metadata
2. Update content as needed
3. Maintain backward compatibility where possible
4. Test changes thoroughly

### Deprecating Skills
If a skill becomes obsolete:
1. Add deprecation notice at the top
2. Point to replacement skill if applicable
3. Keep the skill file for a grace period
4. Eventually remove and update references

## Integration with Copilot Instructions

Skills are referenced from `.github/copilot-instructions.md`, which serves as the main entry point. The instructions file:
- Lists all available skills with brief descriptions
- Provides high-level guidance
- Delegates detailed patterns to skills

This separation keeps the main instructions file concise while allowing skills to provide deep, focused expertise.

## External Skills

The project also supports external GitHub Copilot CLI skills via `conf/copilot-skills.ini`. These are downloaded from GitHub repositories and installed to `~/.copilot/skills/`. See the `CopilotSkills.psm1` module for implementation details.

## Rules

- Every skill must have valid YAML frontmatter
- Skill directory name must match the `name` field in frontmatter
- Skills should be self-contained and focused
- Examples must be relevant to the dotfiles project
- Cross-reference related skills where appropriate
- Keep skills updated as codebase patterns evolve
- Test skill changes by running the test suite
