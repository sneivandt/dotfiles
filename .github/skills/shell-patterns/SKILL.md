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
- `log_dry_run "Would <action>"` - Dry-run actions (always shown in dry-run mode)

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

## File Formatting

**No Trailing Whitespace**: Never leave trailing whitespace at the end of lines.
- This applies to all file types
- Trailing whitespace causes unnecessary git diffs
- Most editors can be configured to automatically remove trailing whitespace on save
