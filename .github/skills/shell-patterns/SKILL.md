---
name: shell-patterns
description: >
  Shell scripting patterns and conventions for the dotfiles project.
  Use when creating or modifying shell scripts in src/linux/ or symlinks/.
metadata:
  author: sneivandt
  version: "1.0"
---

# Shell Scripting Patterns

This skill provides shell scripting patterns and conventions used in the dotfiles project.

## Script Header

Always start new shell scripts with:
```sh
#!/bin/sh
set -o errexit
set -o nounset
```

Use `#!/bin/sh` unless there is a compelling reason for Bash. If Bash required, document it.

## Code Style

### Conditional Statements
Always use compact style with `then` on the same line:
```sh
if [ condition ]; then
  # code
fi
```

NOT:
```sh
if [ condition ]
then
  # code
fi
```

### Variable Quoting
- Use double quotes around variable expansions except when intentional word splitting is required
- Add a shellcheck directive comment when word splitting is intentional

### POSIX Compatibility
- Avoid process substitution and arrays (Bash features) in POSIX scripts
- Always quote glob patterns when iterating variable-expanded paths

## Logging

Use existing helpers instead of ad-hoc echo statements:

- `log_stage "Stage Name"` - Stage headers (prints once per stage with `::` prefix)
  - Uses `_work` flag to print only once per subshell
  - Resets automatically in new subshell
- `log_verbose "Message"` - Verbose details (only shown with `-v` flag)
- `log_error "Error"` - Error messages (exits with status 1)
- `log_dry_run "Would <action>"` - Dry-run actions (automatically shown when `is_dry_run` is true)

**Note**: When using `log_dry_run`, you still need to check `is_dry_run` before performing actions. The logging helper only controls message output, not execution logic. Always wrap actual work in an `else` block:

## Task Function Pattern

Always wrap task functions in subshell `( )` for environment isolation:

```sh
my_task()
{(
  # Check prerequisites
  if ! is_program_installed "tool"; then
    log_verbose "Skipping task: tool not installed"
    return
  fi

  # Check if config exists
  if [ ! -f "$DIR"/conf/config.ini ]; then
    log_verbose "Skipping task: no config.ini"
    return
  fi

  # Do work (log_stage prints once per subshell)
  log_stage "Task name"

  # Dry-run pattern
  if is_dry_run; then
    log_dry_run "Would perform action"
  else
    log_verbose "Performing action"
    # actual work
  fi
)}
```

Benefits of subshell pattern:
- Isolates variables and directory changes
- Isolates `_work` flag state
- Each subshell gets fresh `_work` flag, so `log_stage` prints once per task

## Idempotency

Always check if action is needed before taking it:
- Check file existence, symlink targets, package installation status
- Skip with verbose log if already correct: `log_verbose "Skipping: already correct"`

## Dry-Run Pattern

Check `is_dry_run` before system modifications:
```sh
if is_dry_run; then
  log_dry_run "Would perform action"
else
  log_verbose "Performing action"
  # actual work
fi
```

## Helper Predicates

### Guard Optional Tools
Use `is_program_installed` predicate before using external tools:
```sh
if ! is_program_installed "tool"; then
  log_verbose "Skipping: tool not installed"
  return
fi
```

### Common Predicates
From `src/linux/utils.sh`:
- `is_program_installed` - Check if program exists
- `is_dry_run` - Check if running in dry-run mode
- `should_include_profile_tag` - Check if section/profile should be processed

### Profile Filtering Pattern
Use `should_include_profile_tag` to check if a section should be processed. The function returns 0 (success) if the section should be included, 1 (failure) if it should be skipped:

```sh
# Positive check - process if section is included
if should_include_profile_tag "$section"; then
  # Process this section
fi

# Negative check - skip if section is not included
if ! should_include_profile_tag "$section"; then
  log_verbose "Skipping section [$section]: profile not included"
  continue
fi
```

Both patterns are valid. Use the positive check when you want to focus on the processing logic, and the negative check when you want to explicitly skip early in a loop. See the `profile-system` skill for details on how profile filtering works.

## Package Management

Prefer constructing minimal lists before calling system package managers:
```sh
packages_to_install=""
for package in $all_packages; do
  if ! is_package_installed "$package"; then
    packages_to_install="$packages_to_install $package"
  fi
done

if [ -n "$packages_to_install" ]; then
  pacman -S $packages_to_install  # Intentional word splitting
fi
```

## Rules

- All shell scripts must use `#!/bin/sh` for POSIX compatibility
- Always use compact conditional style with `then` on same line
- Quote all variable expansions except when intentional word splitting is needed
- Use logging helpers instead of direct echo statements
- Wrap task functions in subshells for isolation
- Check idempotency before performing actions
- Support dry-run mode with `is_dry_run` checks
- Guard optional tools with `is_program_installed`
- Construct minimal package lists before calling package managers
