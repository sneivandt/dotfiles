# Documentation Structure

This document explains how documentation is organized in this project and when to use each type.

## Three Types of Documentation

This project maintains three distinct types of documentation, each serving a specific purpose:

### 1. Copilot Instructions (`.github/copilot-instructions.md`)

**Purpose**: Core universal guidance for AI code assistants

**Target Audience**: GitHub Copilot and other AI agents

**Content**:
- Brief project overview and core principles
- High-level workflow guidance for agents
- Pointers to skills and docs for detailed information
- Universal code quality and security requirements
- Available skills reference list

**Characteristics**:
- Short (~70 lines)
- Universal principles applicable to all tasks
- Acts as an index/router to detailed content
- Updated infrequently

**When to read this**: AI agents should always start here for project context.

### 2. Agent Skills (`.github/skills/`)

**Purpose**: Agent-specific technical patterns and coding conventions

**Target Audience**: Primarily AI code assistants, but also useful for developers learning patterns

**Content**:
- Coding patterns and conventions (shell, PowerShell)
- Technical specifications (INI format, profile system)
- API and interface details
- Code style rules
- Programmatic procedures
- "How to write code that fits this project"

**Characteristics**:
- Technical and precise
- Focused on implementation patterns
- Each skill covers a specific technical domain
- Includes Rules sections for agents
- Updated when patterns evolve

**Available Skills**:
- `creating-skills` - Creating new GitHub Copilot Agent Skills
- `customization-guide` - Programmatically adding configuration items
- `git-hooks-patterns` - Git hooks and sensitive data detection
- `ini-configuration` - Working with INI configuration files
- `logging-patterns` - Logging conventions and patterns
- `package-management` - Package installation patterns
- `profile-system` - Understanding the profile system
- `rust-patterns` - Rust coding patterns for the core engine
- `shell-patterns` - Shell scripting patterns and conventions
- `symlink-management` - Detailed symlink conventions
- `testing-patterns` - Testing conventions and validation

**When to read these**: When writing or modifying code, check relevant skills for established patterns.

### 3. Documentation (` docs/`)

**Purpose**: Human-readable guides and reference documentation

**Target Audience**: Humans (end users and contributors), but also useful for AI agents needing context

**Content**:
- User guides and tutorials
- Complete workflows with explanations
- Architecture and design rationale
- Troubleshooting guides
- Contributing procedures
- Platform-specific instructions
- "How to use this project"

**Characteristics**:
- Narrative and explanatory
- Includes context and rationale
- Examples with full procedures
- Updated as features change

**Available Documentation**:
- `USAGE.md` - Installation and usage guide
- `PROFILES.md` - Understanding and using profiles
- `CONFIGURATION.md` - Configuration file reference
- `CUSTOMIZATION.md` - Adding files, packages, and profiles
- `TROUBLESHOOTING.md` - Common issues and solutions
- `CONTRIBUTING.md` - Contribution guidelines
- `ARCHITECTURE.md` - Implementation and design details
- `TESTING.md` - Testing procedures and CI
- `WINDOWS.md` - Windows-specific documentation
- `DOCKER.md` - Docker image usage and building
- `HOOKS.md` - Repository git hooks
- `SECURITY.md` - Security policy and best practices

**When to read these**: For understanding how to use the project, contribute changes, or troubleshoot issues.

## Content Decision Tree

### Should this be in copilot-instructions.md?

Ask these questions:
- Is it a universal principle that applies to ALL agent tasks? → **Instructions**
- Is it a brief pointer to where detailed info lives? → **Instructions**
- Is it about security/safety that must always be followed? → **Instructions**
- Otherwise → **NOT instructions**

### Should this be a Skill?

Ask these questions:
- Is it a technical coding pattern or convention? → **Skill**
- Does it specify how agents should write code? → **Skill**
- Is it an API specification or format definition? → **Skill**
- Is it procedural knowledge for code generation? → **Skill**
- Is it mainly for human understanding? → **NOT a skill**

### Should this be in docs/?

Ask these questions:
- Is it a user guide or tutorial? → **Docs**
- Does it explain "how to use" rather than "how to code"? → **Docs**
- Does it include context, rationale, or design decisions? → **Docs**
- Is it troubleshooting or FAQ content? → **Docs**
- Is it contribution guidelines for humans? → **Docs**

## Cross-Referencing Guidelines

### From Instructions
- Reference skills for technical patterns: "See the `shell-patterns` skill for details"
- Reference docs for procedures: "See `docs/CONTRIBUTING.md` for contribution workflow"

### From Skills
- Reference other skills for related patterns: "See the `profile-system` skill for filtering"
- Reference docs for user context: "For human-focused customization procedures, see `docs/CUSTOMIZATION.md`"
- Don't duplicate content from other skills - cross-reference instead

### From Docs
- Reference skills when mentioning technical patterns: "Follows patterns in the `shell-patterns` skill"
- Reference other docs for related topics: "See [Testing Documentation](TESTING.md) for details"
- Don't duplicate skills content - link to them when appropriate

## Maintenance Guidelines

### When Adding New Content

1. **Determine the type first**: Use the decision tree above
2. **Check for existing coverage**: Don't duplicate content
3. **Add cross-references**: Link to related content
4. **Update indexes**: Update this file and `docs/README.md` if needed

### When Updating Existing Content

1. **Keep it in the right place**: Don't let skills become user guides
2. **Maintain consistency**: Use established patterns
3. **Update cross-references**: If content moves, update all references
4. **Version skills**: Bump version in YAML frontmatter when making significant changes

### Red Flags

**In copilot-instructions.md**:
- ❌ More than ~100 lines
- ❌ Detailed code examples
- ❌ Step-by-step procedures
- ❌ Repository structure details

**In skills**:
- ❌ Tutorial or guide tone ("Now let's...")
- ❌ Troubleshooting by symptoms
- ❌ Design rationale and history
- ❌ End-user procedures

**In docs**:
- ❌ Just code patterns without context
- ❌ Internal agent instructions
- ❌ Pure API specifications

## Examples

### Example 1: Adding a New Package

**Instructions** mentions: "Use skills for technical details"
**Skill** (`package-management`) provides: Code patterns for implementing package installation
**Docs** (`CUSTOMIZATION.md`) explains: Complete user procedure with examples

### Example 2: Shell Script Conventions

**Instructions** mentions: "See `shell-patterns` skill for details"
**Skill** (`shell-patterns`) provides: Detailed coding patterns, quoting rules, function structure
**Docs** (various) use: These patterns but don't re-document them

### Example 3: Troubleshooting

**Instructions**: Not mentioned (not universal to all tasks)
**Skills**: Not covered (not coding patterns)
**Docs** (`TROUBLESHOOTING.md`): Complete guide with symptoms, causes, and solutions

## Benefits of This Structure

1. **Clear Separation**: Each type serves its purpose without overlap
2. **Maintainable**: Changes in one area don't require updates everywhere
3. **Discoverable**: Clear pointers help find the right information
4. **Appropriate Depth**: Right level of detail for each audience
5. **DRY Principle**: Single source of truth for each piece of information

## See Also

- [Documentation Index](README.md) - Complete list of documentation files
- [Contributing Guide](CONTRIBUTING.md) - How to contribute to documentation
- `.github/skills/creating-skills/SKILL.md` - How to create new skills
